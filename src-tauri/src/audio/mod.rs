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

#[cfg(windows)]
pub mod decode_push;
pub mod decoder;
pub mod dsp;

pub mod device;
pub mod device_monitor;
pub mod playback;
pub mod session;
pub mod spectrum;

#[cfg(windows)]
pub mod wasapi;

// 重新导出常用类型
pub use decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
pub use device::AudioDeviceInfo;
pub use device_monitor::{DeviceChangeEvent, DeviceMonitor};
pub use playback::VisualizationSource;

#[cfg(windows)]
pub use wasapi::{PlaybackState, WasapiExclusivePlayback};

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
