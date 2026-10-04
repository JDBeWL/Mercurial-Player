//! WASAPI 独占模式音频输出：播放器句柄，以及它与渲染线程之间的命令/响应协议。
//!
//! 分工：`ring` 无锁采样缓冲，`render` 渲染线程主体，`device` 设备初始化与格式协商，`simd` 采样转换。

mod device;
mod render;
mod ring;
mod simd;

use crate::audio::PushOutcome;
use crate::error::AppError;
use crossbeam_channel::{Receiver, Sender, bounded};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use render::audio_thread_main;
use ring::{SPSC_RING_CAPACITY, SpscSampleRing};

/// WASAPI 独占模式默认淡入/淡出时长(毫秒),用于消除暂停/恢复时的 audible click
const WASAPI_FADE_MS: u32 = 30;

/// 音频线程命令
pub enum AudioCommand {
    Initialize {
        device_name: Option<String>,
    },
    Start,
    Stop,
    Pause,
    Resume,
    SetVolume(f32),
    ClearBuffer,
    Shutdown,
    /// 带淡出的停止(用于切歌/退出场景)
    StopWithFadeOut {
        duration_ms: u32,
    },
    /// 带淡出的暂停(用于用户暂停)
    PauseWithFadeOut {
        duration_ms: u32,
    },
    /// 带淡入的恢复(用于用户恢复)
    ResumeWithFadeIn {
        duration_ms: u32,
    },
}

impl std::fmt::Debug for AudioCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialize { device_name } => f
                .debug_struct("Initialize")
                .field("device_name", device_name)
                .finish(),
            Self::Start => write!(f, "Start"),
            Self::Stop => write!(f, "Stop"),
            Self::Pause => write!(f, "Pause"),
            Self::Resume => write!(f, "Resume"),
            Self::SetVolume(arg0) => f.debug_tuple("SetVolume").field(arg0).finish(),
            Self::ClearBuffer => write!(f, "ClearBuffer"),
            Self::Shutdown => write!(f, "Shutdown"),
            Self::StopWithFadeOut { duration_ms } => f
                .debug_struct("StopWithFadeOut")
                .field("duration_ms", duration_ms)
                .finish(),
            Self::PauseWithFadeOut { duration_ms } => f
                .debug_struct("PauseWithFadeOut")
                .field("duration_ms", duration_ms)
                .finish(),
            Self::ResumeWithFadeIn { duration_ms } => f
                .debug_struct("ResumeWithFadeIn")
                .field("duration_ms", duration_ms)
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeAction {
    /// 淡出后暂停(stop_stream)
    Pause,
    /// 淡出后停止(stop_stream + clear_buffer)
    Stop,
}

/// 音频线程响应
#[derive(Debug)]
pub enum AudioResponse {
    Initialized {
        sample_rate: u32,
        channels: u16,
        device_name: String,
    },
    InitFailed(String),
    Ok,
    Error(String),
}

// 状态语义与 AAudio 独占通道、命令层共用，故提到平台无关的 `crate::audio::PlaybackState`
pub use crate::audio::PlaybackState;

/// WASAPI 独占模式播放器
pub struct WasapiExclusivePlayback {
    command_tx: Sender<AudioCommand>,
    response_rx: Receiver<AudioResponse>,
    audio_thread: Option<JoinHandle<()>>,
    state: Arc<Mutex<PlaybackState>>,
    sample_rate: Arc<AtomicU32>,
    channels: AtomicU32,
    volume: Arc<Mutex<f32>>,
    is_running: Arc<AtomicBool>,
    /// 无锁 SPSC 采样缓冲:音频线程消费,解码线程生产,宿主线程查水位/清空
    sample_buffer: Arc<SpscSampleRing>,
    /// 已写入硬件的采样数（用于计算播放位置）
    samples_written: Arc<AtomicU64>,
}

/// 解码线程用的无锁写入端，由 `WasapiExclusivePlayback::producer` 取得。
///
/// 推送与水位查询都不再经过 `wasapi_player` 互斥量，背压等待因此堵不住命令线程。
pub struct WasapiProducer {
    ring: Arc<SpscSampleRing>,
}

impl WasapiProducer {
    /// 本生产者要使用的写入世代，整个解码线程只取一次（原因见 `SpscSampleRing::write_epoch`）。
    ///
    /// WASAPI 的环构造时分配、途中不重建，故恒为 `Some`，返回 `Option` 只为与 AAudio 侧同一契约。
    #[allow(clippy::unnecessary_wraps)]
    #[must_use]
    pub fn write_epoch(&self) -> Option<u64> {
        Some(self.ring.write_epoch())
    }

    /// 写入一批采样，被作废（`cancelled` 命中或 `epoch` 过期）时返回 [`PushOutcome::Partial`]。
    ///
    /// 恒为 `Ok`：`Result` 只为与 AAudio 侧对齐（那边流未打开会返回 `Err`）。
    #[allow(clippy::unnecessary_wraps)]
    pub fn push_samples(
        &self,
        samples: &[f32],
        epoch: u64,
        cancelled: impl Fn() -> bool,
    ) -> Result<PushOutcome, AppError> {
        if cancelled() {
            return Ok(PushOutcome::Partial);
        }
        match self.ring.push_slice_checked(samples, epoch) {
            // 世代过期：本线程的样本已被作废，剩余部分必须丢弃
            None => Ok(PushOutcome::Partial),
            Some(written) => {
                if written < samples.len() {
                    // 缓冲满即说明 2 秒水位门控失效（如异常设备格式），正常路径到不了这里
                    log::warn!("SPSC 缓冲已满,截断 {} 采样", samples.len() - written);
                    return Ok(PushOutcome::Partial);
                }
                Ok(PushOutcome::Complete)
            }
        }
    }

    #[must_use]
    pub fn buffer_size(&self) -> usize {
        self.ring.len()
    }

    /// 水位门控未通过时的等待。WASAPI 侧不做背压等待（环容量远大于门控、正常不会满），
    /// 退化为一次普通睡眠，只为与 AAudio 侧保持同一套调用契约，供 `decode_push` 共用。
    #[allow(clippy::unused_self)]
    pub fn wait_for_space(&self, timeout: Duration) {
        thread::sleep(timeout);
    }
}

impl WasapiExclusivePlayback {
    #[must_use]
    pub fn new() -> Self {
        let (command_tx, command_rx) = bounded::<AudioCommand>(64);
        let (response_tx, response_rx) = bounded::<AudioResponse>(64);

        let state = Arc::new(Mutex::new(PlaybackState::Uninitialized));
        let volume = Arc::new(Mutex::new(1.0f32));
        let is_running = Arc::new(AtomicBool::new(true));
        let samples_written = Arc::new(AtomicU64::new(0));
        // 一次性按最坏情况预分配，容量理由见 SPSC_RING_CAPACITY
        let sample_buffer = Arc::new(SpscSampleRing::new(SPSC_RING_CAPACITY));
        let sample_rate = Arc::new(AtomicU32::new(48000));

        let state_clone = Arc::clone(&state);
        let is_running_clone = Arc::clone(&is_running);
        let sample_buffer_clone = Arc::clone(&sample_buffer);
        let samples_written_clone = Arc::clone(&samples_written);
        let sample_rate_clone = Arc::clone(&sample_rate);

        let audio_thread = thread::spawn(move || {
            audio_thread_main(
                command_rx,
                response_tx,
                state_clone,
                is_running_clone,
                sample_buffer_clone,
                samples_written_clone,
                sample_rate_clone,
            );
        });

        Self {
            command_tx,
            response_rx,
            audio_thread: Some(audio_thread),
            state,
            sample_rate,
            channels: AtomicU32::new(2),
            volume,
            is_running,
            sample_buffer,
            samples_written,
        }
    }

    pub fn initialize(&self, device_name: Option<&str>) -> Result<(u32, u16, String), AppError> {
        self.command_tx
            .send(AudioCommand::Initialize {
                device_name: device_name.map(String::from),
            })
            .map_err(|e| format!("Failed to send initialize command: {e}"))?;

        match self.response_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(AudioResponse::Initialized {
                sample_rate,
                channels,
                device_name,
            }) => {
                self.sample_rate.store(sample_rate, Ordering::SeqCst);
                self.channels.store(u32::from(channels), Ordering::SeqCst);
                *lock_or_log!(self.state.lock()) = PlaybackState::Stopped;
                // 不按协商出的格式调整 SPSC 缓冲，它已按最坏情况预分配（见 SPSC_RING_CAPACITY）
                Ok((sample_rate, channels, device_name))
            }
            Ok(AudioResponse::InitFailed(e)) => Err(e.into()),
            Ok(other) => Err(format!("Unexpected response: {other:?}").into()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                log::warn!("WASAPI initialize timed out, cleaning up stale responses");
                // 清空残留的过时响应，否则会错配到下一条命令上
                while self.response_rx.try_recv().is_ok() {}
                *lock_or_log!(self.state.lock()) = PlaybackState::Uninitialized;
                Err(
                    "Device initialization timeout - device may be in use or unavailable"
                        .to_string()
                        .into(),
                )
            }
            Err(e) => Err(format!("Failed to receive response: {e}").into()),
        }
    }

    pub fn start(&self) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::Start)
            .map_err(|e| format!("Failed to send start command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Playing;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), AppError> {
        // 先在命令线程侧作废在推送的解码线程，再让渲染线程停流清空（顺序理由见 SpscSampleRing::invalidate）
        self.sample_buffer.invalidate();
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Stopped;
        Ok(())
    }

    /// 带淡出的停止：命令立即返回，淡出与 stop_stream 都在音频线程内完成
    pub fn stop_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::StopWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send stop_with_fade_out command: {e}"))?;
        // 音频线程仍在淡出，故先记 Stopping，淡出完成后由 FadeAction::Stop 转为 Stopped
        *lock_or_log!(self.state.lock()) = PlaybackState::Stopping;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), AppError> {
        self.pause_with_fade_out(WASAPI_FADE_MS)
    }

    /// 不带淡出的暂停(用于用户禁用淡入淡出的场景)
    pub fn pause_no_fade(&self) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::Pause)
            .map_err(|e| format!("Failed to send pause command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Paused;
        Ok(())
    }

    /// 带淡出的暂停：命令立即返回，淡出与随后的暂停都在音频线程内完成
    pub fn pause_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::PauseWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send pause_with_fade_out command: {e}"))?;
        // 同 stop_with_fade_out：Pausing 由淡出结束时的 FadeAction::Pause 落地为 Paused
        *lock_or_log!(self.state.lock()) = PlaybackState::Pausing;
        Ok(())
    }

    pub fn resume(&self) -> Result<(), AppError> {
        self.resume_with_fade_in(WASAPI_FADE_MS)
    }

    /// 不带淡入的恢复(用于用户禁用淡入淡出的场景)
    pub fn resume_no_fade(&self) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::Resume)
            .map_err(|e| format!("Failed to send resume command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Playing;
        Ok(())
    }

    /// 带淡入的恢复：命令立即返回，淡入由音频线程在渲染回调里推进
    pub fn resume_with_fade_in(&self, duration_ms: u32) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::ResumeWithFadeIn { duration_ms })
            .map_err(|e| format!("Failed to send resume_with_fade_in command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Playing;
        Ok(())
    }

    pub fn set_volume(&self, vol: f32) -> Result<(), AppError> {
        let vol = vol.clamp(0.0, 1.0);
        *lock_or_log!(self.volume.lock()) = vol;
        self.command_tx
            .send(AudioCommand::SetVolume(vol))
            .map_err(|e| format!("Failed to send volume command: {e}").into())
    }

    /// 取无锁写入端，见 [`WasapiProducer`]。
    #[must_use]
    pub fn producer(&self) -> WasapiProducer {
        WasapiProducer {
            ring: Arc::clone(&self.sample_buffer),
        }
    }

    pub fn clear_buffer(&self) -> Result<(), AppError> {
        // invalidate 而非 clear：切歌/seek 时同时作废仍持有旧世代的解码线程
        self.sample_buffer.invalidate();
        self.samples_written.store(0, Ordering::SeqCst);
        Ok(())
    }

    #[must_use]
    pub fn state(&self) -> PlaybackState {
        *lock_or_log!(self.state.lock())
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
        *lock_or_log!(self.volume.lock())
    }

    #[must_use]
    pub fn samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::SeqCst)
    }

    pub fn reset_samples_written(&self) {
        self.samples_written.store(0, Ordering::SeqCst);
    }

    #[must_use]
    pub fn buffer_size(&self) -> usize {
        self.sample_buffer.len()
    }
}

impl Default for WasapiExclusivePlayback {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WasapiExclusivePlayback {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(AudioCommand::Shutdown);
        if let Some(thread) = self.audio_thread.take() {
            let _ = thread.join();
        }
    }
}
