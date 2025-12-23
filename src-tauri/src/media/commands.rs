//! 媒体相关的 Tauri 命令
//!
//! 包含文件系统操作和元数据获取命令。

use super::filesystem::{
    check_file_exists_internal, get_all_audio_files_from_dirs, get_audio_files_from_dir, read_dir,
    read_lyrics_file_internal,
};
use super::metadata::{Playlist, TrackMetadata, get_track_metadata_internal};
use crate::AppState;
use tauri::{State, command};

/// 读取指定目录中的子目录列表
#[command]
pub fn read_directory(path: String) -> Result<Vec<String>, String> {
    read_dir(&path)
}

/// 获取指定目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_audio_files(path: String) -> Result<Playlist, String> {
    get_audio_files_from_dir(&path)
}

/// 获取多个目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_all_audio_files(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Vec<Playlist>, String> {
    let config = state.config_manager.load_config()?;
    get_all_audio_files_from_dirs(&paths, &config)
}

/// 检查文件是否存在
#[command]
pub fn check_file_exists(path: String) -> Result<bool, String> {
    Ok(check_file_exists_internal(&path))
}

/// 读取歌词文件内容
#[command]
pub fn read_lyrics_file(path: String) -> Result<String, String> {
    read_lyrics_file_internal(&path)
}

/// 获取音轨的元数据信息
#[command]
pub fn get_track_metadata(path: String) -> Result<TrackMetadata, String> {
    get_track_metadata_internal(&path)
}

/// 批量获取多个音轨的元数据信息
/// 返回成功获取的元数据列表，失败的文件会被跳过
#[command]
pub fn get_tracks_metadata_batch(paths: Vec<String>) -> Vec<TrackMetadata> {
    paths
        .into_iter()
        .filter_map(|path| get_track_metadata_internal(&path).ok())
        .collect()
}
