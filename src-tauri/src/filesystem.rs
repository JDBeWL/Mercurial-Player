//! 文件系统相关的 Tauri 命令
//! 
//! 这个模块包含所有与文件系统操作相关的功能，包括目录读取、文件检查等

use super::metadata::Playlist;
use std::fs;
use std::path::Path;
use rayon::prelude::*;
use tauri::{command, State};
use walkdir::{DirEntry, WalkDir};
use crate::AppState;

/// 读取指定目录中的子目录列表
#[command]
pub fn read_directory(path: String) -> Result<Vec<String>, String> {
    let dir = Path::new(&path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string());
    }

    let result = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.path().to_str().map(String::from))
        .collect::<Vec<String>>();
    Ok(result)
}

/// 获取指定目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_audio_files(path: String) -> Result<Playlist, String> {
    let dir = Path::new(&path);
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
            match crate::metadata::get_track_metadata(file_path) {
                Ok(metadata) => Some(metadata),
                Err(e) => {
                    eprintln!("Failed to get metadata for file: {}", e);
                    None
                }
            }
        })
        .collect();

    let playlist_name = dir.file_name().map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

    Ok(Playlist {
        name: playlist_name,
        files: tracks,
    })
}

/// 获取多个目录中的所有音频文件，并创建播放列表
#[command]
pub fn get_all_audio_files(state: State<AppState>, paths: Vec<String>) -> Result<Vec<Playlist>, String> {
    let mut all_playlists: Vec<Playlist> = Vec::new();
    let config = state.config_manager.load_config()?;

    for path in paths {
        let dir = Path::new(&path);
        if !dir.is_dir() {
            eprintln!("Provided path is not a directory: {}", path);
            continue;
        }

        if config.directory_scan.enable_subdirectory_scan && config.playlist.folder_based_playlists {
            let max_depth = config.directory_scan.max_depth as usize;
            
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
                    let folder_name = parent_dir.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("Unknown");
                    
                    let file_path = entry.path().to_string_lossy().to_string();
                    match crate::metadata::get_track_metadata(file_path) {
                        Ok(metadata) => Some((folder_name.to_string(), metadata)),
                        Err(e) => {
                            eprintln!("Failed to get metadata for file: {}", e);
                            None
                        }
                    }
                })
                .collect();

            let mut folder_playlists = std::collections::HashMap::new();
            for (folder_name, metadata) in tracks_with_folders {
                let playlist = folder_playlists.entry(folder_name.clone())
                    .or_insert_with(|| Playlist::new(folder_name));
                playlist.add_track(metadata);
            }
            
            for (_, playlist) in folder_playlists {
                if !playlist.is_empty() {
                    all_playlists.push(playlist);
                }
            }
        } else {
            let playlist_name = dir.file_name().map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());
            let mut playlist = Playlist::new(playlist_name);
            
            let audio_files: Vec<_> = WalkDir::new(dir)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| is_audio_file(e))
                .collect();

            let tracks: Vec<_> = audio_files
                .par_iter()
                .filter_map(|entry| {
                    let file_path = entry.path().to_string_lossy().to_string();
                    match crate::metadata::get_track_metadata(file_path) {
                        Ok(metadata) => Some(metadata),
                        Err(e) => {
                            eprintln!("Failed to get metadata for file: {}", e);
                            None
                        }
                    }
                })
                .collect();
            
            playlist.files.extend(tracks);
            
            if !playlist.is_empty() {
                all_playlists.push(playlist);
            }
        }
    }

    Ok(all_playlists)
}

/// 检查文件是否存在
#[command]
pub fn check_file_exists(path: String) -> Result<bool, String> {
    // 检查原始路径
    if Path::new(&path).exists() {
        return Ok(true);
    }
    
    // 尝试另一种路径分隔符格式
    let alt_path = if path.contains('/') {
        path.replace("/", "\\")
    } else {
        path.replace("\\", "/")
    };
    
    if alt_path != path && Path::new(&alt_path).exists() {
        return Ok(true);
    }
    
    Ok(false)
}

/// 读取歌词文件内容
#[command]
pub fn read_lyrics_file(path: String) -> Result<String, String> {
    // 使用标准 Rust 文件 API 读取歌词文件
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// 检查是否为音频文件
fn is_audio_file(entry: &DirEntry) -> bool {
    entry.path().extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_lowercase().as_str(), "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac"))
        .unwrap_or(false)
}