//! 音频模块
//!
//! 提供音频播放、解码、设备管理等功能。

pub mod commands;

// ============================================================================
// 锁序约定(避免死锁)
// ============================================================================
//
// `AudioOutputState` 中各锁按以下全局顺序获取,任何需要同时持有多个锁的代码
// 都必须按此顺序,且尽量缩小临界区、避免在持锁期间执行 IPC/文件 IO:
//
//   sink → output_stream → target_volume → exclusive_mode → wasapi_player
//        → current_device_name → current_path
//
// 可视化数据(spectrum_data)与 device_monitor/equalizer 相互独立,
// 不与上述锁同栈嵌套。
//
// 注意:WASAPI 独占模式的采样缓冲(SpscSampleRing)是无锁 SPSC 环形缓冲,
// 不参与上述锁序——音频渲染线程/解码线程/宿主线程通过原子计数访问,
// 持有 wasapi_player 锁期间操作它不再构成锁序嵌套。
//
// 两个既有约定:
// - 核心路径(音频线程等)用 `lock_or_log!`:锁中毒自动恢复,不中断播放;
// - 命令边界(返回错误给前端的 Tauri command)用 [`LockOrErr`]:把获取锁的
//   失败转换为描述性错误。

// ============================================================================
// 共享常量
// ============================================================================

/// 共享模式播放/恢复时的淡入时长(毫秒)。
/// 播放起点没有对应的淡出,用稍长淡入掩盖可能的爆音。
pub(crate) const FADE_IN_MS: u64 = 80;
/// seek 时的淡入时长(毫秒),位置连续故用更短淡入。
pub(crate) const FADE_IN_ON_SEEK_MS: u64 = 50;

/// 独占/直出播放器的状态机
///
/// 原来只定义在 WASAPI 独占模块里；Android 的 AAudio 独占通道要复用同一套
/// 状态（命令层与解码推送线程都按它判断），因此提到平台无关的位置。
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
///
/// 共享输出（`rodio::Player` + `MixerDeviceSink`）是**启动那一刻**按当时的默认设备开的。
/// Android 的默认输出设备会随 USB 拔插变化，而 cpal 的流不会跟着迁移、也不会自愈 ——
/// 不重建的话，拔掉 DAC 之后所有共享模式播放都还在往一条已经死掉的流里灌数据，
/// 现象和独占那条路一样：**没有声音，重启应用才恢复**。
///
/// 调用方需保证此刻没有正在播的音频：本函数会替换掉 sink，当前播放队列随之丢失。
/// 目前唯一的调用点是 [`aaudio::on_audio_route_changed`]，它已经先暂停了播放。
///
/// 新输出建好之前不碰旧状态 —— 创建失败时原样保留，至少还留着"重启可恢复"的退路。
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

/// 统一的锁获取错误映射:把 PoisonError/TryLockError 转成带锁名称的描述性错误,
/// 替代各命令里逐行重复的 `.map_err(|e| format!("Failed to acquire ... lock: {e}"))`。
///
/// 与 `lock_or_log!` 的区别:本 trait 用于命令边界(把错误返回给前端),
/// `lock_or_log!` 用于核心路径(中毒自动恢复,不中断)。
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
