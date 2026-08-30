//! 媒体相关的 Tauri 命令
//!
//! 包含文件系统操作和元数据获取命令。
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
use super::netease;
use crate::AppState;
use tauri::{State, command};

/// 读取指定目录中的子目录列表
#[command]
pub fn read_directory(path: String) -> Result<Vec<String>, AppError> {
    read_dir(&path)
}

/// 获取指定目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_audio_files(path: String) -> Result<Playlist, AppError> {
    get_audio_files_from_dir(&path)
}

/// 获取多个目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_all_audio_files(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Vec<Playlist>, AppError> {
    let config = state.config_manager.load_config()?;
    get_all_audio_files_from_dirs(&paths, &config)
}

/// 检查文件是否存在
#[command]
pub fn check_file_exists(path: String) -> Result<bool, AppError> {
    Ok(check_file_exists_internal(&path))
}

/// 读取歌词文件内容
#[command]
pub fn read_lyrics_file(path: String) -> Result<String, AppError> {
    read_lyrics_file_internal(&path)
}

/// 写入歌词文件内容
#[command]
pub fn write_lyrics_file(path: String, content: String) -> Result<(), AppError> {
    write_lyrics_file_internal(&path, &content)
}

/// 获取音轨的元数据信息（轻量）
#[command]
pub fn get_track_metadata(path: String) -> Result<TrackMetadata, AppError> {
    get_track_metadata_internal(&path)
}

/// 批量获取多个音轨的元数据信息
#[command]
pub fn get_tracks_metadata_batch(paths: Vec<String>) -> Vec<TrackMetadata> {
    paths
        .into_iter()
        .filter_map(|path| get_track_metadata_internal(&path).ok())
        .collect()
}

/// 按需提取并返回音轨封面缓存路径
#[command]
pub fn get_track_cover_path(path: String) -> Result<Option<String>, AppError> {
    get_track_cover_path_internal(&path)
}

/// 搜索网易云音乐歌曲
#[command]
pub async fn netease_search_songs(
    keyword: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<netease::SearchSongResult>, AppError> {
    netease::search_songs(&keyword, limit.unwrap_or(10), offset.unwrap_or(0)).await
}

/// 获取网易云音乐歌词
#[command]
pub async fn netease_get_lyrics(song_id: String) -> Result<netease::LyricsData, AppError> {
    netease::get_lyrics(&song_id).await
}

/// 提取音频文件的封面并保存到指定路径
#[command]
pub fn extract_cover(audio_path: String, output_path: String) -> Result<String, AppError> {
    extract_cover_internal(&audio_path, &output_path)
}

/// 清理封面缓存
#[command]
pub fn clean_cover_cache_command(max_cache_size_mb: Option<u64>) -> Result<usize, AppError> {
    clean_cover_cache(max_cache_size_mb)
}

/// 设置封面缓存路径
#[command]
pub fn set_cover_cache_path_command(path: Option<String>) -> Result<(), AppError> {
    set_cover_cache_path(path)
}

/// 清除元数据缓存
#[command]
pub fn clear_metadata_cache_command() -> Result<(), AppError> {
    clear_metadata_cache()
}

/// 获取元数据缓存统计信息
#[command]
pub fn get_metadata_cache_stats_command() -> (usize, u64) {
    get_metadata_cache_stats()
}

/// 手动刷新元数据缓存到磁盘
#[command]
pub fn flush_metadata_cache_command() -> Result<(), AppError> {
    flush_metadata_cache()
}

/// 获取系统临时目录路径
#[command]
pub fn get_temp_dir_command() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}
