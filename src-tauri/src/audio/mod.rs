//! 音频模块：解码、共享/独占播放、设备管理与频谱。
//!
//! 锁序：`sink` -> `output_stream` -> `target_volume` -> `exclusive_mode` -> `wasapi_player` -> `current_device_name` -> `current_path`；`SampleRing`/`SpscSampleRing` 是无锁 SPSC，不参与锁序。

/// 共享模式播放/恢复的淡入时长(毫秒)：起点没有对应淡出，用稍长淡入掩盖爆音。
pub(crate) const FADE_IN_MS: u64 = 80;
/// seek 时的淡入时长(毫秒)，位置连续故更短。
pub(crate) const FADE_IN_ON_SEEK_MS: u64 = 50;

/// 独占播放器的状态机，Windows 的 WASAPI 与 Android 的 AAudio 共用，命令层与解码推送按它判断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Uninitialized,
    Stopped,
    Playing,
    Paused,
    /// 带淡出的停止中:已请求 StopWithFadeOut,音频线程仍在淡出,完成后转为 Stopped
    Stopping,
    /// 带淡出的暂停中:已请求 PauseWithFadeOut,音频线程仍在淡出,完成后转为 Paused
    Pausing,
}

/// 独占模式解码推送的水位门控（秒）：环内已缓冲量低于该值才继续推送。
/// 两个独占后端的环容量必须严格大于它，否则门控失效，`push_samples` 的背压等待退化成主节奏点。
pub(crate) const EXCLUSIVE_BUFFER_WATERMARK_SECS: usize = 2;

/// 独占后端 `push_samples` 的结果。写不完不能悄悄返回 `Ok`：解码线程会当成写完，一路解码到 EOF
/// 并发出 track-ended（表现为停止后自动续播下一首）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// 本次采样全部写入
    Complete,
    /// 未写完（取消 / 流已停 / 世代过期 / 环满无法继续等待），剩余样本已丢弃；调用方必须停止本线程推送。
    Partial,
}

pub mod commands;

#[cfg(any(windows, target_os = "android"))]
pub mod decode_push;
pub mod decoder;
pub mod dsp;
pub mod emit;
pub mod eq_processor;

pub mod device;
pub mod device_monitor;
pub mod playback;
pub mod queue;
pub mod sample_ring;
pub mod session;
pub mod spectrum;

/// Android 的 AAudio 独占（位完美 / USB DAC 直连）通道
#[cfg(target_os = "android")]
pub mod aaudio;
#[cfg(windows)]
pub mod wasapi;

pub use decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
pub use device::AudioDeviceInfo;
pub use device_monitor::{DeviceChangeEvent, DeviceMonitor};
pub use eq_processor::EqProcessor;
pub use queue::{PlaybackQueue, RepeatMode};
pub use sample_ring::{SampleRing, exclusive_ring_capacity};
pub use spectrum::VisualizationSource;

#[cfg(target_os = "android")]
pub use aaudio::AaudioExclusivePlayer;
#[cfg(windows)]
pub use wasapi::WasapiExclusivePlayback;

/// 按默认设备重建共享输出（Android）；cpal 流不随默认设备迁移也不自愈，不重建则拔 DAC 后无声，失败时保留旧状态。
/// 调用方需保证此刻没有正在播的音频：替换 sink 会丢掉当前播放队列。
#[cfg(target_os = "android")]
pub fn rebuild_shared_sink(state: &crate::AppState) -> Result<(), crate::error::AppError> {
    use rodio::stream::DeviceSinkBuilder;

    let output = &state.player.output;

    // 音量单独读：锁序里 target_volume 在 sink 之后，先读后锁安全，持 sink 时再读才违规。
    let volume = *output
        .target_volume
        .lock()
        .map_err(|e| format!("读取目标音量失败: {e}"))?;

    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("创建默认输出设备失败: {e}"))?
        .open_stream()
        .map_err(|e| format!("打开默认输出流失败: {e}"))?;
    let player = rodio::Player::connect_new(mixer_sink.mixer());
    player.set_volume(volume);

    // 按锁序 sink -> output_stream 依次替换（两个临界区不嵌套）：先换 sink，旧 sink drop 关旧 cpal 流时旧播放器已释放。
    {
        let mut sink = output
            .sink
            .lock()
            .map_err(|e| format!("替换共享播放器失败: {e}"))?;
        *sink = player;
    }
    {
        let mut stream = output
            .output_stream
            .lock()
            .map_err(|e| format!("替换共享输出流失败: {e}"))?;
        *stream = Some(mixer_sink);
    }

    log::info!("共享模式输出已按当前默认设备重建");
    Ok(())
}

use std::sync::{LockResult, MutexGuard, RwLockReadGuard, RwLockWriteGuard, TryLockError};

/// 锁获取失败转成带锁名的描述性错误；命令边界用它，核心路径用 `lock_or_log!`（锁中毒不中断播放）。
pub(crate) trait LockOrErr {
    type Guard;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String>;
}

impl<'a, T> LockOrErr for LockResult<MutexGuard<'a, T>> {
    type Guard = MutexGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}

impl<'a, T> LockOrErr for LockResult<RwLockReadGuard<'a, T>> {
    type Guard = RwLockReadGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}

impl<'a, T> LockOrErr for LockResult<RwLockWriteGuard<'a, T>> {
    type Guard = RwLockWriteGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}

impl<'a, T> LockOrErr for Result<MutexGuard<'a, T>, TryLockError<MutexGuard<'a, T>>> {
    type Guard = MutexGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}

impl<'a, T> LockOrErr for Result<RwLockReadGuard<'a, T>, TryLockError<RwLockReadGuard<'a, T>>> {
    type Guard = RwLockReadGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}

impl<'a, T> LockOrErr for Result<RwLockWriteGuard<'a, T>, TryLockError<RwLockWriteGuard<'a, T>>> {
    type Guard = RwLockWriteGuard<'a, T>;

    fn lock_or_err(self, name: &str) -> Result<Self::Guard, String> {
        self.map_err(|e| format!("Failed to acquire {name} lock: {e}"))
    }
}
