use crate::AppState;
use crate::config::AppConfig;
use crate::audio_decoder::{SymphoniaDecoder, SymphoniaSource};
use lofty::{Accessor, AudioFile, Probe, TaggedFileExt};
use rodio::Source;
use std::sync::Arc;
use std::fs::{self, File};
use serde::Serialize;
use std::io::BufReader;
use std::time::Duration;
use tauri::{command, State};
use walkdir::{DirEntry, WalkDir};
use base64::{Engine as _, engine::general_purpose};
use std::path::Path;

// The metadata for a single track.
#[derive(Debug, Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackMetadata {
    path: String,
    name: String,
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<f64>,
    cover: Option<String>,
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    channels: Option<u8>,
}

// A playlist containing multiple tracks.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    name: String,
    files: Vec<TrackMetadata>,
}

// Represents the current status of the audio player.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    is_playing: bool,
    position_secs: f32,
    volume: f32,
}

// --- Audio Playback Commands ---

// ... (rest of the file)

#[command]
pub fn play_track(state: State<AppState>, path: String) -> Result<(), String> {
    let player_state = &state.player;

    // Stop the current track and clear the sink
    {
        let sink = player_state.sink.lock().unwrap();
        sink.stop();
        // Restore volume before playing the new track
        let target_volume = *player_state.target_volume.lock().unwrap();
        sink.set_volume(target_volume);
    }
    
    // Update current track info
    *player_state.current_path.lock().unwrap() = Some(path.clone());
    *player_state.current_source.lock().unwrap() = None;

    let file_path = Path::new(&path);
    let extension = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let source: Box<dyn rodio::Source<Item = f32> + Send> = if extension.eq_ignore_ascii_case("mp3") {
        println!("使用rodio解码器播放MP3: {}", path);
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let reader = BufReader::new(file);
        Box::new(rodio::Decoder::new(reader).map_err(|e| e.to_string())?.convert_samples::<f32>())
    } else {
        match SymphoniaDecoder::new(&path) {
            Ok(symphonia_decoder) => {
                println!("使用Symphonia解码器: {}", path);
                Box::new(SymphoniaSource::new(symphonia_decoder))
            }
            Err(e) => {
                println!("Symphonia解码失败，回退到rodio解码器: {}", e);
                let file = File::open(&path).map_err(|e| e.to_string())?;
                let reader = BufReader::new(file);
                let rodio_source = rodio::Decoder::new(reader).map_err(|e| e.to_string())?;
                Box::new(rodio_source.convert_samples::<f32>())
            }
        }
    };

    let sink = player_state.sink.lock().unwrap();
    sink.append(source);
    sink.play();
    
    Ok(())
}


#[command]
pub fn pause_track(state: State<AppState>) {
    let player_state = &state.player;
    let sink = Arc::clone(&player_state.sink);
    sink.lock().unwrap().pause();
}

#[command]
pub fn resume_track(state: State<AppState>) {
    let sink = state.player.sink.lock().unwrap();
    sink.play();
}

#[command]
pub fn set_volume(state: State<AppState>, volume: f32) {
    let player_state = &state.player;
    *player_state.target_volume.lock().unwrap() = volume;
    let sink = player_state.sink.lock().unwrap();
    sink.set_volume(volume);
}

#[command]
pub fn is_track_finished(state: State<AppState>) -> bool {
    let sink = state.player.sink.lock().unwrap();
    // 检查sink是否为空且当前没有暂停状态
    sink.empty() && !sink.is_paused()
}

#[command]
pub fn seek_track(state: State<AppState>, time: f32) -> Result<(), String> {
    let player_state = &state.player;
    let duration = Duration::from_secs_f32(time);
    
    if let Some(path) = player_state.current_path.lock().unwrap().clone() {
        let file_path = Path::new(&path);
        let extension = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");

        // 如果是mp3，直接使用rodio的seek
        if extension.eq_ignore_ascii_case("mp3") {
            println!("使用rodio seek for MP3");
            let sink = player_state.sink.lock().unwrap();
            return match sink.try_seek(duration) {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("Failed to seek track with rodio: {}", e)),
            };
        }

        // 对于其他格式，继续使用Symphonia
        match SymphoniaDecoder::new(&path) {
            Ok(mut decoder) => {
                match decoder.seek(duration) {
                    Ok(_) => {
                        let source: Box<dyn rodio::Source<Item = f32> + Send> = Box::new(SymphoniaSource::new(decoder));
                        
                        let sink = player_state.sink.lock().unwrap();
                        sink.stop();
                        // The sink is implicitly cleared when stopped and a new source is appended.
                        // No need to sleep, usually.
                        
                        sink.append(source);
                        sink.play();
                        
                        return Ok(());
                    }
                    Err(e) => {
                        println!("Symphonia seek failed: {}, falling back to rodio seek", e);
                    }
                }
            }
            Err(e) => {
                println!("Could not create Symphonia decoder for seek: {}, falling back to rodio seek", e);
            }
        }
    }
    
    // Fallback for non-Symphonia or failed Symphonia seek
    let sink = player_state.sink.lock().unwrap();
    match sink.try_seek(duration) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to seek track with rodio: {}", e)),
    }
}

#[command]
pub fn get_playback_status(state: State<AppState>) -> PlaybackStatus {
    let player_state = &state.player;
    let sink = player_state.sink.lock().unwrap();
    
    // 尝试从当前音轨获取持续时间
    let total_duration = if let Some(current_path) = player_state.current_path.lock().unwrap().as_ref() {
        match get_track_metadata(current_path.clone()) {
            Ok(metadata) => metadata.duration,
            Err(_) => None,
        }
    } else {
        None
    };
    
    // 计算播放位置
    let position_secs = if !sink.empty() {
        // 如果播放器不为空，尝试从当前音轨获取位置
        if let (Some(_total), Some(current_path)) = (total_duration, player_state.current_path.lock().unwrap().as_ref()) {
            // 简单估算：基于已经播放的时间
            // 注意：这只是一个粗略的估计，实际应用中可能需要更精确的位置跟踪
            if let Ok(metadata) = get_track_metadata(current_path.clone()) {
                if let (Some(_duration), Some(_)) = (metadata.duration, total_duration) {
                    // 如果有总时长，我们可以使用播放位置比例
                    // 但由于我们无法从rodio获取精确位置，这里返回0
                    0.0
                } else {
                    0.0
                }
            } else {
                0.0
            }
        } else {
            0.0
        }
    } else {
        0.0
    };
    
    PlaybackStatus {
        is_playing: !sink.is_paused() && !sink.empty(),
        position_secs,
        volume: sink.volume(),
    }
}

// --- File System and Metadata Commands ---

#[command]
pub fn get_track_metadata(path: String) -> Result<TrackMetadata, String> {
    let file_path = std::path::Path::new(&path);

    let tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs_f64();

    let mut metadata = TrackMetadata {
        // 标准化路径格式，确保使用反斜杠（Windows格式）
        path: path.replace("/", "\\"),
        name: file_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        duration: if duration > 0.0 { Some(duration) } else { None },
        bitrate: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        ..Default::default()
    };

    if let Some(tag) = tagged_file.primary_tag() {
        metadata.title = tag.title().map(String::from);
        metadata.artist = tag.artist().map(String::from);
        metadata.album = tag.album().map(String::from);

        if let Some(picture) = tag.pictures().get(0) {
            let mime_type = picture.mime_type().map_or("image/jpeg", |m| m.as_str());
            let data = picture.data();
            let cover_data_url = format!(
                "data:{};base64,{}",
                mime_type,
                general_purpose::STANDARD.encode(data)
            );
            metadata.cover = Some(cover_data_url);
        }
    }
    
    if metadata.title.is_none() || metadata.title.as_deref() == Some("") {
        metadata.title = Some(metadata.name.clone());
    }

    Ok(metadata)
}

#[command]
pub fn get_audio_files(path: String) -> Result<Playlist, String> {
    let dir = std::path::Path::new(&path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string());
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| is_audio_file(e))
    {
        // 使用to_string_lossy处理非ASCII字符路径
        let file_path = entry.path().to_string_lossy().to_string();
        match get_track_metadata(file_path) {
            Ok(metadata) => {
                files.push(metadata);
            }
            Err(e) => {
                eprintln!("Failed to get metadata for file: {}", e);
            }
        }
    }

    let playlist_name = dir.file_name().map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

    Ok(Playlist {
        name: playlist_name,
        files,
    })
}

#[command]
pub fn get_all_audio_files(state: State<AppState>, paths: Vec<String>) -> Result<Vec<Playlist>, String> {
    let mut all_playlists: Vec<Playlist> = Vec::new();
    let config = state.config_manager.load_config()?;

    for path in paths {
        let dir = std::path::Path::new(&path);
        if !dir.is_dir() {
            eprintln!("Provided path is not a directory: {}", path);
            continue;
        }

        // 如果启用递归扫描并且需要基于文件夹创建播放列表
        if config.directory_scan.enable_subdirectory_scan && config.playlist.folder_based_playlists {
            // 创建基于文件夹的播放列表
            let mut folder_playlists = std::collections::HashMap::new();
            
            let walker = WalkDir::new(dir)
                .max_depth(config.directory_scan.max_depth as usize)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| {
                    // Apply directory scan configurations
                    let entry_path = e.path();
                    if config.directory_scan.ignore_hidden_folders && 
                       entry_path.file_name().map_or(false, |s| s.to_string_lossy().starts_with('.')) {
                        return false;
                    }
                    if config.directory_scan.folder_blacklist.iter().any(|f| entry_path.ends_with(f)) {
                        return false;
                    }
                    true
                });

            // 收集所有音频文件并按文件夹分组
            for entry in walker {
                if is_audio_file(&entry) {
                    // 使用to_string_lossy处理非ASCII字符路径，并统一路径格式
                    let file_path = entry.path().to_string_lossy().to_string();
                    match get_track_metadata(file_path.clone()) {
                        Ok(mut metadata) => {
                            // 标准化路径格式，确保使用反斜杠（Windows格式）
                            metadata.path = metadata.path.replace("/", "\\");
                            
                            // 获取文件所在的父目录
                            if let Some(parent_dir) = entry.path().parent() {
                                let dir_name = parent_dir.file_name()
                                    .map(|name| name.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "Unknown".to_string());
                                
                                // 使用HashMap来按目录分组
                                folder_playlists.entry(dir_name)
                                    .or_insert_with(Vec::new)
                                    .push(metadata);
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to get metadata for {}: {}", file_path, e);
                        }
                    }
                }
            }
            
            // 将HashMap转换为Playlist并添加到结果中
            for (folder_name, files) in folder_playlists {
                if !files.is_empty() {
                    all_playlists.push(Playlist {
                        name: folder_name,
                        files,
                    });
                }
            }
        } else {
            // 非递归模式或不基于文件夹创建播放列表：为整个目录创建一个播放列表
            let walker = WalkDir::new(dir)
                .max_depth(if config.directory_scan.enable_subdirectory_scan { 
                    config.directory_scan.max_depth as usize 
                } else { 
                    1 
                })
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| {
                    // Apply directory scan configurations
                    let entry_path = e.path();
                    if config.directory_scan.ignore_hidden_folders && 
                       entry_path.file_name().map_or(false, |s| s.to_string_lossy().starts_with('.')) {
                        return false;
                    }
                    if config.directory_scan.folder_blacklist.iter().any(|f| entry_path.ends_with(f)) {
                        return false;
                    }
                    true
                });

            let mut files = Vec::new();
            for entry in walker {
                if is_audio_file(&entry) {
                    // 使用to_string_lossy处理非ASCII字符路径，并统一路径格式
                    let file_path = entry.path().to_string_lossy().to_string();
                    match get_track_metadata(file_path) {
                        Ok(mut metadata) => {
                            // 标准化路径格式，确保使用反斜杠（Windows格式）
                            metadata.path = metadata.path.replace("/", "\\");
                            files.push(metadata);
                        }
                        Err(e) => {
                            eprintln!("Failed to get metadata for file: {}", e);
                        }
                    }
                }
            }

            let playlist_name = dir.file_name().map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

            if !files.is_empty() {
                all_playlists.push(Playlist {
                    name: playlist_name,
                    files,
                });
            }
        }
    }

    // 如果配置中要求生成"全部歌曲"播放列表，则添加一个包含所有歌曲的播放列表
    if config.playlist.generate_all_songs_playlist && !all_playlists.is_empty() {
        let all_files: Vec<TrackMetadata> = all_playlists.iter()
            .flat_map(|p| p.files.clone())
            .collect();
            
        if !all_files.is_empty() {
            all_playlists.insert(0, Playlist {
                name: "全部歌曲".to_string(),
                files: all_files,
            });
        }
    }

    Ok(all_playlists)
}

#[command]
pub fn read_directory(path: String) -> Result<Vec<String>, String> {
    let dir = std::path::Path::new(&path);
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

#[tauri::command]
pub fn read_lyrics_file(path: String) -> Result<String, String> {
    // 使用标准 Rust 文件 API 读取歌词文件
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

fn is_audio_file(entry: &DirEntry) -> bool {
    entry.path().extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_lowercase().as_str(), "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac"))
        .unwrap_or(false)
}

// --- File System Commands ---

#[command]
pub fn check_file_exists(path: String) -> Result<bool, String> {
    // 检查原始路径
    if std::path::Path::new(&path).exists() {
        return Ok(true);
    }
    
    // 尝试另一种路径分隔符格式
    let alt_path = if path.contains('/') {
        path.replace("/", "\\")
    } else {
        path.replace("\\", "/")
    };
    
    if alt_path != path && std::path::Path::new(&alt_path).exists() {
        return Ok(true);
    }
    
    Ok(false)
}

// --- Config Management Commands ---

#[command]
pub fn initialize_config_files(state: State<AppState>) -> Result<(), String> {
    state.config_manager.initialize_config_files()
}

#[command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, String> {
    state.config_manager.load_config()
}

#[command]
pub fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    state.config_manager.save_config(&config)
}

#[command]
pub fn export_config(state: State<AppState>, config: AppConfig, file_path: String) -> Result<(), String> {
    state.config_manager.export_config(&config, &file_path)
}

#[command]
pub fn import_config(state: State<AppState>, file_path: String) -> Result<AppConfig, String> {
    state.config_manager.import_config(&file_path)
}

#[command]
pub fn reset_config(state: State<AppState>) -> Result<AppConfig, String> {
    state.config_manager.reset_config()
}

// --- Font Management Commands ---

#[command]
pub fn get_system_fonts() -> Result<Vec<String>, String> {
    let mut fonts = Vec::new();
    
    // 添加最常用的系统字体和MD3推荐字体
    fonts.push("system-ui".to_string());         // 系统默认UI字体
    fonts.push("Roboto".to_string());            // MD3字体
    fonts.push("Arial".to_string());             // Windows常用
    fonts.push("Helvetica".to_string());          // macOS常用
    fonts.push("Times New Roman".to_string());    // 衬线字体
    fonts.push("Noto Sans".to_string());         // Google字体，支持多语言
    fonts.push("Segoe UI".to_string());          // Windows 10/11默认
    fonts.push("PingFang SC".to_string());       // macOS中文默认
    fonts.push("Microsoft YaHei".to_string());   // Windows中文默认
    fonts.push("Consolas".to_string());          // 常用等宽字体
    
    // 去重并排序
    fonts.sort();
    fonts.dedup();
    
    Ok(fonts)
}

// --- Music Directory Management Commands ---

#[command]
pub fn add_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, String> {
    let mut config = state.config_manager.load_config()?;
    if !config.music_directories.contains(&path) {
        config.music_directories.push(path);
        state.config_manager.save_config(&config)?;
    }
    Ok(config.music_directories)
}

#[command]
pub fn remove_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, String> {
    let mut config = state.config_manager.load_config()?;
    config.music_directories.retain(|p| p != &path);
    state.config_manager.save_config(&config)?;
    Ok(config.music_directories)
}

#[command]
pub fn set_music_directories(state: State<AppState>, paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut config = state.config_manager.load_config()?;
    config.music_directories = paths;
    state.config_manager.save_config(&config)?;
    Ok(config.music_directories)
}

#[command]
pub fn get_music_directories(state: State<AppState>) -> Result<Vec<String>, String> {
    let config = state.config_manager.load_config()?;
    Ok(config.music_directories)
}