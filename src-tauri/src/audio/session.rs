//! 上次播放会话恢复
//!
//! 启动时根据配置中的 last_session 进行校验并恢复:
//! - L1: 文件存在 (Path::exists)
//! - L2: 文件大小 + 修改时间一致 (检测被替换)
//!
//! 文件不存在: 静默清除记录 (前端负责从播放列表移除)
//! 文件被替换: 视为新文件,从 0 开始播放
//! 全部通过: 恢复到 position_secs

use crate::config::manager::{LastSession, TrackSnapshot, LAST_SESSION_MAX_AGE_SECS};
use crate::AppState;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};
/// 当前 Unix 时间 (秒)
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 获取文件大小和最后修改时间 (Unix 秒)
///
/// 失败返回 None
fn get_file_metadata(path: &str) -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let size = metadata.len();
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((size, mtime))
}

/// 启动恢复结果
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeResult {
    /// 是否成功恢复
    pub resumed: bool,
    /// 给前端用于 UI 显示的曲目元数据
    pub track_path: Option<String>,
    pub track_title: Option<String>,
    pub track_artist: Option<String>,
    pub duration_secs: Option<f32>,
    /// 实际恢复到的位置 (秒)
    /// - 文件被替换时返回 0
    /// - 恢复成功时返回原 position
    pub position_secs: Option<f32>,
    /// 所在播放列表名 (用于上下首导航)
    pub playlist_name: Option<String>,
    pub track_index_in_playlist: Option<usize>,
    /// 播放队列快照 (用于恢复 player.playlist)
    /// 前端直接用此数组构造 Track[],不依赖 musicLibrary 缓存
    pub playlist_tracks: Vec<TrackSnapshot>,
    /// 文件状态描述 (用于前端日志/调试)
    pub status: String,
}

/// 校验并恢复上次播放会话
///
/// 调用流程: 前端启动时调用此命令 -> 根据返回结果决定 UI 状态和是否调用 play_track
pub fn try_resume_last_session(
    app: &AppHandle,
    state: &State<AppState>,
) -> Result<ResumeResult, String> {
    let mut config = state.config_manager.load_config()?;

    let session = match config.last_session.take() {
        None => {
            return Ok(ResumeResult {
                resumed: false,
                track_path: None,
                track_title: None,
                track_artist: None,
                duration_secs: None,
                position_secs: None,
                playlist_name: None,
                track_index_in_playlist: None,
                playlist_tracks: Vec::new(),
                status: "no_session".to_string(),
            });
        }
        Some(s) => s,
    };

    // 过期检查 (30 天)
    let now = now_secs();
    if now.saturating_sub(session.saved_at) > LAST_SESSION_MAX_AGE_SECS {
        log::info!("Last session expired (saved_at={}, now={})", session.saved_at, now);
        // 持久化清除
        let _ = state.config_manager.save_config(&config);
        return Ok(ResumeResult {
            resumed: false,
            track_path: None,
            track_title: None,
            track_artist: None,
            duration_secs: None,
            position_secs: None,
            playlist_name: None,
            track_index_in_playlist: None,
            playlist_tracks: Vec::new(),
            status: "expired".to_string(),
        });
    }

    // L1: 文件存在性校验
    if !Path::new(&session.track_path).exists() {
        log::info!(
            "Last session track not found, clearing: {}",
            session.track_path
        );
        // 静默清除记录 - 前端通过 status="not_found" 决定是否从播放列表移除
        // 但不在此处直接操作播放列表 (播放列表管理在前端 store 中)
        let _ = state.config_manager.save_config(&config);
        return Ok(ResumeResult {
            resumed: false,
            track_path: Some(session.track_path),
            track_title: Some(session.track_title),
            track_artist: Some(session.track_artist),
            duration_secs: Some(session.duration_secs),
            position_secs: None,
            playlist_name: session.playlist_name,
            track_index_in_playlist: session.track_index_in_playlist,
            // 返回 playlist_tracks 让前端能重建播放列表 (虽然当前曲目不存在)
            playlist_tracks: session.playlist_tracks,
            status: "not_found".to_string(),
        });
    }

    // L2: 文件大小 + 修改时间校验 (检测文件被替换)
    let (actual_size, actual_mtime) = if let Some(meta) = get_file_metadata(&session.track_path) { meta } else {
        // 文件存在但无法读取 metadata (权限问题等)
        // 视为不可用,清除记录
        log::warn!(
            "Failed to read file metadata for last session: {}",
            session.track_path
        );
        let _ = state.config_manager.save_config(&config);
        return Ok(ResumeResult {
            resumed: false,
            track_path: Some(session.track_path),
            track_title: Some(session.track_title),
            track_artist: Some(session.track_artist),
            duration_secs: Some(session.duration_secs),
            position_secs: None,
            playlist_name: session.playlist_name,
            track_index_in_playlist: session.track_index_in_playlist,
            playlist_tracks: session.playlist_tracks,
            status: "metadata_unreadable".to_string(),
        });
    };

    let file_replaced =
        actual_size != session.file_size || actual_mtime != session.file_mtime;

    // 实际恢复位置: 文件被替换则从 0 开始,否则用原位置
    let resume_position = if file_replaced {
        log::info!(
            "Last session file replaced (size {}->{}, mtime {}->{}), restarting from 0",
            session.file_size,
            actual_size,
            session.file_mtime,
            actual_mtime
        );
        0.0
    } else {
        session.position_secs
    };

    // 更新 session 中的位置和文件元数据,然后写回配置 (保持记录新鲜)
    let mut updated_session = session;
    updated_session.position_secs = resume_position;
    updated_session.file_size = actual_size;
    updated_session.file_mtime = actual_mtime;
    updated_session.saved_at = now;
    config.last_session = Some(updated_session.clone());
    let _ = state.config_manager.save_config(&config);

    // 调用 play_track 恢复播放
    // 根据 exclusive_mode 标志派发到对应路径
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    let path = updated_session.track_path.clone();
    let position = Some(resume_position);
    let status = if file_replaced {
        "resumed_replaced".to_string()
    } else {
        "resumed".to_string()
    };

    let play_result = if exclusive_mode {
        crate::audio::playback::play_track_exclusive(app, state, &path, position)
    } else {
        crate::audio::playback::play_track_shared(app, state, &path, position)
    };

    match play_result {
        Ok(()) => {
            // play_track_* 已开始播放,立即 pause 让 UI 处于暂停状态
            // 这样启动恢复后用户看到的是"暂停在 position"的 UI,
            // 点击播放按钮时只需调用 resume_track 即可从该位置继续
            if let Err(e) = pause_playback(state) {
                log::warn!("Failed to pause after resume (playback may still be running): {e}");
                // 不视为致命错误,仍然返回 resumed=true
            }
            Ok(ResumeResult {
                resumed: true,
                track_path: Some(updated_session.track_path),
                track_title: Some(updated_session.track_title),
                track_artist: Some(updated_session.track_artist),
                duration_secs: Some(updated_session.duration_secs),
                position_secs: Some(resume_position),
                playlist_name: updated_session.playlist_name,
                track_index_in_playlist: updated_session.track_index_in_playlist,
                playlist_tracks: updated_session.playlist_tracks,
                status,
            })
        }
        Err(e) => {
            log::error!("Failed to resume last session playback: {e}");
            // 播放失败也清除记录,避免下次启动又失败
            let mut config = state.config_manager.load_config()?;
            config.last_session = None;
            let _ = state.config_manager.save_config(&config);
            Ok(ResumeResult {
                resumed: false,
                track_path: None,
                track_title: None,
                track_artist: None,
                duration_secs: None,
                position_secs: None,
                playlist_name: None,
                track_index_in_playlist: None,
                playlist_tracks: Vec::new(),
                status: format!("playback_failed: {e}"),
            })
        }
    }
}

/// 暂停当前播放 (用于启动恢复后立即暂停)
///
/// 复用与 pause_track 命令相同的逻辑,但不通过 #[command] 包装
fn pause_playback(state: &State<AppState>) -> Result<(), String> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
            if let Some(ref wasapi) = *guard {
                wasapi.pause()?;
            } else {
                return Err("WASAPI player not initialized".to_string());
            }
        }
        #[cfg(not(windows))]
        {
            let _ = exclusive_mode;
            return Err("Exclusive mode is only supported on Windows".to_string());
        }
    } else {
        let player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        player.pause();
    }
    Ok(())
}

/// 保存上次播放会话
///
/// 调用时机: 暂停/切曲/关闭窗口/节流写入 (前端控制)
#[allow(clippy::too_many_arguments)]
pub fn save_last_session(
    state: &State<AppState>,
    track_path: String,
    track_title: String,
    track_artist: String,
    duration_secs: f32,
    position_secs: f32,
    playlist_name: Option<String>,
    track_index_in_playlist: Option<usize>,
    playlist_tracks: Vec<TrackSnapshot>,
) -> Result<(), String> {
    // L2 校验需要文件大小和修改时间
    // 如果文件不存在或无法读取,则不保存 (避免无效记录)
    let (file_size, file_mtime) = if let Some(meta) = get_file_metadata(&track_path) { meta } else {
        log::warn!("Cannot save last session: file metadata unreadable for {track_path}");
        return Ok(()); // 不视为错误,只是不保存
    };

    let now = now_secs();
    let session = LastSession {
        track_path,
        track_title,
        track_artist,
        duration_secs,
        position_secs,
        playlist_name,
        track_index_in_playlist,
        file_size,
        file_mtime,
        saved_at: now,
        playlist_tracks,
    };

    let mut config = state.config_manager.load_config()?;
    config.last_session = Some(session);
    state.config_manager.save_config(&config)
}

/// 清除上次播放会话记录 (用于文件失效场景)
pub fn clear_last_session(state: &State<AppState>) -> Result<(), String> {
    let mut config = state.config_manager.load_config()?;
    if config.last_session.is_some() {
        config.last_session = None;
        state.config_manager.save_config(&config)?;
    }
    Ok(())
}
