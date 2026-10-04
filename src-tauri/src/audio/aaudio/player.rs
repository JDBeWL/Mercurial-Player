//! AAudio 独占（位完美）输出播放器：绕过 AudioFlinger 的混音与重采样，按原生采样率直写 USB DAC。
//!
//! 数据通路是解码线程 `push_samples` 写无锁 SPSC 环形缓冲，AAudio 数据回调取走并施加音量与
//! 淡入淡出；回调内全走原子量，不阻塞、不分配。

// 本模块就是 AAudio 的 FFI 边界（裸指针解引用、C 回调 ABI、跨线程 UnsafeCell），逐项标注只会
// 重复同一句话，故在模块级统一 allow；每个 unsafe 块内仍保留独立的 SAFETY 说明。
#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::os::raw::c_void;
use std::ptr;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::device::{
    OutputDeviceInfo, find_usb_output_device, pick_channel_count, pick_sample_rate,
};
use super::ffi;
use crate::audio::{PlaybackState, PushOutcome, SampleRing, spectrum::now_ms};
use crate::error::AppError;

/// 环形缓冲的目标时长（秒）：独占 buffer 很小，缓冲给足才能吸收解码抖动而不断流。
/// 硬约束是必须大于水位门控，见 [`crate::audio::sample_ring::exclusive_ring_capacity`]。
const RING_SECONDS: f32 = 1.5;
/// 默认淡入淡出时长（毫秒）
const DEFAULT_FADE_MS: u32 = 30;
/// 看门狗轮询间隔（毫秒）
const WATCHDOG_INTERVAL_MS: u64 = 20;
/// 解码线程 park gate 的兜底超时（毫秒）：正常由控制侧与消费进度唤醒，超时只兜漏通知。
const PARK_TICK_MS: u64 = 250;
/// 看门狗替回调收尾的宽限（毫秒）：超过 duration + 本宽限斜坡仍未推进，说明回调已停摆。
const FADE_FALLBACK_GRACE_MS: u64 = 100;
/// 数据回调 scratch 的预留下限（交错采样数）。实际预留量开流后按生效容量重算，
/// 这里只兜住容量查询失败或设备给出很小容量的情况。
const AAUDIO_SCRATCH_CAPACITY: usize = 32_768;
/// 单次数据回调的目标时长（毫秒）：不指定时 AAudio 每 burst 回调一次，放大回调块能按同比例
/// 减少唤醒，而输出延迟对本应用没有意义（环形缓冲已有 `RING_SECONDS` 余量）。
///
/// 上限受 `DEFAULT_FADE_MS` 约束：块长一旦超过淡变时长，整条淡出会在单块内走完，退化成阶跃。
const CALLBACK_MILLIS: u32 = 25;
/// 欠载补零前的收敛斜坡长度（帧），约 1.5 ms @44.1k：够消掉阶跃造成的 click，
/// 又短到听感上只是"这句尾巴轻了一点"。
const UNDERRUN_RAMP_FRAMES: usize = 64;
/// 欠载上报的最小间隔（秒）：回调是实时线程不能打日志，观测由看门狗按此间隔汇报增量。
const UNDERRUN_LOG_INTERVAL_SECS: u64 = 5;

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

/// 状态码的日志文本
const fn state_text(code: u8) -> &'static str {
    match code {
        ST_UNINITIALIZED => "Uninitialized",
        ST_STOPPED => "Stopped",
        ST_PLAYING => "Playing",
        ST_PAUSED => "Paused",
        ST_STOPPING => "Stopping",
        ST_PAUSING => "Pausing",
        _ => "Unknown",
    }
}

/// 状态迁移的唯一入口：媒体按键与 `set_usb_dac_exclusive` 都按这个原子量判断"当前是否在播"，
/// 看门狗落地 pause/stop 之后也必须走这里，否则状态会永远停在 Pausing/Stopping。
fn transition_state(state: &AtomicU8, new: u8) {
    let old = state.swap(new, Ordering::SeqCst);
    if old != new {
        log::debug!(
            "AAudio 播放器状态: {} -> {}",
            state_text(old),
            state_text(new)
        );
    }
}

/// 淡出完成后要执行的动作（由看门狗在回调线程之外执行）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeAction {
    Pause,
    Stop,
}

/// 待办槽的"空"值（打包后的 0）。动作码从 1 开始，因此 0 不会与任何动作组合撞车
const PENDING_NONE: u64 = 0;
const PENDING_PAUSE: u8 = 1;
const PENDING_STOP: u8 = 2;

/// `pending` / `fade_action` 的打包格式：高 56 位为命令代际，低 8 位为动作码。
///
/// 代际让"恢复播放"能把更早排队、尚未执行的暂停/停止判为过期（否则快速暂停再恢复，看门狗会
/// 在恢复之后又执行一次暂停）；打包进同一个原子量则保证读侧永远拿到一致的 (代际, 动作) 组合。
const fn pack_pending(seq: u64, action: u8) -> u64 {
    (seq << 8) | action as u64
}

const fn pending_action(packed: u64) -> u8 {
    (packed & 0xFF) as u8
}

const fn pending_seq(packed: u64) -> u64 {
    packed >> 8
}

// 斜坡状态：回调线程每帧递减 remaining，命令线程只在淡变开始时写一次，
// 因此用原子而不是 Mutex —— 实时回调里阻塞会造成 xrun。
const FADE_IDLE: u8 = 0;
const FADE_OUT: u8 = 1;
const FADE_IN: u8 = 2;

/// 音频回调与命令线程共享的状态（全部是无锁字段）
struct Shared {
    ring: SampleRing,
    /// 已交付给 AAudio 的采样数（交织后，即 帧数 * 声道数）
    written: AtomicU64,
    /// 音量，f32 位模式
    volume: AtomicU32,
    channels: AtomicU32,
    format: AtomicI32,
    fade_dir: AtomicU8,
    fade_left: AtomicUsize,
    /// 斜坡的挂钟兜底截止时刻（毫秒）。回调停摆（流已暂停/断开）时斜坡会卡在半途，
    /// 看门狗过了这个时刻就替回调收尾。
    fade_deadline_ms: AtomicU64,
    fade_total: AtomicUsize,
    /// 淡出结束时由回调转交给 `pending` 的动作，已按 [`pack_pending`] 带上发起时的代际
    /// （命令线程写入，可置回 PENDING_NONE 取消）
    fade_action: AtomicU64,
    /// 淡出完成后待看门狗执行的动作，同样打包了代际
    pending: AtomicU64,
    /// 命令代际：每次"开始/恢复播放"递增。带更早代际的待办动作执行前会被判过期作废
    /// （见 [`Shared::bump_cmd_seq`] 与 [`pack_pending`]）
    cmd_seq: AtomicU64,
    underruns: AtomicU64,
    disconnected: AtomicBool,
    /// 流是否处于可写状态（关闭/重建期间置 false）
    running: AtomicBool,
    /// 与播放器共享的对外状态量：看门狗落地 pause/stop 后要把 Pausing/Stopping 推进到
    /// Paused/Stopped，否则命令层永远看不到淡出真正完成
    state: Arc<AtomicU8>,
    /// park gate：解码线程环满时停在这里，等控制侧或消费进度变化唤醒。用条件变量而非轮询，
    /// 暂停时零唤醒（只靠 [`PARK_TICK_MS`] 兜底），恢复立即唤醒。
    /// 实时回调绝不触碰这两个字段（`Mutex` 在实时线程上可能陷入内核造成 xrun），
    /// 因此不参与 `audio/mod.rs` 的锁序，也不会与命令锁嵌套。
    park_mutex: Mutex<()>,
    park_cvar: Condvar,
}

impl Shared {
    /// 开启新一代命令，返回新代际。恢复/起播路径调用后，所有更早排队的待办动作即告过期
    fn bump_cmd_seq(&self) -> u64 {
        self.cmd_seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// 唤醒停在 park gate 上的解码线程。控制侧改变了它等待的条件（暂停/恢复/停止/切歌）、
    /// 或消费侧腾出了空间时调用；只允许非实时线程调用。
    fn notify_park(&self) {
        self.park_cvar.notify_all();
    }

    /// 停在 park gate 上等一会儿（返回后调用方必须重新检查取消判据与水位）。
    fn park(&self, timeout: Duration) {
        let guard = lock_or_log!(self.park_mutex.lock());
        drop(self.park_cvar.wait_timeout(guard, timeout));
    }

    /// 以"当前代际"把动作写入待办槽。暂停/停止用它排队，恢复播放时递增代际即可作废
    fn queue_pending(&self, action: u8) {
        let seq = self.cmd_seq.load(Ordering::SeqCst);
        self.pending
            .store(pack_pending(seq, action), Ordering::SeqCst);
        self.notify_park();
    }

    /// 把淡出完成转交的动作写进待办槽，仅当槽位为空（槽里已有的是更新的命令，不能被旧动作盖掉）。
    ///
    /// 会从数据回调（实时线程）调用，因此只做一次 CAS、不打日志。CAS 失败不是错误：
    /// 抢占槽位的新命令本身就是对这次淡出动作的否定。
    fn handoff_fade_action(&self, packed: u64) {
        if packed == PENDING_NONE {
            return;
        }
        let _ =
            self.pending
                .compare_exchange(PENDING_NONE, packed, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// 无条件取消斜坡（任何方向）并清掉排队中的收尾动作。停止 / 无淡变暂停时调用。
    fn cancel_fade(&self) {
        self.fade_dir.store(FADE_IDLE, Ordering::SeqCst);
        self.fade_left.store(0, Ordering::SeqCst);
        self.fade_action.store(PENDING_NONE, Ordering::SeqCst);
    }

    /// 只取消进行中的淡出（保留淡入）。恢复 / 重新起播必须调用，否则淡出中的回调会把增益推到 0
    /// 再跳回 1.0，即音量抽动。刻意不动 `FADE_IN`：`resume_with_fade_in` 先置好淡入参数再调
    /// `request_start`，无条件清掉会把刚设好的淡入一起抹掉。
    fn cancel_fade_out(&self) {
        if self.fade_dir.load(Ordering::SeqCst) != FADE_OUT {
            return;
        }
        self.fade_dir.store(FADE_IDLE, Ordering::SeqCst);
        self.fade_left.store(0, Ordering::SeqCst);
        self.fade_action.store(PENDING_NONE, Ordering::SeqCst);
    }
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
    /// 原生流句柄。取用与关闭都必须持这把控制锁：AAudio 的 close 不是线程安全操作，命令线程
    /// 关流（拔 DAC / 换采样率）时，看门狗可能正拿着同一句柄请求 pause/stop。
    /// 指针的读取也必须在锁内——在锁外先读出的旧指针不受保护。
    stream: Mutex<*mut ffi::AAudioStreamStruct>,
    ctx: Arc<Ctx>,
    device: OutputDeviceInfo,
}

impl Inner {
    /// 在控制锁内使用流句柄。返回 `None` 表示流已关闭（或正在关闭）。
    ///
    /// 读取也放进锁里：`close_stream` 取走句柄后置 null，之后进来的调用只会看到 null，
    /// 不可能再碰到已释放的流。
    fn with_stream<R>(&self, f: impl FnOnce(*mut ffi::AAudioStreamStruct) -> R) -> Option<R> {
        self.with_control(|stream| stream.map(f))
    }

    /// 控制锁内的通用临界区：闭包拿到的句柄可能为 `None`（流已关闭）。
    ///
    /// 这把锁同时是"待办动作是否过期"的判定点：看门狗的过期复查与 `request_start` 的代际自增
    /// 都在锁内，不会出现"复查通过后、真正 pause 之前被恢复命令插队"的窗口。
    fn with_control<R>(&self, f: impl FnOnce(Option<*mut ffi::AAudioStreamStruct>) -> R) -> R {
        let stream = lock_or_log!(self.stream.lock());
        f(if stream.is_null() {
            None
        } else {
            Some(*stream)
        })
    }
}

// SAFETY: 句柄由 Mutex 包裹且只在锁内解引用（见 with_stream 与 close_stream），
// 回调只经 userData 触碰 Ctx、不碰句柄；其余字段为原子量/Arc/不可变数据。
// AAudio 保证 close 会等待回调结束后才返回，故 Ctx 在 Inner 存活期间始终有效。
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
    state: Arc<AtomicU8>,
    /// 独占是否真的生效（开流后由 AAudio 回报的 sharing mode 确认）
    exclusive_confirmed: AtomicBool,
}

impl Default for AaudioExclusivePlayer {
    fn default() -> Self {
        Self::new()
    }
}

/// 取当前流的共享状态（克隆 Arc，避免长时间持锁）
fn resolve_shared(inner: &Mutex<Option<Arc<Inner>>>) -> Option<Arc<Shared>> {
    let guard = lock_or_log!(inner.lock());
    guard.as_ref().map(|i| Arc::clone(&i.ctx.shared))
}

/// 解码线程用的无锁写入端，由 `AaudioExclusivePlayer::producer` 取得。
///
/// 只在每次调用开头短暂锁 `inner` 取当前 `Shared`（流可能被重建），随后的背压等待不持任何锁
/// ——否则解码线程会抱着上层 `wasapi_player` 互斥量睡觉，把切设备、停止这些命令一起堵死。
pub struct AaudioProducer {
    inner: Arc<Mutex<Option<Arc<Inner>>>>,
}

impl AaudioProducer {
    /// 本生产者要使用的写入世代。整个解码线程只取一次（见 `decode_push`）：每次推送重取会拿到
    /// "永远新鲜"的世代，等于没有校验。设备切换会重建环，但新环继承同一世代，不会被误判过期。
    #[must_use]
    pub fn write_epoch(&self) -> Option<u64> {
        resolve_shared(&self.inner).map(|s| s.ring.write_epoch())
    }

    /// 把采样推进环形缓冲；缓冲满时停在 park gate 等待，`cancelled` 是取消判据
    /// （generation / thread_id，见 `decode_push`）。
    /// 流断开 / 被取消 / 世代过期时返回 [`PushOutcome::Partial`]，流未起播只需等它可写。
    pub fn push_samples(
        &self,
        samples: &[f32],
        epoch: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<PushOutcome, AppError> {
        let Some(shared) = resolve_shared(&self.inner) else {
            return Err(AppError::msg("AAudio 播放器未初始化"));
        };
        let mut offset = 0;
        while offset < samples.len() {
            // 断开与取消都是终止条件：没人再消费，继续等只会卡住
            if shared.disconnected.load(Ordering::SeqCst) || cancelled() {
                return Ok(PushOutcome::Partial);
            }
            if !shared.running.load(Ordering::Acquire) {
                // 流还没起来（或正在重建）：等它变成可写，别退出也别忙等。
                // 真正终止的情形已由上面的判据与世代校验覆盖。
                shared.park(Duration::from_millis(PARK_TICK_MS));
                continue;
            }
            match shared.ring.push_slice_checked(&samples[offset..], epoch) {
                // 世代过期：本线程已被切歌/停止作废，剩余样本必须丢弃
                None => return Ok(PushOutcome::Partial),
                Some(0) => {
                    // 环满：停在条件变量上，由控制侧或消费进度变化唤醒（见 notify_park）
                    shared.park(Duration::from_millis(PARK_TICK_MS));
                }
                Some(written) => offset += written,
            }
        }
        Ok(PushOutcome::Complete)
    }

    /// 缓冲里尚未被硬件取走的采样数
    #[must_use]
    pub fn buffer_size(&self) -> usize {
        resolve_shared(&self.inner).map_or(0, |s| s.ring.len())
    }

    /// 水位门控未通过时的等待，与 [`Self::push_samples`] 共用同一套唤醒源；可能因
    /// [`PARK_TICK_MS`] 超时返回，调用方须重新检查取消判据与水位。
    pub fn wait_for_space(&self, timeout: Duration) {
        match resolve_shared(&self.inner) {
            Some(shared) => shared.park(timeout),
            None => std::thread::sleep(timeout),
        }
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
            state: Arc::new(AtomicU8::new(ST_UNINITIALIZED)),
            exclusive_confirmed: AtomicBool::new(false),
        }
    }

    /// 当前流的共享状态（克隆 Arc，避免长时间持锁）
    fn shared(&self) -> Option<Arc<Shared>> {
        resolve_shared(&self.inner)
    }

    /// 打开独占流。`device` 传设备 id 的字符串形式（与 WASAPI 版本共用同一签名）；
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
        Ok((rate, channels, device_info.name))
    }

    /// 当前流绑定的输出设备快照（无流时为 `None`）
    #[must_use]
    pub fn current_device(&self) -> Option<OutputDeviceInfo> {
        lock_or_log!(self.inner.lock())
            .as_ref()
            .map(|i| i.device.clone())
    }

    /// 作废当前流（USB DAC 热插拔后由 [`super::on_audio_route_changed`] 调用）。
    /// 必须连同缓存的设备快照一起丢掉：AAudio 的流死绑创建时的 device id，设备重插后系统给的
    /// 是新 id，拿旧快照开流会让独占与共享回退两条路都失败。
    pub fn release_stream(&self) {
        self.close_stream();
    }

    /// 按曲目的原生采样率/声道对齐输出，必要时关掉旧流重开；返回实际生效的 (采样率, 声道)，
    /// 与源一致即为直出（无重采样）。
    /// 没有流时在这里重新解析当前 USB 设备而不是报错：拔插一次后旧设备 id 已失效。
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
            unsafe { ffi::AAudio_createStreamBuilder(&raw mut builder) },
            "createStreamBuilder",
        )?;

        let shared = Arc::new(Shared {
            ring: SampleRing::new(ring_capacity(wanted_rate, wanted_channels)),
            written: AtomicU64::new(0),
            volume: AtomicU32::new(self.volume.load(Ordering::SeqCst)),
            channels: AtomicU32::new(wanted_channels as u32),
            format: AtomicI32::new(wanted_format),
            fade_dir: AtomicU8::new(FADE_IDLE),
            fade_left: AtomicUsize::new(0),
            fade_total: AtomicUsize::new(0),
            fade_deadline_ms: AtomicU64::new(0),
            fade_action: AtomicU64::new(PENDING_NONE),
            pending: AtomicU64::new(PENDING_NONE),
            // 代际从 0 起：首条命令（起播）会把它推进到 1，而 0 代际的槽位是空的
            cmd_seq: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            disconnected: AtomicBool::new(false),
            running: AtomicBool::new(false),
            state: Arc::clone(&self.state),
            park_mutex: Mutex::new(()),
            park_cvar: Condvar::new(),
        });
        let ctx = Arc::new(Ctx {
            shared: Arc::clone(&shared),
            // 请求量由 CALLBACK_MILLIS 折算，开流后再按生效容量校正一次预留，
            // 使正常回调路径上的 resize 只改 len 不触发扩容
            scratch: UnsafeCell::new(Vec::with_capacity(AAUDIO_SCRATCH_CAPACITY)),
        });

        // 回调块按目标时长折算成帧数，容量给到它的四倍：AAudio 头文件要求请求的回调块小于容量
        // 的一半以留出双缓冲，否则会额外插一层内部缓冲。容量只会"至少这么大"，不像采样率那样
        // 会让 open 失败，所以不需要为独占模式准备一条不开容量的退路。
        let callback_frames = (wanted_rate * CALLBACK_MILLIS / 1000).max(1) as i32;
        let capacity_frames = callback_frames.saturating_mul(4);

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
            ffi::AAudioStreamBuilder_setBufferCapacityInFrames(builder, capacity_frames);
            ffi::AAudioStreamBuilder_setFramesPerDataCallback(builder, callback_frames);
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
        let mut result = unsafe { ffi::AAudioStreamBuilder_openStream(builder, &raw mut stream) };
        if result != ffi::AAUDIO_OK {
            // 独占开不出（速率/格式不被接受、被占用）就退共享模式重试。回退时须把速率交回
            // "未指定"并换格式：原样重试只会再失败；共享模式由 AudioFlinger 重采样。
            log::warn!(
                "AAudio 独占开流失败({})，回退共享模式: {}",
                // SAFETY: 结果码转文本，见 ffi::result_to_text
                unsafe { ffi::result_to_text(result) },
                device.name
            );
            // SAFETY: 同上，改共享模式后重新 open
            unsafe {
                ffi::AAudioStreamBuilder_setSharingMode(builder, ffi::AAUDIO_SHARING_MODE_SHARED);
                ffi::AAudioStreamBuilder_setSampleRate(builder, ffi::AAUDIO_UNSPECIFIED);
                ffi::AAudioStreamBuilder_setFormat(builder, shared_fallback_format(wanted_format));
            }
            result = unsafe { ffi::AAudioStreamBuilder_openStream(builder, &raw mut stream) };
            if result != ffi::AAUDIO_OK {
                // 再兜一层：少数老设备连"未指定速率 + 该格式"也不接受，退到最保守的 48k / I16
                log::warn!(
                    "AAudio 共享模式仍未开出({})，改用 48000Hz / I16 重试",
                    // SAFETY: 结果码转文本
                    unsafe { ffi::result_to_text(result) }
                );
                // SAFETY: 同上，换最保守的速率与格式后重新 open
                unsafe {
                    ffi::AAudioStreamBuilder_setSampleRate(builder, 48_000);
                    ffi::AAudioStreamBuilder_setFormat(builder, ffi::AAUDIO_FORMAT_PCM_I16);
                }
                result = unsafe { ffi::AAudioStreamBuilder_openStream(builder, &raw mut stream) };
            }
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
        // SAFETY: stream 由 openStream 成功创建，以下两个查询只读
        let (burst, capacity) = unsafe {
            (
                ffi::AAudioStream_getFramesPerBurst(stream),
                ffi::AAudioStream_getBufferCapacityInFrames(stream),
            )
        };
        // 刻意不调 AAudioStream_setBufferSizeInFrames 把缓冲顶到容量上限：那会让多达 capacity
        // 的音频滞留在流内部，而 seek/stop 走的是 `ring.clear()`、没有 requestFlush，这段在途
        // 音频会变成可听的旧内容尾巴。省电收益来自回调块变大，不来自缓冲变大。

        // 容量和回调块都可能被 AAudio 按设备约束调整（独占模式尤甚），据实际值预留 scratch：
        // 多声道高速率下 25ms 会超过 AAUDIO_SCRATCH_CAPACITY，不预留的话首个回调要在实时
        // 线程上 realloc
        let scratch_cap =
            (capacity.max(0) as usize * actual_channels as usize).max(AAUDIO_SCRATCH_CAPACITY);
        // SAFETY: ctx 尚未放进 Inner，回调要等 start 之后才运行，此刻只有本线程能碰到
        unsafe { (*ctx.scratch.get()).reserve(scratch_cap) };

        shared
            .channels
            .store(actual_channels as u32, Ordering::Relaxed);
        shared.format.store(actual_format, Ordering::Relaxed);
        // running 是"可以按当前格式收发"的信号，必须最后发布：先置 true 时回调可能拿着
        // 旧的 channels/format 计算偏移，按错误的采样宽度读写
        shared.running.store(true, Ordering::Release);

        self.exclusive_confirmed.store(
            sharing == ffi::AAUDIO_SHARING_MODE_EXCLUSIVE,
            Ordering::SeqCst,
        );
        self.sample_rate.store(actual_rate, Ordering::SeqCst);
        self.channels
            .store(actual_channels as u32, Ordering::SeqCst);
        self.set_state(ST_STOPPED);

        *lock_or_log!(self.inner.lock()) = Some(Arc::new(Inner {
            stream: Mutex::new(stream),
            ctx,
            device: device.clone(),
        }));
        // 唤醒停在 park gate 上的解码线程：重建流时它会一直等 `running`，靠 250ms 兜底太迟钝。
        // 放在 Inner 安装之后，反过来的话线程会先醒来、却仍然只看到旧流。
        shared.notify_park();

        log::info!(
            "AAudio stream opened: {} @ {actual_rate}Hz, {actual_channels}ch, format={actual_format}, sharing={sharing}",
            device.name
        );
        // burst 与容量一起打出来：真机上据此判断 AAudio 有没有按请求放大回调块
        log::info!(
            "AAudio buffer: burst={burst}, capacity={capacity}, callback_frames={callback_frames}"
        );
        // 看门狗随开流启动而非 initialize：ensure_format 重建流时也需要它，重复启动无副作用
        self.start_watchdog();
        Ok((actual_rate, actual_channels))
    }

    /// 关闭并释放当前流（close 会等回调结束，因此之后才能释放 ctx）
    fn close_stream(&self) {
        let taken = lock_or_log!(self.inner.lock()).take();
        let Some(inner) = taken else { return };

        inner.ctx.shared.running.store(false, Ordering::Release);
        // 取走句柄而不只是置 null：锁外读指针的调用者可能已经拿到旧句柄（见 `Inner::with_control`）
        let stream = {
            let mut handle = lock_or_log!(inner.stream.lock());
            std::mem::replace(&mut *handle, ptr::null_mut())
        };
        if !stream.is_null() {
            // SAFETY: stream 由 openStream 创建且尚未 close；close 不能从回调里调用，
            // 这里一定在命令线程上
            unsafe {
                let _ = ffi::AAudioStream_requestStop(stream);
                let _ = ffi::AAudioStream_close(stream);
            }
        }
        drop(inner); // ctx 也随之释放（close 已保证回调不再运行）
        self.set_state(ST_UNINITIALIZED);
        self.exclusive_confirmed.store(false, Ordering::SeqCst);
        // 必须停掉并 join：否则重建流时 start_watchdog 会因旧线程未退出而直接返回，新流再无
        // 看门狗，pause/stop 落地（PENDING_*、断连收尾）与淡出兜底会一起失效。
        self.stop_watchdog();
    }

    /// 停掉看门狗线程并等它退出（可重复调用；Drop 与 close_stream 都会走这里）
    fn stop_watchdog(&self) {
        self.watchdog_stop.store(true, Ordering::SeqCst);
        // 先 take 再 join：写成 if let 的 scrutinee 会让 MutexGuard 活到整个 if let 结束
        let handle = lock_or_log!(self.watchdog.lock()).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    /// 启动看门狗：执行淡出后的 pause/stop、处理设备断开
    fn start_watchdog(&self) {
        let mut guard = lock_or_log!(self.watchdog.lock());
        // 线程 panic 退出后 handle 仍是 Some，只看 is_some 会让它永不重启——而 pause/stop
        // 的落地（PENDING_* 执行、淡出兜底、断连收尾）完全依赖这只狗
        if guard.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }
        self.watchdog_stop.store(false, Ordering::SeqCst);
        let stop = Arc::clone(&self.watchdog_stop);
        let inner_getter = WatchdogRef {
            inner: Arc::clone(&self.inner),
        };
        let handle = std::thread::spawn(move || {
            // 上一次看到的环水位，用于识别"消费侧腾出了空间"
            let mut last_ring_len = 0usize;
            // 欠载计数与下次允许上报的时刻（实时回调里不能打日志，观测只能借看门狗的节拍）
            let mut last_underruns = 0u64;
            let mut next_underrun_log = std::time::Instant::now();
            while !stop.load(Ordering::SeqCst) {
                if let Some(inner) = inner_getter.current() {
                    let shared = &inner.ctx.shared;
                    // 回调停摆（流已暂停/断开）时斜坡会停在半途，pending 永不落地，
                    // 状态卡在 Pausing/Stopping。过了兜底截止时刻就替回调收尾
                    let fade_dir_now = shared.fade_dir.load(Ordering::SeqCst);
                    if fade_dir_now != FADE_IDLE
                        && shared.fade_left.load(Ordering::SeqCst) > 0
                        && shared.fade_deadline_ms.load(Ordering::SeqCst) <= now_ms()
                    {
                        log::warn!(
                            "AAudio 斜坡无人推进（fade_dir={fade_dir_now}），看门狗兜底收尾"
                        );
                        shared.fade_dir.store(FADE_IDLE, Ordering::SeqCst);
                        shared.fade_left.store(0, Ordering::SeqCst);
                        let packed = shared.fade_action.swap(PENDING_NONE, Ordering::SeqCst);
                        shared.handoff_fade_action(packed);
                    }
                    // 取待办；过期判定与实际下发都在控制锁内完成，与 request_start 的代际自增
                    // 串行。关流（拔 DAC / 换采样率）时看到 null，动作同样作废。
                    let packed = shared.pending.swap(PENDING_NONE, Ordering::SeqCst);
                    if packed != PENDING_NONE {
                        let action = pending_action(packed);
                        let seq = pending_seq(packed);
                        inner.with_stream(|stream| {
                            if seq != shared.cmd_seq.load(Ordering::SeqCst) {
                                log::debug!(
                                    "AAudio 待办动作已过期（seq={seq}，action={action}），作废"
                                );
                                return;
                            }
                            match action {
                                PENDING_PAUSE => {
                                    // SAFETY: 句柄未关闭；控制锁保证 close 不会与本次调用并发
                                    let r = unsafe { ffi::AAudioStream_requestPause(stream) };
                                    log::info!("AAudio requestPause -> {}", unsafe {
                                        ffi::result_to_text(r)
                                    });
                                    // 命令线程只置到 Pausing，真正暂停发生在这里：必须把状态推进，
                                    // 否则按键/路由判断会一直以为还在 Pausing 而重复 requestStart
                                    transition_state(&shared.state, ST_PAUSED);
                                }
                                PENDING_STOP => {
                                    // SAFETY: 同上
                                    unsafe {
                                        let _ = ffi::AAudioStream_requestStop(stream);
                                    }
                                    shared.ring.invalidate();
                                    shared.notify_park();
                                    shared.written.store(0, Ordering::Relaxed);
                                    transition_state(&shared.state, ST_STOPPED);
                                }
                                _ => {}
                            }
                        });
                    }
                    if shared.disconnected.swap(false, Ordering::SeqCst) {
                        // 断开后不会再有二次通知，须在此收尾：否则回调只写静音、状态仍是
                        // Playing，进度照走却无声。不清 written，让位置停在断开处。
                        log::error!(
                            "AAudio 流已断开（USB DAC 被拔出或设备被抢占），停止输出等待重建"
                        );
                        shared.running.store(false, Ordering::Release);
                        inner.with_stream(|stream| {
                            // SAFETY: 句柄未关闭；控制锁保证 close 不会与本次调用并发
                            unsafe {
                                let _ = ffi::AAudioStream_requestStop(stream);
                            }
                        });
                        shared.ring.invalidate();
                        shared.notify_park();
                        transition_state(&shared.state, ST_STOPPED);
                    }
                    // 水位下降说明消费侧腾出了空间，唤醒 park gate 上的解码线程补货。实时回调
                    // 不能唤醒非实时线程，故借看门狗 20ms 节拍代劳；暂停时水位不动，不唤醒。
                    let ring_len = shared.ring.len();
                    if ring_len < last_ring_len {
                        shared.notify_park();
                    }
                    last_ring_len = ring_len;
                    // 欠载观测：回调里打日志会破坏实时性，改由看门狗按固定间隔汇报增量。没有它，
                    // 偶发 click 在现场无法归因是解码供给不足还是存储抖动（对照 WASAPI 侧的
                    // UnderrunLogger）。
                    let underruns = shared.underruns.load(Ordering::Relaxed);
                    if underruns != last_underruns && std::time::Instant::now() >= next_underrun_log
                    {
                        log::warn!(
                            "AAudio 欠载 {} 次（累计 {underruns}），解码供给或存储可能跟不上",
                            underruns - last_underruns
                        );
                        last_underruns = underruns;
                        next_underrun_log = std::time::Instant::now()
                            + Duration::from_secs(UNDERRUN_LOG_INTERVAL_SECS);
                    }
                }
                std::thread::sleep(Duration::from_millis(WATCHDOG_INTERVAL_MS));
            }
        });
        *guard = Some(handle);
    }

    // 播放控制

    pub fn start(&self) -> Result<(), AppError> {
        self.request_start()
    }

    pub fn stop(&self) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        // 取消进行中的淡出，否则回调结束后会把 pending 覆盖成暂停
        shared.cancel_fade();
        shared.queue_pending(PENDING_STOP);
        // invalidate 而非 clear：同时作废仍持有旧世代的解码线程。只清空是不够的——被作废前正
        // 卡在 park gate 上的线程会在腾出空间后把上一首的样本写进来。
        // 唤醒放在 invalidate 之后：顺序反了线程可能先醒来、发现世代仍有效后重新睡下。
        shared.ring.invalidate();
        shared.notify_park();
        self.set_state(ST_STOPPED);
        Ok(())
    }

    pub fn stop_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.begin_fade_out(duration_ms, FadeAction::Stop);
        Ok(())
    }

    pub fn pause(&self) -> Result<(), AppError> {
        self.pause_with_fade_out(DEFAULT_FADE_MS)
    }

    pub fn pause_no_fade(&self) -> Result<(), AppError> {
        let Some(shared) = self.shared() else {
            return Ok(());
        };
        shared.cancel_fade();
        shared.queue_pending(PENDING_PAUSE);
        self.set_state(ST_PAUSED);
        Ok(())
    }

    pub fn pause_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.begin_fade_out(duration_ms, FadeAction::Pause);
        Ok(())
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
            shared.fade_total.store(frames, Ordering::SeqCst);
            shared.fade_left.store(frames, Ordering::SeqCst);
            shared.fade_action.store(PENDING_NONE, Ordering::SeqCst);
            shared.fade_deadline_ms.store(
                now_ms() + duration_ms as u64 + FADE_FALLBACK_GRACE_MS,
                Ordering::SeqCst,
            );
            shared.fade_dir.store(FADE_IN, Ordering::SeqCst);
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

    /// 取无锁写入端，见 [`AaudioProducer`]。未初始化时也可取到，写入时报错。
    #[must_use]
    pub fn producer(&self) -> AaudioProducer {
        AaudioProducer {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn clear_buffer(&self) -> Result<(), AppError> {
        if let Some(shared) = self.shared() {
            // invalidate 而非 clear：切歌/seek 时同时作废旧世代的解码线程（见 `stop`）
            shared.ring.invalidate();
            shared.notify_park();
            shared.written.store(0, Ordering::SeqCst);
        }
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> PlaybackState {
        state_from_code(self.state.load(Ordering::SeqCst))
    }

    fn set_state(&self, new: u8) {
        transition_state(&self.state, new);
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

    // 内部实现

    fn request_start(&self) -> Result<(), AppError> {
        let handle = {
            let guard = lock_or_log!(self.inner.lock());
            guard.as_ref().map(Arc::clone)
        };
        let Some(inner) = handle else {
            return Err(AppError::msg("AAudio 播放器未初始化"));
        };
        // 代际自增、清待办、句柄使用全在同一临界区，与关流互斥（close 会等 requestStart 返回）。
        // 递增代际使"暂停后快速恢复"时排队的旧暂停/旧停止立即过期，见 `pack_pending`。
        let r = inner.with_control(|stream| {
            let shared = &inner.ctx.shared;
            shared.bump_cmd_seq();
            shared.pending.store(PENDING_NONE, Ordering::SeqCst);
            // 取消可能正在进行中的淡出，理由见 `cancel_fade_out`
            shared.cancel_fade_out();
            let Some(stream) = stream else {
                return Err(AppError::msg("AAudio 流未打开"));
            };
            // SAFETY: 句柄未关闭；控制锁保证 close 不会与本次调用并发
            Ok(unsafe { ffi::AAudioStream_requestStart(stream) })
        })?;
        if r != ffi::AAUDIO_OK {
            // SAFETY: 结果码转文本
            return Err(AppError::msg(format!(
                "AAudio requestStart 失败: {}",
                unsafe { ffi::result_to_text(r) }
            )));
        }
        // 起播成功后才发布 running：失败时流根本没在跑，置了会让回调与解码线程都以为可写
        inner.ctx.shared.running.store(true, Ordering::Release);
        // 唤醒停在 park gate 上的解码线程：恢复播放会让它等的条件（缓冲能装下的量）重新成立
        inner.ctx.shared.notify_park();
        self.set_state(ST_PLAYING);
        Ok(())
    }

    fn begin_fade_out(&self, duration_ms: u32, action: FadeAction) {
        let Some(shared) = self.shared() else {
            return;
        };
        let frames = fade_frames(duration_ms, self.sample_rate.load(Ordering::SeqCst));
        let action_code = if action == FadeAction::Pause {
            PENDING_PAUSE
        } else {
            PENDING_STOP
        };
        if frames == 0 {
            shared.queue_pending(action_code);
            shared.fade_dir.store(FADE_IDLE, Ordering::SeqCst);
        } else {
            // 先写斜坡参数，最后置方向，避免回调读到半更新状态
            shared.fade_total.store(frames, Ordering::SeqCst);
            shared.fade_left.store(frames, Ordering::SeqCst);
            // 带上发起淡出时的代际：途中若用户恢复播放，这个动作会被看门狗判为过期
            let packed = pack_pending(shared.cmd_seq.load(Ordering::SeqCst), action_code);
            shared.fade_action.store(packed, Ordering::SeqCst);
            shared.fade_deadline_ms.store(
                now_ms() + duration_ms as u64 + FADE_FALLBACK_GRACE_MS,
                Ordering::SeqCst,
            );
            shared.fade_dir.store(FADE_OUT, Ordering::SeqCst);
        }
        self.set_state(if action == FadeAction::Pause {
            ST_PAUSING
        } else {
            ST_STOPPING
        });
    }
}

impl Drop for AaudioExclusivePlayer {
    fn drop(&mut self) {
        self.stop_watchdog();
        self.close_stream();
    }
}

// 回调与工具

/// 环形缓冲容量：由 [`crate::audio::sample_ring::exclusive_ring_capacity`] 统一推导，
/// 保证严格大于解码线程的水位门控（否则门控形同虚设，背压等待会变成主节奏点）。
fn ring_capacity(sample_rate: u32, channels: u16) -> usize {
    crate::audio::sample_ring::exclusive_ring_capacity(sample_rate, channels, RING_SECONDS)
}

fn fade_frames(duration_ms: u32, sample_rate: u32) -> usize {
    if duration_ms == 0 || sample_rate == 0 {
        return 0;
    }
    ((sample_rate as u64 * duration_ms as u64) / 1000) as usize
}

/// 按设备上报的编码挑一个格式：优先 float（解码器输出本身就是 f32，免一次量化），
/// 其次是整数位深，最后 16 位。取值见 `android.media.AudioFormat.ENCODING_*`。
fn pick_format(device: &OutputDeviceInfo) -> ffi::aaudio_format_t {
    const ENCODING_PCM_16BIT: i32 = 2;
    const ENCODING_PCM_FLOAT: i32 = 4;
    const ENCODING_PCM_24BIT_PACKED: i32 = 21;
    const ENCODING_PCM_32BIT: i32 = 22;

    if device.encodings.is_empty() {
        // 设备没上报：float 在 AAudio 上总是可用（必要时由框架转换）
        return ffi::AAUDIO_FORMAT_PCM_FLOAT;
    }
    if device.encodings.contains(&ENCODING_PCM_FLOAT) {
        ffi::AAUDIO_FORMAT_PCM_FLOAT
    } else if device.encodings.contains(&ENCODING_PCM_32BIT) {
        ffi::AAUDIO_FORMAT_PCM_I32
    } else if device.encodings.contains(&ENCODING_PCM_24BIT_PACKED) {
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

/// 回退共享模式时用的采样格式：共享模式只保证支持 FLOAT 与 I16，
/// 24-bit packed（API 31+）与 I32 在部分设备上会直接让 openStream 失败。
const fn shared_fallback_format(wanted: ffi::aaudio_format_t) -> ffi::aaudio_format_t {
    if wanted == ffi::AAUDIO_FORMAT_PCM_FLOAT {
        ffi::AAUDIO_FORMAT_PCM_FLOAT
    } else {
        ffi::AAUDIO_FORMAT_PCM_I16
    }
}

/// 数据回调入口：只做一层 panic 收口，真正的工作在 [`fill_block`]。
///
/// 回调里的 panic 会跨 FFI 边界——`panic = "abort"` 构建直接杀进程，否则 unwind 穿进 AAudio 的
/// C 栈（UB）。此处能接住就补静音继续放；abort 下 catch_unwind 接不住，故 [`fill_block`]
/// 只允许有界索引与原子量操作，不留 panic 路径。
unsafe extern "C" fn data_callback(
    _stream: *mut ffi::AAudioStreamStruct,
    user: *mut c_void,
    audio: *mut c_void,
    frames: i32,
) -> ffi::aaudio_data_callback_result_t {
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: 见 fill_block 的前置条件
        unsafe { fill_block(user, audio, frames) }
    }));
    if caught.is_err() && !audio.is_null() && frames > 0 {
        // 实时线程上不做格式化、不分配，静默补零即可
        // SAFETY: 同 fill_block —— user 仍指向存活的 Ctx，audio 由 AAudio 提供
        let ctx = unsafe { &*(user as *const Ctx) };
        write_silence(&ctx.shared, audio, frames as usize);
    }
    ffi::AAUDIO_CALLBACK_RESULT_CONTINUE
}

/// 数据回调主体。
/// # Safety
/// `user` 必须是 `open_stream` 传入且仍存活的 `Arc<Ctx>` 裸指针（`AAudioStream_close`
/// 会等本次回调返回后才释放）；`audio` 为 AAudio 输出区，容量 `frames * channels`。
unsafe fn fill_block(user: *mut c_void, audio: *mut c_void, frames: i32) {
    // SAFETY: userData 是 open_stream 时传入的 Arc<Ctx> 裸指针，
    // 释放发生在 AAudioStream_close 之后（close 会等本次回调返回）
    let ctx = unsafe { &*(user as *const Ctx) };
    let shared = &ctx.shared;
    if frames <= 0 || audio.is_null() {
        return;
    }
    if !shared.running.load(Ordering::Acquire) {
        write_silence(shared, audio, frames as usize);
        return;
    }

    let channels = shared.channels.load(Ordering::Relaxed).max(1) as usize;
    let need = frames as usize * channels;

    // SAFETY: scratch 只在本回调线程访问
    let scratch = unsafe { &mut *ctx.scratch.get() };
    scratch.clear();
    let got = shared.ring.drain_into(scratch, need);
    if got < need {
        scratch.resize(need, 0.0);
        shared.underruns.fetch_add(1, Ordering::Relaxed);
        // 补零本身是硬切：最后一个真样本与紧随其后的 0 之间的阶跃就是一次可听 click。
        // 把缺口之前的最后一小段线性收敛到 0，阶跃就变成斜率有界的斜坡。
        let ramp_frames = UNDERRUN_RAMP_FRAMES.min(got / channels);
        let start = got - ramp_frames * channels;
        for f in 0..ramp_frames {
            // 末帧增益恰为 0，与缺口里的静音无缝衔接
            let gain = 1.0 - (f + 1) as f32 / ramp_frames as f32;
            for c in 0..channels {
                scratch[start + f * channels + c] *= gain;
            }
        }
    }

    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    // 斜坡按帧而不是按采样推进：一帧含 channels 个采样，按采样推进会在半个回调块内就走完斜坡
    // 并继续过冲（淡出时增益转负，即反相失真），左右声道还不一致
    let (fade_start, fade_end) = {
        let dir = shared.fade_dir.load(Ordering::Relaxed);
        let total = shared.fade_total.load(Ordering::Relaxed);
        if dir == FADE_IDLE || total == 0 {
            (1.0f32, 1.0f32)
        } else {
            let now = shared.fade_left.load(Ordering::Relaxed);
            let left = now.saturating_sub(frames as usize);
            shared.fade_left.store(left, Ordering::Relaxed);
            let ratio_now = now as f32 / total as f32;
            let ratio_left = left as f32 / total as f32;
            let gain = if dir == FADE_OUT {
                (ratio_now, ratio_left)
            } else {
                (1.0 - ratio_now, 1.0 - ratio_left)
            };
            if left == 0 {
                shared.fade_dir.store(FADE_IDLE, Ordering::Relaxed);
                if dir == FADE_OUT {
                    // 转交打包了代际的动作，过期判定见 `pack_pending`
                    let packed = shared.fade_action.swap(PENDING_NONE, Ordering::SeqCst);
                    shared.handoff_fade_action(packed);
                }
            }
            gain
        }
    };

    let step = if frames > 1 {
        (fade_end - fade_start) / (frames - 1) as f32
    } else {
        0.0
    };

    // SAFETY: audio 由 AAudio 提供，容量 = frames * channels * 采样字节数
    unsafe {
        match shared.format.load(Ordering::Relaxed) {
            ffi::AAUDIO_FORMAT_PCM_FLOAT => {
                let out = std::slice::from_raw_parts_mut(audio.cast::<f32>(), need);
                for (i, dst) in out.iter_mut().enumerate() {
                    *dst = scratch[i] * volume * (fade_start + step * (i / channels) as f32);
                }
            }
            ffi::AAUDIO_FORMAT_PCM_I32 => {
                let out = std::slice::from_raw_parts_mut(audio.cast::<i32>(), need);
                for (i, dst) in out.iter_mut().enumerate() {
                    let v = scratch[i] * volume * (fade_start + step * (i / channels) as f32);
                    *dst = (v.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
                }
            }
            ffi::AAUDIO_FORMAT_PCM_I24_PACKED => {
                // 3 字节小端、无对齐：逐样本展开写入
                let v_scale = 8_388_607.0f32; // 2^23 - 1
                let out = std::slice::from_raw_parts_mut(audio.cast::<u8>(), need * 3);
                for i in 0..need {
                    let v = scratch[i] * volume * (fade_start + step * (i / channels) as f32);
                    let q = (v.clamp(-1.0, 1.0) * v_scale) as i32;
                    let b = q.to_le_bytes();
                    out[i * 3] = b[0];
                    out[i * 3 + 1] = b[1];
                    out[i * 3 + 2] = b[2];
                }
            }
            _ => {
                // I16（未知格式也按 I16 处理，AAudio 不接受时已在 open 阶段回落共享模式）
                let out = std::slice::from_raw_parts_mut(audio.cast::<i16>(), need);
                for (i, dst) in out.iter_mut().enumerate() {
                    let v = scratch[i] * volume * (fade_start + step * (i / channels) as f32);
                    *dst = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                }
            }
        }
    }

    // 只计真正取走的采样：欠载补的零不是音乐。按 need 计会让播放位置比实际听到的跑得快
    // （卡顿或拔线几秒后位置已经跳过去一段，进度条与声音对不上）。
    shared.written.fetch_add(got as u64, Ordering::Relaxed);
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
    // 与 fill_block 同口径地 .max(1)：channels 尚未写入（流刚开）时它会短暂为 0，那时 n 也变成
    // 0，等于什么都没写、把上一轮的残留数据留在了输出区
    let channels = (shared.channels.load(Ordering::Relaxed) as usize).max(1);
    let n = frames * channels;
    // SAFETY: audio 由 AAudio 提供，容量与格式匹配
    unsafe {
        match shared.format.load(Ordering::Relaxed) {
            ffi::AAUDIO_FORMAT_PCM_FLOAT => {
                std::slice::from_raw_parts_mut(audio.cast::<f32>(), n).fill(0.0);
            }
            ffi::AAUDIO_FORMAT_PCM_I32 => {
                std::slice::from_raw_parts_mut(audio.cast::<i32>(), n).fill(0);
            }
            ffi::AAUDIO_FORMAT_PCM_I24_PACKED => {
                std::slice::from_raw_parts_mut(audio.cast::<u8>(), n * 3).fill(0);
            }
            _ => {
                std::slice::from_raw_parts_mut(audio.cast::<i16>(), n).fill(0);
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
        lock_or_log!(self.inner.lock()).clone()
    }
}
