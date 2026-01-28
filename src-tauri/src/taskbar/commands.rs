//! 任务栏相关的 Tauri 命令

use super::{PlaybackState, update_playback_state};

/// 更新任务栏播放状态
#[tauri::command]
pub fn update_taskbar_state(is_playing: bool) -> Result<(), String> {
    let state = if is_playing {
        PlaybackState::Playing
    } else {
        PlaybackState::Paused
    };
    update_playback_state(state)
}

/// 设置任务栏为停止状态
#[tauri::command]
pub fn set_taskbar_stopped() -> Result<(), String> {
    update_playback_state(PlaybackState::Stopped)
}

