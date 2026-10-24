//! 上次播放会话恢复：启动时校验配置里的 `last_session`。
//!
//! 文件不存在则静默清除记录（播放列表由前端维护）；大小或修改时间变化视为被替换，从 0 开始；全部通过则恢复到 `position_secs`。

use crate::AppState;
use crate::config::manager::{LAST_SESSION_MAX_AGE_SECS, LastSession, TrackSnapshot};
use crate::error::AppError;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, State};

/// `save_last_session` 写盘节流：数千首的播放列表每次 pause/切歌都全量序列化 + 落盘，用它兜住开销。
/// 代价是窗口内退出会丢掉最后一次更新，下次启动最多回退这么久。
const SAVE_THROTTLE_DURATION: Duration = Duration::from_secs(5);

static LAST_SAVE_TIME: Mutex<Option<Instant>> = Mutex::new(None);
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn get_file_metadata(path: &str) -> Option<(u64, u64)> {
    // content URI 没有可 stat 的路径，只能从 SAF fd 上取
    let metadata = if crate::android::saf::is_content_uri(path) {
        crate::android::saf::open_media_file(path)
            .ok()?
            .metadata()
            .ok()?
    } else {
        std::fs::metadata(path).ok()?
    };
    let size = metadata.len();
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((size, mtime))
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeResult {
    pub resumed: bool,
    pub track_path: Option<String>,
    pub track_title: Option<String>,
    pub track_artist: Option<String>,
    pub duration_secs: Option<f32>,
    /// 实际恢复到的位置 (秒)：文件被替换时返回 0，恢复成功时返回原 position
    pub position_secs: Option<f32>,
    pub playlist_name: Option<String>,
    pub track_index_in_playlist: Option<usize>,
    /// 播放队列快照：前端直接构造 Track[] 恢复 `player.playlist`，不依赖 musicLibrary 缓存
    pub playlist_tracks: Vec<TrackSnapshot>,
    /// 上次会话的循环模式（前端 RepeatMode：none / track / list）。
    /// None = 没有可用的会话记录，前端保持当前默认值，不要覆盖
    pub repeat_mode: Option<String>,
    /// 上次会话是否随机播放；None 的语义同 `repeat_mode`
    pub is_shuffle: Option<bool>,
    pub status: String,
}

/// 校验并恢复上次播放会话：内部完成 `play_track_*` 派发并暂停在 `position`，前端只按 status 决定 UI。
pub async fn try_resume_last_session(
    app: &AppHandle,
    state: &State<'_, AppState>,
) -> Result<ResumeResult, AppError> {
    // 取出会话记录：「读-改-写」在写锁内一次完成，避免与 `save_last_session` 并发丢更新；各提前返回不再写回。
    let session = match state
        .config_manager
        .update_config(|config| config.last_session.take())?
    {
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
                repeat_mode: None,
                is_shuffle: None,
                status: "no_session".to_string(),
            });
        }
        Some(s) => s,
    };

    let now = now_secs();
    if now.saturating_sub(session.saved_at) > LAST_SESSION_MAX_AGE_SECS {
        log::info!(
            "Last session expired (saved_at={}, now={})",
            session.saved_at,
            now
        );
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
            repeat_mode: None,
            is_shuffle: None,
            status: "expired".to_string(),
        });
    }

    // 文件存在性校验：SAF content URI 视为存在，交由打开时校验
    if !crate::android::saf::is_content_uri(&session.track_path)
        && !Path::new(&session.track_path).exists()
    {
        log::info!(
            "Last session track not found, clearing: {}",
            session.track_path
        );
        // 只回 status="not_found"，是否从播放列表移除由前端 store 决定
        return Ok(ResumeResult {
            resumed: false,
            track_path: Some(session.track_path),
            track_title: Some(session.track_title),
            track_artist: Some(session.track_artist),
            duration_secs: Some(session.duration_secs),
            position_secs: None,
            playlist_name: session.playlist_name,
            track_index_in_playlist: session.track_index_in_playlist,
            // 仍返回 playlist_tracks，让前端在曲目缺失时也能重建播放列表
            playlist_tracks: session.playlist_tracks,
            // 曲目没了但会话是完整的：播放模式照样还给前端，别让用户重启一次就要重设
            repeat_mode: Some(session.repeat_mode),
            is_shuffle: Some(session.is_shuffle),
            status: "not_found".to_string(),
        });
    }

    let (actual_size, actual_mtime) = if let Some(meta) = get_file_metadata(&session.track_path) {
        meta
    } else {
        // 文件存在但无法读取 metadata (权限问题等)，视为不可用
        log::warn!(
            "Failed to read file metadata for last session: {}",
            session.track_path
        );
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
            repeat_mode: Some(session.repeat_mode),
            is_shuffle: Some(session.is_shuffle),
            status: "metadata_unreadable".to_string(),
        });
    };

    let file_replaced = actual_size != session.file_size || actual_mtime != session.file_mtime;

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

    // 写回刷新后的记录：独立临界区，不与下方的 .await 重叠
    let mut updated_session = session;
    updated_session.position_secs = resume_position;
    updated_session.file_size = actual_size;
    updated_session.file_mtime = actual_mtime;
    updated_session.saved_at = now;
    if let Err(e) = state.config_manager.update_config(|config| {
        config.last_session = Some(updated_session.clone());
    }) {
        log::warn!("写回归档位置失败，下次启动可能从错误位置续播: {e}");
    }

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
        crate::audio::playback::play_track_exclusive(app, state, &path, position, true).await
    } else {
        crate::audio::playback::play_track_shared(app, state, &path, position)
    };

    match play_result {
        Ok(()) => {
            // play_track_* 已在播放，立即 pause：恢复后 UI 停在「暂停于 position」，点播放只需 resume 继续。
            if let Err(e) = crate::audio::commands::pause_any(app, state) {
                log::warn!("Failed to pause after resume (playback may still be running): {e}");
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
                repeat_mode: Some(updated_session.repeat_mode),
                is_shuffle: Some(updated_session.is_shuffle),
                status,
            })
        }
        Err(e) => {
            log::error!("Failed to resume last session playback: {e}");
            // 播放失败也清除记录,避免下次启动又失败
            if let Err(e) = state.config_manager.update_config(|config| {
                config.last_session = None;
            }) {
                log::warn!("清除失效的续播记录失败，下次启动仍会重试该曲目: {e}");
            }
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
                repeat_mode: None,
                is_shuffle: None,
                status: format!("playback_failed: {e}"),
            })
        }
    }
}

/// 前端 `RepeatMode` 的合法取值；不在白名单内的一律落成 none，避免脏值写进配置文件
fn normalize_repeat_mode(mode: Option<&str>) -> String {
    match mode {
        Some("list") => "list".to_string(),
        Some("track") => "track".to_string(),
        _ => "none".to_string(),
    }
}

/// 保存上次播放会话（由前端在暂停/切曲/关窗/节流时调用）。
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
    repeat_mode: Option<String>,
    is_shuffle: Option<bool>,
) -> Result<(), AppError> {
    // 节流窗口见 SAVE_THROTTLE_DURATION；检查与更新时间在同一把锁内完成，保证原子性
    {
        let mut guard = LAST_SAVE_TIME
            .lock()
            .map_err(|e| format!("Failed to acquire LAST_SAVE_TIME lock: {e}"))?;
        if let Some(last) = *guard {
            let elapsed = last.elapsed();
            if elapsed < SAVE_THROTTLE_DURATION {
                log::debug!(
                    "save_last_session throttled: elapsed {elapsed:?} < {SAVE_THROTTLE_DURATION:?}, skipping write"
                );
                return Ok(());
            }
        }
        *guard = Some(Instant::now());
    }

    // 记录要带文件大小与修改时间做替换校验；读不到就不保存，避免写入无效记录
    let (file_size, file_mtime) = if let Some(meta) = get_file_metadata(&track_path) {
        meta
    } else {
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
        repeat_mode: normalize_repeat_mode(repeat_mode.as_deref()),
        is_shuffle: is_shuffle.unwrap_or(false),
    };

    state
        .config_manager
        .update_config(|config| config.last_session = Some(session))
}

/// 清除上次播放会话记录。
pub fn clear_last_session(state: &State<AppState>) -> Result<(), AppError> {
    state.config_manager.update_config(|config| {
        if config.last_session.is_some() {
            config.last_session = None;
        }
    })
}
