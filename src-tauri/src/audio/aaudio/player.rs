//! AAudio 独占（位完美）输出播放器
//!
//! 与 Windows 的 WASAPI 独占模式对应：绕过 AudioFlinger 的混音与重采样，
//! 把解码后的 PCM 按**设备原生采样率**直接写进 USB DAC。
//!
//! 三个关键点：
//! 1. **共享模式**：`AAUDIO_SHARING_MODE_EXCLUSIVE`。共享模式下 AAudio 一定会把
//!    输出重采样到设备速率（典型 48kHz），44.1kHz 的唱片就会被"洗"一遍；
//!    独占模式下 AAudio 不接受非原生速率，速率不对会直接开不出流——这也正好
//!    用来判断"这条链路是不是真的位完美"。
//! 2. **速率匹配**：`ensure_format()` 先按曲目原生采样率开流，开不出来再退到
//!    设备支持的其它速率（此时由解码线程重采样，前端显示的状态会说明）。
//! 3. **热插拔**：USB DAC 拔掉时 AAudio 会抛 `Disconnected`，由看门狗线程收尾；
//!    Kotlin 的设备回调再触发 `super::on_audio_route_changed()` 决定回落或重连。
//!
//! 数据通路：解码线程 `push_samples` → 无锁 SPSC 环形缓冲 → AAudio 数据回调取走
//! （回调里做音量与淡入淡出，绝不阻塞、不分配）。

// 与 `wasapi/exclusive.rs` 的 `mod simd_convert` 同一策略：整个模块就是 AAudio 的
// FFI 边界——裸指针解引用、C 回调 ABI、跨线程共享的 `UnsafeCell` 都必须用 unsafe，
// 逐项标注只会重复同一句话，故在模块级统一 allow。该 allow 仅限本文件：
// Cargo.toml 里全局的 `unsafe_code = "warn"` 保持不变，模块外新增的 unsafe
// 依旧会被提示；每个 unsafe 块内部仍保留独立的 SAFETY 说明。
#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::os::raw::c_void;
use std::ptr;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU8, AtomicU32, AtomicU64, Ordering,
};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::device::{
    OutputDeviceInfo, find_usb_output_device, pick_channel_count, pick_sample_rate,
};
use super::ffi;
use crate::audio::{PlaybackState, SampleRing};
use crate::error::AppError;

/// 环形缓冲的目标时长（秒）。独占模式的 buffer 通常很小（低延迟），
/// 缓冲给足可以避免解码线程偶发抖动导致的断流，同时 seek/切歌清空也不拖沓。
const RING_SECONDS: f32 = 1.5;
/// 默认淡入淡出时长（毫秒）
const DEFAULT_FADE_MS: u32 = 30;
/// 看门狗轮询间隔（毫秒）
const WATCHDOG_INTERVAL_MS: u64 = 20;

/// `PlaybackState` 的原子编码（状态要能在音频回调里更新，不能用 Mutex）
const ST_UNINITIALIZED: u8 = 0;
const ST_STOPPED: u8 = 1;
const ST_PLAYING: u8 = 2;
const ST_PAUSED: u8 = 3;
const ST_STOPPING: u8 = 4;
const ST_PAUSING: u8 = 5;

const fn state_from_code(c: u8) -> PlaybackState {
    match c {
        ST_STOPPED => PlaybackState::Stopped,
        ST_PLAYING => PlaybackState::Playing,
        ST_PAUSED => PlaybackState::Paused,
        ST_STOPPING => PlaybackState::Stopping,
        ST_PAUSING => PlaybackState::Pausing,
        _ => PlaybackState::Uninitialized,
    }
}

/// 淡出完成后要执行的动作（由看门狗在回调线程之外执行）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeAction {
    Pause,
    Stop,
}

const PENDING_NONE: u8 = 0;
const PENDING_PAUSE: u8 = 1;
const PENDING_STOP: u8 = 2;

#[derive(Debug, Clone, Copy)]
enum FadeState {
    Idle,
    Out {
        remaining: usize,
        total: usize,
        action: FadeAction,
    },
    In {
        remaining: usize,
        total: usize,
    },
}

/// 音频回调与命令线程共享的状态（全部是无锁字段）
struct Shared {
    ring: SampleRing,
    /// 已交付给 AAudio 的采样数（交织后，即 帧数 × 声道数）
    written: AtomicU64,
    /// 音量，f32 位模式
    volume: AtomicU32,
    channels: AtomicU32,
    format: AtomicI32,
    fade: Mutex<FadeState>,
    /// 淡出完成后待执行的动作
    pending: AtomicU8,
    underruns: AtomicU64,
    disconnected: AtomicBool,
    /// 流是否处于可写状态（关闭/重建期间置 false）
    running: AtomicBool,
}

/// 传给 AAudio 的 userData。
///
/// `scratch` 只由数据回调所在线程访问，用 `UnsafeCell` 避免每次回调重新分配。
struct Ctx {
    shared: Arc<Shared>,
    scratch: UnsafeCell<Vec<f32>>,
}

// SAFETY: scratch 只在数据回调线程触碰（AAudio 保证同一时刻只有一个回调在跑），
// 其余字段都是原子或带 Mutex 的
unsafe impl Send for Ctx {}
unsafe impl Sync for Ctx {}

/// 一条 AAudio 流的全部资源
struct Inner {
    stream: AtomicPtr<ffi::AAudioStreamStruct>,
    ctx: Arc<Ctx>,
    device: OutputDeviceInfo,
}

// SAFETY: AAudioStream 指针只在命令线程（requestXxx/close）与回调中使用，
// AAudio 自身保证线程安全；close 会等待回调结束后才返回
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

pub struct AaudioExclusivePlayer {
    /// 用 Arc 包一层：看门狗线程需要看到"当前"的流，而不是启动那一刻的快照
    inner: Arc<Mutex<Option<Arc<Inner>>>>,
    watchdog: Mutex<Option<JoinHandle<()>>>,
    watchdog_stop: Arc<AtomicBool>,
    /// 对外暴露的采样率/声道（initialize 之后恒定，ensure_format 会更新）
    sample_rate: AtomicU32,
    channels: AtomicU32,
    volume: AtomicU32,
    state: AtomicU8,
    /// 独占是否真的生效（开流后由 AAudio 回报的 sharing mode 确认）
    exclusive_confirmed: AtomicBool,
}

impl Default for AaudioExclusivePlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AaudioExclusivePlayer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            watchdog: Mutex::new(None),
            watchdog_stop: Arc::new(AtomicBool::new(false)),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            state: AtomicU8::new(ST_UNINITIALIZED),
            exclusive_confirmed: AtomicBool::new(false),
        }
    }

    /// 当前流的共享状态（克隆 Arc，避免长时间持锁）
    fn shared(&self) -> Option<Arc<Shared>> {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.as_ref().map(|i| Arc::clone(&i.ctx.shared))
    }

    /// 打开独占流。
    ///
    /// `device` 传设备 id 的字符串形式（与 WASAPI 版本传设备名共用同一个签名）；
    /// 传 `None` 或解析失败时自动取当前 USB 音频设备。
    pub fn initialize(&self, device: Option<&str>) -> Result<(u32, u16, String), AppError> {
        let wanted_id = device.and_then(|s| s.trim().parse::<i32>().ok());
        let device_info = match wanted_id {
            Some(id) => super::device::query_output_devices()?
                .into_iter()
                .find(|d| d.id == id)
                .ok_or_else(|| AppError::msg(format!("未找到 id 为 {id} 的输出设备")))?,
            None => find_usb_output_device()?
                .ok_or_else(|| AppError::msg("未检测到 USB 音频设备，无法启用 USB DAC 独占输出"))?,
        };

        let (rate, channels) = self.open_stream(&device_info, None, None)?;
        Ok((rate, channels, device_info.name.clone()))
    }

    /// 当前流绑定的输出设备快照（无流时为 `None`）
    #[must_use]
    pub fn current_device(&self) -> Option<OutputDeviceInfo> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|i| i.device.clone())
    }

    /// 作废当前流（USB DAC 热插拔后由 [`super::on_audio_route_changed`] 调用）。
    ///
    /// 必须连同**缓存的设备快照**一起丢掉，原因见 [`Self::ensure_format`]：
    /// AAudio 的流在创建时就死绑 `AAudioStreamBuilder_setDeviceId` 拿到的设备 id，
    /// 设备拔掉后这条流不会自愈，而系统在设备重新接入时分配的是**新的 id**。
    pub fn release_stream(&self) {
        self.close_stream();
    }

    /// 按曲目的原生采样率/声道对齐输出；必要时关掉旧流重开。
    ///
    /// 返回实际生效的 (采样率, 声道)。采样率与源一致时即为"直出"（无重采样）。
    ///
    /// ⚠️ 没有流时这里会**重新解析当前的 USB 输出设备**，而不是报错。
    /// 这不只是"首次调用"的路径：USB DAC 拔插一次之后旧流就废了
    /// （设备 id 变了 / 设备已不在），若继续拿 `Inner` 里缓存的旧 id 去开流，
    /// 独占与共享回退两条路都会失败 —— 用户看到的现象就是
    /// "拔插一次 USB DAC 后再也放不出声，必须重启应用"，所以自愈是必要的。
    pub fn ensure_format(&self, sample_rate: u32, channels: u16) -> Result<(u32, u16), AppError> {
        let current = (
            self.sample_rate.load(Ordering::SeqCst),
            self.channels.load(Ordering::SeqCst) as u16,
        );
        if current == (sample_rate, channels)
            && self.state.load(Ordering::SeqCst) != ST_UNINITIALIZED
        {
            return Ok(current);
        }

        let device = match self.current_device() {
            Some(device) => device,
            None => find_usb_output_device()?
                .ok_or_else(|| AppError::msg("未检测到 USB 音频设备，无法启用 USB DAC 独占输出"))?,
        };

        // 设备支持曲目原生速率就直接用（位完美），否则退到最接近的一档
        let target_rate = pick_sample_rate(&device, sample_rate).unwrap_or(sample_rate);
        let target_channels = pick_channel_count(&device, channels).unwrap_or(channels);
        self.open_stream(&device, Some(target_rate), Some(target_channels))
    }

    /// 开流（若已有旧流先关掉）
    fn open_stream(
        &self,
        device: &OutputDeviceInfo,
        rate: Option<u32>,
        channels: Option<u16>,
    ) -> Result<(u32, u16), AppError> {
        self.close_stream();

        let wanted_rate = rate
            .or_else(|| device.sample_rates.iter().copied().max())
            .unwrap_or(48000);
        let wanted_channels = channels
            .or_else(|| device.channel_counts.iter().copied().max())
            .unwrap_or(2);
        let wanted_format = pick_format(device);

        // 先试独占，失败再退共享：共享模式虽然会被重采样，但至少能出声，
        // 由前端把"当前并非位完美"如实告诉用户。
        let mut builder: ffi::AAudioStreamBuilder = ptr::null_mut();
        // SAFETY: builder 为输出参数指针，调用成功后由 AAudio 填充
        check(
            unsafe { ffi::AAudio_createStreamBuilder(&mut builder) },
            "createStreamBuilder",
        )?;

        let shared = Arc::new(Shared {
            ring: SampleRing::new(ring_capacity(wanted_rate, wanted_channels)),
            written: AtomicU64::new(0),
            volume: AtomicU32::new(self.volume.load(Ordering::SeqCst)),
            channels: AtomicU32::new(wanted_channels as u32),
            format: AtomicI32::new(wanted_format),
            fade: Mutex::new(FadeState::Idle),
            pending: AtomicU8::new(PENDING_NONE),
            underruns: AtomicU64::new(0),
            disconnected: AtomicBool::new(false),
            running: AtomicBool::new(false),
        });
        let ctx = Arc::new(Ctx {
            shared: Arc::clone(&shared),
            scratch: UnsafeCell::new(Vec::with_capacity(8192)),
        });

        // SAFETY: builder 由 AAudio_createStreamBuilder 成功创建；以下 setter
        // 全部是同步调用，不涉及回调线程
        unsafe {
            ffi::AAudioStreamBuilder_setDirection(builder, ffi::AAUDIO_DIRECTION_OUTPUT);
            ffi::AAudioStreamBuilder_setDeviceId(builder, device.id);
            ffi::AAudioStreamBuilder_setSharingMode(builder, ffi::AAUDIO_SHARING_MODE_EXCLUSIVE);
            ffi::AAudioStreamBuilder_setPerformanceMode(
                builder,
                ffi::AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
            );
            ffi::AAudioStreamBuilder_setSampleRate(builder, wanted_rate as i32);
            ffi::AAudioStreamBuilder_setChannelCount(builder, wanted_channels as i32);
            ffi::AAudioStreamBuilder_setFormat(builder, wanted_format);
            ffi::AAudioStreamBuilder_setDataCallback(
                builder,
                Some(data_callback),
                Arc::as_ptr(&ctx) as *mut c_void,
            );
            ffi::AAudioStreamBuilder_setErrorCallback(
                builder,
                Some(error_callback),
                Arc::as_ptr(&ctx) as *mut c_void,
            );
        }

        let mut stream: ffi::AAudioStream = ptr::null_mut();
        // SAFETY: builder 有效，stream 为输出参数
        let mut result = unsafe { ffi::AAudioStreamBuilder_openStream(builder, &mut stream) };
        if result != ffi::AAUDIO_OK {
            // 独占开不出（速率/格式不被接受、被其它应用占用）→ 退共享模式重试一次
            log::warn!(
                "AAudio 独占开流失败({})，回退共享模式: {}",
                // SAFETY: 结果码转文本，见 ffi::result_to_text
                unsafe { ffi::result_to_text(result) },
                device.name
            );
            // SAFETY: 同上，改共享模式后重新 open
            unsafe {
                ffi::AAudioStreamBuilder_setSharingMode(builder, ffi::AAUDIO_SHARING_MODE_SHARED);
                ffi::AAudioStreamBuilder_setSampleRate(builder, wanted_rate as i32);
            }
            result = unsafe { ffi::AAudioStreamBuilder_openStream(builder, &mut stream) };
        }
        // SAFETY: builder 不再使用（stream 已持有资源）
        unsafe { ffi::AAudioStreamBuilder_delete(builder) };
        check(result, "openStream")?;

        // SAFETY: stream 由 openStream 成功创建
        let (actual_rate, actual_channels, actual_format, sharing) = unsafe {
            (
                ffi::AAudioStream_getSampleRate(stream) as u32,
                ffi::AAudioStream_getChannelCount(stream) as u16,
                ffi::AAudioStream_getFormat(stream),
                ffi::AAudioStream_getSharingMode(stream),
            )
        };
        shared.running.store(true, Ordering::Release);
        shared
            .channels
            .store(actual_channels as u32, Ordering::Relaxed);
        shared.format.store(actual_format, Ordering::Relaxed);

        self.exclusive_confirmed.store(
            sharing == ffi::AAUDIO_SHARING_MODE_EXCLUSIVE,
            Ordering::SeqCst,
        );
        self.sample_rate.store(actual_rate, Ordering::SeqCst);
        self.channels
            .store(actual_channels as u32, Ordering::SeqCst);
        self.state.store(ST_STOPPED, Ordering::SeqCst);

        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(Inner {
            stream: AtomicPtr::new(stream),
            ctx,
            device: device.clone(),
        }));

        log::info!(
            "AAudio stream opened: {} @ {actual_rate}Hz, {actual_channels}ch, format={actual_format}, sharing={sharing}",
            device.name
        );
        // 看门狗在这里启动（而不是 initialize）：它负责把 pending 的 pause/stop
        // 请求转成 AAudio 调用，凡是"流存在"就必须有它，而 ensure_format 也会开流
        // （热插拔后按新设备重建时）。重复调用无副作用。
        self.start_watchdog();
        Ok((actual_rate, actual_channels))
    }

    /// 关闭并释放当前流（close 会等回调结束，因此之后才能释放 ctx）
    fn close_stream(&self) {
        let taken = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let Some(inner) = taken else { return };

        inner.ctx.shared.running.store(false, Ordering::Release);
        let stream = inner.stream.load(Ordering::Acquire);
        if !stream.is_null() {
            // SAFETY: stream 由 openStream 创建且尚未 close；close 不能从回调里调用，
            // 这里一定在命令线程上
            unsafe {
                let _ = ffi::AAudioStream_requestStop(stream);
                let _ = ffi::AAudioStream_close(stream);
            }
        }
        drop(inner); // ctx 也随之释放（close 已保证回调不再运行）
        self.state.store(ST_UNINITIALIZED, Ordering::SeqCst);
        self.exclusive_confirmed.store(false, Ordering::SeqCst);
    }

    /// 启动看门狗：执行淡出后的 pause/stop、处理设备断开
    fn start_watchdog(&self) {
        let mut guard = self
            .watchdog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_some() {
            return;
        }
        self.watchdog_stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.watchdog_stop);
        let inner_getter = WatchdogRef {
            inner: Arc::clone(&self.inner),
        };
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if let Some(inner) = inner_getter.current() {
                    let shared = &inner.ctx.shared;
                    let stream = inner.stream.load(Ordering::Acquire);
                    match shared.pending.swap(PENDING_NONE, Ordering::SeqCst) {
                        PENDING_PAUSE if !stream.is_null() => {
                            // SAFETY: stream 有效，requestPause 允许在任何线程调用
                            let r = unsafe { ffi::AAudioStream_requestPause(stream) };
                            log::info!("AAudio requestPause -> {}", unsafe {
                                ffi::result_to_text(r)
                            });
                        }
                        PENDING_STOP if !stream.is_null() => {
                            // SAFETY: 同上
                            unsafe {
                                let _ = ffi::AAudioStream_requestStop(stream);
                            }
                            shared.ring.clear();
                            shared.written.store(0, Ordering::Relaxed);
                        }
                        _ => {}
                    }
                    if shared.disconnected.load(Ordering::SeqCst) {
                        log::warn!("AAudio 流已断开（USB DAC 可能已拔出）");
                        shared.disconnected.store(false, Ordering::SeqCst);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(WATCHDOG_INTERVAL_MS));
            }
        });
        *guard = Some(handle);
    }

    // ---------------- 播放控制 ----------------

    pub fn start(&self) -> Result<(), AppError> {
        self.request_start()
    }

    pub fn stop(&self) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        shared.pending.store(PENDING_STOP, Ordering::SeqCst);
        shared.ring.clear();
        self.state.store(ST_STOPPED, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.begin_fade_out(duration_ms, FadeAction::Stop)
    }

    pub fn pause(&self) -> Result<(), AppError> {
        self.pause_with_fade_out(DEFAULT_FADE_MS)
    }

    pub fn pause_no_fade(&self) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        *shared
            .fade
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = FadeState::Idle;
        shared.pending.store(PENDING_PAUSE, Ordering::SeqCst);
        self.state.store(ST_PAUSED, Ordering::SeqCst);
        Ok(())
    }

    pub fn pause_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.begin_fade_out(duration_ms, FadeAction::Pause)
    }

    pub fn resume(&self) -> Result<(), AppError> {
        self.resume_with_fade_in(DEFAULT_FADE_MS)
    }

    pub fn resume_no_fade(&self) -> Result<(), AppError> {
        self.request_start()
    }

    pub fn resume_with_fade_in(&self, duration_ms: u32) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Err(AppError::msg("AAudio 播放器未初始化"));
        };
        let frames = fade_frames(duration_ms, self.sample_rate.load(Ordering::SeqCst));
        if frames > 0 {
            *shared
                .fade
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = FadeState::In {
                remaining: frames,
                total: frames,
            };
        }
        self.request_start()
    }

    pub fn set_volume(&self, vol: f32) -> Result<(), AppError> {
        let vol = vol.clamp(0.0, 1.0);
        self.volume.store(vol.to_bits(), Ordering::SeqCst);
        if let Some(shared) = self.shared() {
            shared.volume.store(vol.to_bits(), Ordering::SeqCst);
        }
        Ok(())
    }

    pub fn push_samples(&self, samples: &[f32]) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Err(AppError::msg("AAudio 播放器未初始化"));
        };
        let mut offset = 0;
        // 背压：缓冲满时等一会儿再推。解码线程不是实时线程，等在这里不会造成爆音，
        // 而直接丢弃样本会。
        while offset < samples.len() {
            let written = shared.ring.push_slice(&samples[offset..]);
            if written == 0 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            offset += written;
        }
        Ok(())
    }

    pub fn clear_buffer(&self) -> Result<(), AppError> {
        if let Some(shared) = self.shared() {
            shared.ring.clear();
            shared.written.store(0, Ordering::SeqCst);
        }
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> PlaybackState {
        state_from_code(self.state.load(Ordering::SeqCst))
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels.load(Ordering::SeqCst) as u16
    }

    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::SeqCst))
    }

    #[must_use]
    pub fn samples_written(&self) -> u64 {
        self.shared()
            .map_or(0, |s| s.written.load(Ordering::SeqCst))
    }

    pub fn reset_samples_written(&self) {
        if let Some(shared) = self.shared() {
            shared.written.store(0, Ordering::SeqCst);
        }
    }

    /// 缓冲里尚未被硬件取走的采样数
    #[must_use]
    pub fn buffer_size(&self) -> usize {
        self.shared().map_or(0, |s| s.ring.len())
    }

    /// 独占是否真的生效（供设置页如实展示）
    #[must_use]
    pub fn is_exclusive_confirmed(&self) -> bool {
        self.exclusive_confirmed.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn underruns(&self) -> u64 {
        self.shared()
            .map_or(0, |s| s.underruns.load(Ordering::SeqCst))
    }

    // ---------------- 内部 ----------------

    fn request_start(&self) -> Result<(), AppError> {
        let handle = {
            let guard = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.as_ref().map(Arc::clone)
        };
        let Some(inner) = handle else {
            return Err(AppError::msg("AAudio 播放器未初始化"));
        };
        let stream = inner.stream.load(Ordering::Acquire);
        if stream.is_null() {
            return Err(AppError::msg("AAudio 流未打开"));
        }
        inner.ctx.shared.running.store(true, Ordering::Release);
        // SAFETY: stream 有效
        let r = unsafe { ffi::AAudioStream_requestStart(stream) };
        if r != ffi::AAUDIO_OK {
            // SAFETY: 结果码转文本
            return Err(AppError::msg(format!(
                "AAudio requestStart 失败: {}",
                unsafe { ffi::result_to_text(r) }
            )));
        }
        self.state.store(ST_PLAYING, Ordering::SeqCst);
        Ok(())
    }

    fn begin_fade_out(&self, duration_ms: u32, action: FadeAction) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        let frames = fade_frames(duration_ms, self.sample_rate.load(Ordering::SeqCst));
        *shared
            .fade
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = if frames == 0 {
            shared.pending.store(
                if action == FadeAction::Pause {
                    PENDING_PAUSE
                } else {
                    PENDING_STOP
                },
                Ordering::SeqCst,
            );
            FadeState::Idle
        } else {
            FadeState::Out {
                remaining: frames,
                total: frames,
                action,
            }
        };
        self.state.store(
            if action == FadeAction::Pause {
                ST_PAUSING
            } else {
                ST_STOPPING
            },
            Ordering::SeqCst,
        );
        Ok(())
    }
}

impl Drop for AaudioExclusivePlayer {
    fn drop(&mut self) {
        self.watchdog_stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self
            .watchdog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = handle.join();
        }
        self.close_stream();
    }
}

// ============================================================================
// 回调与工具
// ============================================================================

fn ring_capacity(sample_rate: u32, channels: u16) -> usize {
    ((sample_rate as usize * channels as usize) as f32 * RING_SECONDS) as usize
}

fn fade_frames(duration_ms: u32, sample_rate: u32) -> usize {
    if duration_ms == 0 || sample_rate == 0 {
        return 0;
    }
    ((sample_rate as u64 * duration_ms as u64) / 1000) as usize
}

/// 按设备上报的编码挑一个格式：优先 32 位浮点（解码器输出本身就是 f32，
/// 免一次量化），其次是 32 位 / 24 位整数，最后 16 位。
fn pick_format(device: &OutputDeviceInfo) -> ffi::aaudio_format_t {
    const ENCODING_PCM_16BIT: i32 = 2;
    const ENCODING_PCM_FLOAT: i32 = 4;
    const ENCODING_PCM_24BIT: i32 = 6;
    const ENCODING_PCM_32BIT: i32 = 7;

    if device.encodings.is_empty() {
        // 设备没上报：float 在 AAudio 上总是可用（必要时由框架转换）
        return ffi::AAUDIO_FORMAT_PCM_FLOAT;
    }
    if device.encodings.contains(&ENCODING_PCM_FLOAT) {
        ffi::AAUDIO_FORMAT_PCM_FLOAT
    } else if device.encodings.contains(&ENCODING_PCM_32BIT) {
        ffi::AAUDIO_FORMAT_PCM_I32
    } else if device.encodings.contains(&ENCODING_PCM_24BIT) {
        ffi::AAUDIO_FORMAT_PCM_I24_PACKED
    } else if device.encodings.contains(&ENCODING_PCM_16BIT) {
        ffi::AAUDIO_FORMAT_PCM_I16
    } else {
        ffi::AAUDIO_FORMAT_PCM_FLOAT
    }
}

fn check(result: ffi::aaudio_result_t, what: &str) -> Result<(), AppError> {
    if result == ffi::AAUDIO_OK {
        return Ok(());
    }
    // SAFETY: 结果码转文本
    let text = unsafe { ffi::result_to_text(result) };
    Err(AppError::msg(format!(
        "AAudio {what} 失败({result}): {text}"
    )))
}

unsafe extern "C" fn data_callback(
    _stream: *mut ffi::AAudioStreamStruct,
    user: *mut c_void,
    audio: *mut c_void,
    frames: i32,
) -> ffi::aaudio_data_callback_result_t {
    // SAFETY: userData 是 open_stream 时传入的 Arc<Ctx> 裸指针，
    // 释放发生在 AAudioStream_close 之后（close 会等本次回调返回）
    let ctx = unsafe { &*(user as *const Ctx) };
    let shared = &ctx.shared;
    if frames <= 0 || audio.is_null() {
        return ffi::AAUDIO_CALLBACK_RESULT_CONTINUE;
    }
    if !shared.running.load(Ordering::Acquire) {
        write_silence(shared, audio, frames as usize);
        return ffi::AAUDIO_CALLBACK_RESULT_CONTINUE;
    }

    let channels = shared.channels.load(Ordering::Relaxed) as usize;
    let need = frames as usize * channels;

    // SAFETY: scratch 只在本回调线程访问
    let scratch = unsafe { &mut *ctx.scratch.get() };
    scratch.clear();
    let got = shared.ring.drain_into(scratch, need);
    if got < need {
        scratch.resize(need, 0.0);
        shared.underruns.fetch_add(1, Ordering::Relaxed);
    }

    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let (fade_start, fade_end, finished) = {
        let mut fade = shared
            .fade
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match *fade {
            FadeState::Idle => (1.0f32, 1.0f32, None),
            FadeState::Out {
                remaining,
                total,
                action,
            } => {
                let left = remaining.saturating_sub(frames as usize);
                let start = remaining as f32 / total as f32;
                let end = left as f32 / total as f32;
                *fade = if left == 0 {
                    FadeState::Idle
                } else {
                    FadeState::Out {
                        remaining: left,
                        total,
                        action,
                    }
                };
                (start, end, if left == 0 { Some(action) } else { None })
            }
            FadeState::In { remaining, total } => {
                let left = remaining.saturating_sub(frames as usize);
                let start = 1.0 - remaining as f32 / total as f32;
                let end = 1.0 - left as f32 / total as f32;
                *fade = if left == 0 {
                    FadeState::Idle
                } else {
                    FadeState::In {
                        remaining: left,
                        total,
                    }
                };
                (start, end, None)
            }
        }
    };

    let step = if frames > 1 {
        (fade_end - fade_start) / (frames - 1) as f32
    } else {
        0.0
    };

    // SAFETY: audio 由 AAudio 提供，容量 = frames × channels × 采样字节数
    unsafe {
        match shared.format.load(Ordering::Relaxed) {
            ffi::AAUDIO_FORMAT_PCM_FLOAT => {
                let out = std::slice::from_raw_parts_mut(audio as *mut f32, need);
                for (i, dst) in out.iter_mut().enumerate() {
                    *dst = scratch[i] * volume * (fade_start + step * i as f32);
                }
            }
            ffi::AAUDIO_FORMAT_PCM_I32 => {
                let out = std::slice::from_raw_parts_mut(audio as *mut i32, need);
                for (i, dst) in out.iter_mut().enumerate() {
                    let v = scratch[i] * volume * (fade_start + step * i as f32);
                    *dst = (v.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
                }
            }
            ffi::AAUDIO_FORMAT_PCM_I24_PACKED => {
                // 3 字节小端、无对齐：逐样本展开写入
                let v_scale = 8_388_607.0f32; // 2^23 - 1
                let out = std::slice::from_raw_parts_mut(audio as *mut u8, need * 3);
                for i in 0..need {
                    let v = scratch[i] * volume * (fade_start + step * i as f32);
                    let q = (v.clamp(-1.0, 1.0) * v_scale) as i32;
                    let b = q.to_le_bytes();
                    out[i * 3] = b[0];
                    out[i * 3 + 1] = b[1];
                    out[i * 3 + 2] = b[2];
                }
            }
            _ => {
                // I16（未知格式也按 I16 处理，AAudio 不接受时已在 open 阶段回落共享模式）
                let out = std::slice::from_raw_parts_mut(audio as *mut i16, need);
                for (i, dst) in out.iter_mut().enumerate() {
                    let v = scratch[i] * volume * (fade_start + step * i as f32);
                    *dst = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                }
            }
        }
    }

    shared.written.fetch_add(need as u64, Ordering::Relaxed);

    if let Some(action) = finished {
        shared.pending.store(
            if action == FadeAction::Pause {
                PENDING_PAUSE
            } else {
                PENDING_STOP
            },
            Ordering::SeqCst,
        );
    }

    ffi::AAUDIO_CALLBACK_RESULT_CONTINUE
}

unsafe extern "C" fn error_callback(
    _stream: *mut ffi::AAudioStreamStruct,
    user: *mut c_void,
    error: i32,
) {
    // SAFETY: 同 data_callback
    let ctx = unsafe { &*(user as *const Ctx) };
    ctx.shared.disconnected.store(true, Ordering::SeqCst);
    log::error!("AAudio 错误回调: {}", unsafe {
        ffi::result_to_text(error)
    });
}

/// 流未运行时把输出区填零（避免回放上一轮的残留数据）
fn write_silence(shared: &Shared, audio: *mut c_void, frames: usize) {
    let channels = shared.channels.load(Ordering::Relaxed) as usize;
    let n = frames * channels;
    // SAFETY: audio 由 AAudio 提供，容量与格式匹配
    unsafe {
        match shared.format.load(Ordering::Relaxed) {
            ffi::AAUDIO_FORMAT_PCM_FLOAT => {
                std::slice::from_raw_parts_mut(audio as *mut f32, n).fill(0.0);
            }
            ffi::AAUDIO_FORMAT_PCM_I32 => {
                std::slice::from_raw_parts_mut(audio as *mut i32, n).fill(0);
            }
            ffi::AAUDIO_FORMAT_PCM_I24_PACKED => {
                std::slice::from_raw_parts_mut(audio as *mut u8, n * 3).fill(0);
            }
            _ => {
                std::slice::from_raw_parts_mut(audio as *mut i16, n).fill(0);
            }
        }
    }
}

/// 看门狗线程持有的一份"当前流"引用
struct WatchdogRef {
    inner: Arc<Mutex<Option<Arc<Inner>>>>,
}

impl WatchdogRef {
    fn current(&self) -> Option<Arc<Inner>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
