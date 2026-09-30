//! WASAPI 独占模式音频输出：播放器句柄，以及它与渲染线程之间的命令/响应协议。
//!
//! 分工：`ring` 是无锁采样环形缓冲，`render` 是渲染线程主体，
//! `device` 负责独占设备初始化与格式协商，`simd` 是 f32 到整型字节的转换。

mod device;
mod render;
mod ring;
mod simd;

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

// 播放器状态已提到平台无关的 `crate::audio::PlaybackState`：
// Android 的 AAudio 独占通道与命令层共用同一套状态语义。
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

impl WasapiExclusivePlayback {
    #[must_use]
    pub fn new() -> Self {
        let (command_tx, command_rx) = bounded::<AudioCommand>(64);
        let (response_tx, response_rx) = bounded::<AudioResponse>(64);

        let state = Arc::new(Mutex::new(PlaybackState::Uninitialized));
        let volume = Arc::new(Mutex::new(1.0f32));
        let is_running = Arc::new(AtomicBool::new(true));
        let samples_written = Arc::new(AtomicU64::new(0));
        // 无锁环形缓冲:一次性按最坏情况预分配(见 SPSC_RING_CAPACITY 注释)
        let sample_buffer = Arc::new(SpscSampleRing::new(SPSC_RING_CAPACITY));
        let sample_rate = Arc::new(AtomicU32::new(48000));

        let state_clone = Arc::clone(&state);
        let volume_clone = Arc::clone(&volume);
        let is_running_clone = Arc::clone(&is_running);
        let sample_buffer_clone = Arc::clone(&sample_buffer);
        let samples_written_clone = Arc::clone(&samples_written);
        let sample_rate_clone = Arc::clone(&sample_rate);

        let audio_thread = thread::spawn(move || {
            audio_thread_main(
                command_rx,
                response_tx,
                state_clone,
                volume_clone,
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

        // 使用超时接收响应，防止无限等待
        match self.response_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(AudioResponse::Initialized {
                sample_rate,
                channels,
                device_name,
            }) => {
                self.sample_rate.store(sample_rate, Ordering::SeqCst);
                self.channels.store(u32::from(channels), Ordering::SeqCst);
                *lock_or_log!(self.state.lock()) = PlaybackState::Stopped;
                // 采样缓冲(SPSC 环形缓冲)已按最坏情况预分配,无需按格式调整
                Ok((sample_rate, channels, device_name))
            }
            Ok(AudioResponse::InitFailed(e)) => Err(e.into()),
            Ok(other) => Err(format!("Unexpected response: {other:?}").into()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                log::warn!("WASAPI initialize timed out, cleaning up stale responses");
                // 清空可能残留的过时响应,避免影响后续命令
                while self.response_rx.try_recv().is_ok() {}
                // 超时后设备未成功初始化,重置状态为 Uninitialized
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
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {e}"))?;
        *lock_or_log!(self.state.lock()) = PlaybackState::Stopped;
        Ok(())
    }

    /// 带淡出的停止(用于切歌/退出)
    /// 主线程发送命令后立即返回,音频线程内部完成淡出再 stop_stream
    pub fn stop_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::StopWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send stop_with_fade_out command: {e}"))?;
        // 状态标记为 Stopping,表示音频线程仍在淡出;淡出完成后由 FadeAction::Stop 转为 Stopped
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

    /// 带淡出的暂停(默认用于用户暂停)
    pub fn pause_with_fade_out(&self, duration_ms: u32) -> Result<(), AppError> {
        self.command_tx
            .send(AudioCommand::PauseWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send pause_with_fade_out command: {e}"))?;
        // 状态标记为 Pausing,表示音频线程仍在淡出;淡出完成后由 FadeAction::Pause 转为 Paused
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

    /// 带淡入的恢复(默认用于用户恢复)
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

    pub fn push_samples(&self, samples: &[f32]) -> Result<(), AppError> {
        let written = self.sample_buffer.push_slice(samples);
        if written < samples.len() {
            // 生产者有 2 秒水位门控,正常不应满;截断意味着门控失效(如异常设备格式)
            log::warn!("SPSC 缓冲已满,截断 {} 采样", samples.len() - written);
        }
        Ok(())
    }

    pub fn clear_buffer(&self) -> Result<(), AppError> {
        self.sample_buffer.clear();
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

    /// 获取已写入硬件的采样数
    #[must_use]
    pub fn samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::SeqCst)
    }

    /// 重置已写入采样计数器
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
