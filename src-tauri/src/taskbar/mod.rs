//! Windows 任务栏缩略图工具栏模块
//!
//! 提供 Windows 任务栏上的播放控制按钮（上一首、播放/暂停、下一首）

#[cfg(windows)]
mod windows_impl;

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
pub mod commands;

/// 播放状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// 正在播放
    Playing,
    /// 已暂停
    Paused,
    /// 已停止
    Stopped,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self::Stopped
    }
}
