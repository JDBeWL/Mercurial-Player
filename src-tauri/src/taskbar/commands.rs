//! 任务栏相关的 Tauri 命令
use crate::error::AppError;

use super::{TaskbarPlaybackState, update_playback_state};

/// 更新任务栏播放状态
#[tauri::command]
pub fn update_taskbar_state(is_playing: bool) -> Result<(), AppError> {
    let state = if is_playing {
        TaskbarPlaybackState::Playing
    } else {
        TaskbarPlaybackState::Paused
    };
    update_playback_state(state)
}

/// 设置任务栏为停止状态
#[tauri::command]
pub fn set_taskbar_stopped() -> Result<(), AppError> {
    update_playback_state(TaskbarPlaybackState::Stopped)
}
