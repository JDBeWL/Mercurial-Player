//! 文件系统操作模块
//!
//! 提供目录读取、文件检查等功能。

use super::metadata::{Playlist, flush_metadata_cache, get_track_metadata_internal};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::security::{has_allowed_extension, is_sensitive_path};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

/// 支持的音频文件扩展名
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "aac"];

/// 允许读写的歌词文件扩展名
const LYRICS_EXTENSIONS: [&str; 3] = ["lrc", "ass", "srt"];

/// 校验歌词文件路径：仅允许歌词扩展名，且不允许访问敏感目录
fn validate_lyrics_path(path: &str) -> Result<(), AppError> {
    if !has_allowed_extension(path, &LYRICS_EXTENSIONS) {
        return Err("仅允许读写歌词文件（.lrc/.ass/.srt）".to_string().into());
    }
    if is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }
    Ok(())
}

/// 读取指定目录中的子目录列表
pub fn read_dir(path: &str) -> Result<Vec<String>, AppError> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string().into());
    }

    fs::read_dir(dir)
        .map_err(|e| AppError::msg(e.to_string()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path().to_str().map(String::from))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Failed to convert paths".to_string().into())
}

/// 获取指定目录中的所有音频文件，并创建播放列表
pub fn get_audio_files_from_dir(path: &str) -> Result<Playlist, AppError> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string().into());
    }

    let audio_files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    let playlist_name = dir.file_name().map_or_else(
        || "Unknown".to_string(),
        |s| s.to_string_lossy().to_string(),
    );

    Ok(Playlist {
        name: playlist_name,
        files: tracks,
    })
}

/// 获取多个目录中的所有音频文件，并创建播放列表
pub fn get_all_audio_files_from_dirs(
    paths: &[String],
    config: &AppConfig,
) -> Result<Vec<Playlist>, AppError> {
    let mut all_playlists: Vec<Playlist> = Vec::new();

    for path in paths {
        let dir = Path::new(path);
        if !dir.is_dir() {
            log::warn!("Provided path is not a directory: {path}");
            continue;
        }

        if config.directory_scan.enable_subdirectory_scan && config.playlist.folder_based_playlists
        {
            let playlists =
                scan_with_folder_playlists(dir, config.directory_scan.max_depth as usize);
            all_playlists.extend(playlists);
        } else if let Some(playlist) = scan_single_playlist(dir) {
            all_playlists.push(playlist);
        }
    }

    // 扫描完成后，批量保存内存缓存到磁盘
    if let Err(e) = flush_metadata_cache() {
        log::warn!("批量保存元数据缓存失败: {e}");
    } else {
        log::info!("元数据缓存已批量保存到磁盘");
    }

    Ok(all_playlists)
}

/// 扫描目录并按文件夹创建播放列表（轻量模式，不读取封面）
fn scan_with_folder_playlists(dir: &Path, max_depth: usize) -> Vec<Playlist> {
    let audio_files: Vec<_> = WalkDir::new(dir)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks_with_folders: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let parent_dir = entry.path().parent().unwrap_or(dir);
            let folder_name = parent_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Unknown")
                .to_string();

            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map(|metadata| (folder_name, metadata))
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    let mut folder_playlists: HashMap<String, Playlist> = HashMap::new();
    for (folder_name, metadata) in tracks_with_folders {
        folder_playlists
            .entry(folder_name.clone())
            .or_insert_with(|| Playlist::new(folder_name))
            .add_track(metadata);
    }

    folder_playlists
        .into_values()
        .filter(|p| !p.is_empty())
        .collect()
}

/// 扫描目录创建单个播放列表（完整模式，包含封面）
fn scan_single_playlist(dir: &Path) -> Option<Playlist> {
    let playlist_name = dir.file_name().map_or_else(
        || "Unknown".to_string(),
        |s| s.to_string_lossy().to_string(),
    );

    let audio_files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    if tracks.is_empty() {
        None
    } else {
        Some(Playlist {
            name: playlist_name,
            files: tracks,
        })
    }
}

/// 检查文件是否存在
#[must_use]
pub fn check_file_exists_internal(path: &str) -> bool {
    if Path::new(path).exists() {
        return true;
    }

    // 尝试另一种路径分隔符格式
    let alt_path = if path.contains('/') {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    };

    alt_path != path && Path::new(&alt_path).exists()
}

/// 读取歌词文件内容
pub fn read_lyrics_file_internal(path: &str) -> Result<String, AppError> {
    validate_lyrics_path(path)?;
    fs::read_to_string(path).map_err(|e| e.to_string().into())
}

/// 写入歌词文件内容
pub fn write_lyrics_file_internal(path: &str, content: &str) -> Result<(), AppError> {
    validate_lyrics_path(path)?;

    // 确保父目录存在
    if let Some(parent) = Path::new(path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    fs::write(path, content).map_err(|e| format!("Failed to write file: {e}").into())
}

/// 检查是否为音频文件
fn is_audio_file(entry: &DirEntry) -> bool {
    entry
        .path()
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}
