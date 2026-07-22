//! Windows 任务栏缩略图工具栏模块
//!
//! 提供 Windows 任务栏上的播放控制按钮（上一首、播放/暂停、下一首）

#[cfg(windows)]
mod windows_impl;

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(windows)]
pub mod commands;

#[cfg(windows)]
pub mod desktop_lyrics;

/// 任务栏播放状态枚举
///
/// 注意: 与 wasapi::PlaybackState 不同,此枚举仅用于任务栏按钮状态显示
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum TaskbarPlaybackState {
    /// 正在播放
    Playing,
    /// 已暂停
    Paused,
    /// 已停止
    #[default]
    Stopped,
}

