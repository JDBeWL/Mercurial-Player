//! 音频模块：解码、播放（共享 rodio / 平台独占）、设备管理与频谱。
//!
//! 锁序约定（避免死锁）：`AudioOutputState` 各锁按下面的全局顺序获取，需要同时持有多个锁
//! 的代码必须遵守此顺序，并尽量缩小临界区、避免在持锁期间执行 IPC/文件 IO：
//!
//!   sink → output_stream → target_volume → exclusive_mode → wasapi_player
//!        → current_device_name → current_path
//!
//! 采样环 `SampleRing`（频谱、AAudio）与 `SpscSampleRing`（WASAPI 独占）是无锁 SPSC，
//! 不参与锁序：渲染/解码/宿主线程只经原子计数访问，持锁期间操作它们不构成嵌套。
//! 可视化数据(spectrum_data)与 device_monitor/equalizer 相互独立，不与上述锁同栈嵌套。
//!
//! 核心路径（音频线程等）用 `lock_or_log!`：锁中毒自动恢复，不中断播放；
//! 命令边界用 [`LockOrErr`]：把获取锁失败转成描述性错误返回给前端。

/// 共享模式播放/恢复时的淡入时长(毫秒)。
/// 播放起点没有对应的淡出,用稍长淡入掩盖可能的爆音。
pub(crate) const FADE_IN_MS: u64 = 80;
/// seek 时的淡入时长(毫秒),位置连续故用更短淡入。
pub(crate) const FADE_IN_ON_SEEK_MS: u64 = 50;

/// 独占/直出播放器的状态机
/// 提到平台无关位置：Windows 的 WASAPI 与 Android 的 AAudio 共用，命令层与解码推送按它判断。
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

pub mod commands;

#[cfg(any(windows, target_os = "android"))]
pub mod decode_push;
pub mod decoder;
pub mod dsp;

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

// 重新导出常用类型
pub use decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
pub use device::AudioDeviceInfo;
pub use device_monitor::{DeviceChangeEvent, DeviceMonitor};
pub use playback::{EqProcessor, VisualizationSource};
pub use queue::{PlaybackQueue, RepeatMode};
pub use sample_ring::SampleRing;

#[cfg(target_os = "android")]
pub use aaudio::AaudioExclusivePlayer;
#[cfg(windows)]
pub use wasapi::WasapiExclusivePlayback;

/// 按当前的系统默认输出设备重建共享模式输出（Android）
/// cpal 的流不会随默认设备迁移也不自愈，不重建则拔掉 DAC 后共享播放一直没声；创建失败时
/// 原样保留旧状态。调用方需保证此刻没有正在播的音频：替换 sink 会丢掉当前播放队列。
#[cfg(target_os = "android")]
pub fn rebuild_shared_sink(state: &crate::AppState) -> Result<(), crate::error::AppError> {
    use rodio::stream::DeviceSinkBuilder;

    let output = &state.player.output;

    // 音量单独读（不与其它锁嵌套；全局锁序里 target_volume 排在 sink 之后，
    // 先读后锁是安全的顺序，反过来持锁读才是违规）
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

    // 按锁序 sink → output_stream 依次替换（两个临界区不嵌套）。
    // 先换 sink 再换 output_stream：旧 sink 的 drop 关掉旧 cpal 流时，
    // 旧播放器已经释放掉了。
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

/// 统一的锁获取错误映射：把 PoisonError/TryLockError 转成带锁名的描述性错误，免掉各命令里
/// 重复的 `.map_err(...)`。命令边界用它，核心路径用 `lock_or_log!`（中毒不中断播放）。
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
