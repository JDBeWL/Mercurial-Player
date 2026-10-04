//! 媒体相关的 Tauri 命令：文件系统操作与元数据获取。
use crate::error::AppError;

use super::filesystem::{
    check_file_exists_internal, get_all_audio_files_from_dirs, get_audio_files_from_dir, read_dir,
    read_lyrics_file_internal, write_lyrics_file_internal,
};
use super::metadata::{
    Playlist, TrackMetadata, clean_cover_cache, clear_metadata_cache, extract_cover_internal,
    flush_metadata_cache, get_metadata_cache_stats, get_track_cover_path_internal,
    get_track_metadata_internal, set_cover_cache_path,
};
use crate::AppState;
use crate::security::is_within_music_dirs;
use tauri::{State, command};

/// 扫描前的白名单门禁，理由见 [`is_within_music_dirs`]
fn ensure_scannable(music_dirs: &[String], path: &str) -> Result<(), AppError> {
    if is_within_music_dirs(path, music_dirs) {
        Ok(())
    } else {
        Err(AppError::msg("安全限制：该目录不在已添加的音乐目录内"))
    }
}

#[command]
pub fn read_directory(state: State<'_, AppState>, path: String) -> Result<Vec<String>, AppError> {
    let music_dirs = state.config_manager.load_config()?.music_directories;
    ensure_scannable(&music_dirs, &path)?;
    read_dir(&path)
}

/// 扫描是 CPU + IPC 密集型，放到阻塞线程池执行
#[command]
pub async fn get_audio_files(
    state: State<'_, AppState>,
    path: String,
) -> Result<Playlist, AppError> {
    let music_dirs = state.config_manager.load_config()?.music_directories;
    ensure_scannable(&music_dirs, &path)?;
    tauri::async_runtime::spawn_blocking(move || get_audio_files_from_dir(&path))
        .await
        .map_err(|e| AppError::msg(format!("扫描任务异常退出: {e}")))?
}

#[command]
pub async fn get_all_audio_files(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<Playlist>, AppError> {
    let config = state.config_manager.load_config()?;
    for path in &paths {
        ensure_scannable(&config.music_directories, path)?;
    }
    tauri::async_runtime::spawn_blocking(move || get_all_audio_files_from_dirs(&paths, &config))
        .await
        .map_err(|e| AppError::msg(format!("批量扫描任务异常退出: {e}")))?
}

#[command]
pub fn check_file_exists(path: String) -> Result<bool, AppError> {
    Ok(check_file_exists_internal(&path))
}

#[command]
pub fn read_lyrics_file(path: String) -> Result<String, AppError> {
    read_lyrics_file_internal(&path)
}

#[command]
pub fn write_lyrics_file(path: String, content: String) -> Result<(), AppError> {
    write_lyrics_file_internal(&path, &content)
}

#[command]
pub fn get_track_metadata(path: String) -> Result<TrackMetadata, AppError> {
    get_track_metadata_internal(&path)
}

/// 批量获取多个音轨的元数据信息；rayon 并行的理由见 `filesystem::scan_content_tree`
#[command]
pub async fn get_tracks_metadata_batch(paths: Vec<String>) -> Vec<TrackMetadata> {
    tauri::async_runtime::spawn_blocking(move || {
        use rayon::prelude::*;
        paths
            .par_iter()
            .filter_map(|path| {
                get_track_metadata_internal(path)
                    .map_err(|e| log::warn!("批量元数据提取失败 {path}: {e}"))
                    .ok()
            })
            .collect()
    })
    .await
    .unwrap_or_else(|e| {
        log::error!("批量元数据提取任务失败（返回空列表）: {e}");
        Vec::new()
    })
}

#[command]
pub fn get_track_cover_path(path: String) -> Result<Option<String>, AppError> {
    get_track_cover_path_internal(&path)
}

#[command]
pub fn extract_cover(audio_path: String, output_path: String) -> Result<String, AppError> {
    extract_cover_internal(&audio_path, &output_path)
}

#[command]
pub fn clean_cover_cache_command(max_cache_size_mb: Option<u64>) -> Result<usize, AppError> {
    clean_cover_cache(max_cache_size_mb)
}

#[command]
pub fn set_cover_cache_path_command(path: Option<String>) -> Result<(), AppError> {
    set_cover_cache_path(path)
}

#[command]
pub fn clear_metadata_cache_command() -> Result<(), AppError> {
    clear_metadata_cache()
}

#[command]
pub fn get_metadata_cache_stats_command() -> (usize, u64) {
    get_metadata_cache_stats()
}

#[command]
pub fn flush_metadata_cache_command() -> Result<(), AppError> {
    flush_metadata_cache()
}

/// 系统临时目录路径（未设置封面缓存路径时，设置页展示的就是它）
#[command]
pub fn get_temp_dir_command() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}

/// 调起系统目录选择器（Android SAF），仅移动端生效；桌面端为 no-op
#[command]
pub fn saf_request_pick() -> Result<(), AppError> {
    crate::android::saf::request_pick_directory()
}

/// 获取已保存的音乐目录树 URI（Android SAF；桌面端返回 None）
#[command]
pub fn saf_get_saved_tree() -> Result<Option<String>, AppError> {
    crate::android::saf::get_saved_tree_uri()
}

/// 获取 SAF 授权状态（URI + 授权版本号 + 显示名）
///
/// 前端轮询 `version` 判断系统选择器是否已返回：重新授权同一目录时 URI 不变，只有 version 递增。
#[command]
pub fn saf_get_pick_state() -> Result<crate::android::saf::SafPickState, AppError> {
    crate::android::saf::get_pick_state()
}

/// 移除某个 SAF 目录时调用：释放该树的持久授权（并清掉残留记录，避免影响重新授权）
#[command]
pub fn saf_clear_saved_tree(uri: String) -> Result<(), AppError> {
    crate::android::saf::clear_saved_tree(&uri)
}
