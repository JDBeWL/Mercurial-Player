//! 文件系统操作模块
//!
//! 提供目录读取、文件检查等功能。

use super::metadata::{get_track_metadata_internal, Playlist};
use crate::config::AppConfig;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

/// 支持的音频文件扩展名
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "aac"];

/// 读取指定目录中的子目录列表
pub fn read_dir(path: &str) -> Result<Vec<String>, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string());
    }

    fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path().to_str().map(String::from))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Failed to convert paths".to_string())
}

/// 获取指定目录中的所有音频文件，并创建播放列表
pub fn get_audio_files_from_dir(path: &str) -> Result<Playlist, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string());
    }

    let audio_files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| is_audio_file(e))
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map_err(|e| eprintln!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    let playlist_name = dir
        .file_name()
        .map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

    Ok(Playlist {
        name: playlist_name,
        files: tracks,
    })
}

/// 获取多个目录中的所有音频文件，并创建播放列表
pub fn get_all_audio_files_from_dirs(paths: &[String], config: &AppConfig) -> Result<Vec<Playlist>, String> {
    let mut all_playlists: Vec<Playlist> = Vec::new();

    for path in paths {
        let dir = Path::new(path);
        if !dir.is_dir() {
            eprintln!("Provided path is not a directory: {path}");
            continue;
        }

        if config.directory_scan.enable_subdirectory_scan && config.playlist.folder_based_playlists {
            let playlists = scan_with_folder_playlists(dir, config.directory_scan.max_depth as usize);
            all_playlists.extend(playlists);
        } else if let Some(playlist) = scan_single_playlist(dir) {
            all_playlists.push(playlist);
        }
    }

    Ok(all_playlists)
}

/// 扫描目录并按文件夹创建播放列表
fn scan_with_folder_playlists(dir: &Path, max_depth: usize) -> Vec<Playlist> {
    let audio_files: Vec<_> = WalkDir::new(dir)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| is_audio_file(e))
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

/// 扫描目录创建单个播放列表
fn scan_single_playlist(dir: &Path) -> Option<Playlist> {
    let playlist_name = dir
        .file_name()
        .map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

    let audio_files: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| is_audio_file(e))
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path).ok()
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
pub fn read_lyrics_file_internal(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| e.to_string())
}

/// 检查是否为音频文件
fn is_audio_file(entry: &DirEntry) -> bool {
    entry
        .path()
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}
