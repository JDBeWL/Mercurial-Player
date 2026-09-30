//! 播放事件的发送：曲目结束与进度上报。
//!
//! 音频回调线程与分析线程都要发这两类事件，抽在这里避免互相引用。

use super::queue;
use crate::AppState;
use tauri::{AppHandle, Emitter, Manager};

/// 音轨结束事件
#[derive(Debug, serde::Serialize, Clone)]
pub struct TrackEndedEvent {}

#[inline]
pub(super) fn emit_track_ended(
    app: &AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("track-ended", TrackEndedEvent {})?;
    // Android 进入后台后 WebView 的 JS 会被节流/冻结，`track-ended` 可能无人处理，
    // 交由 Rust 侧队列接管推进（桌面端内部直接返回）。
    queue::handle_track_ended(app, &app.state::<AppState>());
    Ok(())
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPositionEvent {
    pub position: f32, // 秒
}

pub(super) fn emit_playback_position(
    app: &AppHandle,
    position: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("playback-position", PlaybackPositionEvent { position })?;
    // 通知栏/MediaSession 的进度基准（仅内存原子量，无额外开销）
    queue::note_position(position);
    Ok(())
}
