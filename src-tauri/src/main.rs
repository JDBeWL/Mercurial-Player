//! Mer Music Player - 主入口模块
//!
//! 这是一个基于 Tauri 的音乐播放器应用程序。
//! 支持 WASAPI 独占模式和共享模式播放。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use music_player::{
    AppState, PlayerState, audio,
    audio::WasapiExclusivePlayback,
    config,
    config::ConfigManager,
    equalizer,
    equalizer::{Equalizer, GlobalEqualizer},
    media, system,
};

use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, Sink};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

fn main() {
    // 初始化 cpal host
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No default output device available");
    let device_name = device
        .name()
        .unwrap_or_else(|_| "Unknown Device".to_string());

    // 创建配置管理器
    let config_manager = ConfigManager::new();

    // 初始化配置文件
    if let Err(e) = config_manager.initialize_config_files() {
        eprintln!("Failed to initialize config files: {e}");
    }

    // 从配置加载独占模式设置
    let exclusive_mode_enabled = config_manager
        .load_config()
        .map(|c| c.audio.exclusive_mode)
        .unwrap_or(false);

    println!("Loaded exclusive mode from config: {exclusive_mode_enabled}");

    // 根据独占模式设置创建播放器
    let (sink, wasapi_player) = if exclusive_mode_enabled {
        create_exclusive_mode_player(&device_name)
    } else {
        create_shared_mode_player(&device)
    };

    // 创建应用程序状态
    let app_state = AppState {
        player: PlayerState {
            sink: Arc::new(Mutex::new(sink)),
            current_source: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(None)),
            target_volume: Arc::new(Mutex::new(1.0)),
            current_device_name: Arc::new(Mutex::new(device_name)),
            exclusive_mode: Arc::new(Mutex::new(
                exclusive_mode_enabled && wasapi_player.is_some(),
            )),
            waveform_data: Arc::new(Mutex::new(Vec::with_capacity(1024))),
            spectrum_data: Arc::new(Mutex::new(vec![0.0; 128])),
            wasapi_player: Arc::new(Mutex::new(wasapi_player)),
            decode_thread_stop: Arc::new(AtomicBool::new(false)),
            decode_thread_id: Arc::new(AtomicU64::new(0)),
            equalizer: Arc::new(Mutex::new(Equalizer::new(48000, 2))),
        },
        config_manager,
        equalizer: GlobalEqualizer::new(),
    };

    tauri::Builder::default()
        .manage(app_state)
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                use tauri::Manager;
                let window = _app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            // 文件系统命令
            media::commands::read_directory,
            media::commands::get_audio_files,
            media::commands::read_lyrics_file,
            media::commands::get_all_audio_files,
            media::commands::check_file_exists,
            // 元数据命令
            media::commands::get_track_metadata,
            media::commands::get_tracks_metadata_batch,
            // 播放命令
            audio::commands::play_track,
            audio::commands::pause_track,
            audio::commands::resume_track,
            audio::commands::set_volume,
            audio::commands::get_playback_status,
            audio::commands::seek_track,
            audio::commands::is_track_finished,
            audio::commands::get_waveform_data,
            audio::commands::get_spectrum_data,
            // 配置命令
            config::commands::initialize_config_files,
            config::commands::load_config,
            config::commands::save_config,
            config::commands::export_config,
            config::commands::import_config,
            config::commands::reset_config,
            // 音乐目录命令
            config::commands::add_music_directory,
            config::commands::remove_music_directory,
            config::commands::set_music_directories,
            config::commands::get_music_directories,
            // 系统命令
            system::commands::get_system_info,
            system::commands::get_system_fonts,
            // 音频设备命令
            audio::commands::get_audio_devices,
            audio::commands::set_audio_device,
            audio::commands::get_current_audio_device,
            audio::commands::toggle_exclusive_mode,
            audio::commands::get_exclusive_mode,
            // EQ 均衡器命令
            equalizer::commands::get_eq_bands,
            equalizer::commands::get_eq_settings,
            equalizer::commands::set_eq_enabled,
            equalizer::commands::set_eq_gains,
            equalizer::commands::set_eq_band_gain,
            equalizer::commands::set_eq_preamp,
            equalizer::commands::get_eq_presets,
            equalizer::commands::apply_eq_preset,
            equalizer::commands::reset_eq,
            // 窗口命令
            system::commands::set_mini_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 创建独占模式播放器
fn create_exclusive_mode_player(device_name: &str) -> (Sink, Option<WasapiExclusivePlayback>) {
    println!("Starting in WASAPI exclusive mode");

    // 创建一个空的 rodio sink（使用默认设备，但不会实际使用）
    let (_stream, stream_handle) =
        OutputStream::try_default().expect("Failed to create default output stream");
    Box::leak(Box::new(_stream));
    let sink = Sink::try_new(&stream_handle).expect("Failed to create sink");

    // 创建 WASAPI 独占播放器
    let wasapi_playback = WasapiExclusivePlayback::new();
    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_name)) => {
            println!(
                "WASAPI Exclusive initialized: {actual_name} @ {sample_rate}Hz, {channels} channels"
            );
            (sink, Some(wasapi_playback))
        }
        Err(e) => {
            eprintln!("Failed to initialize WASAPI exclusive mode: {e}");
            eprintln!("Falling back to shared mode");
            (sink, None)
        }
    }
}

/// 创建共享模式播放器
fn create_shared_mode_player(device: &cpal::Device) -> (Sink, Option<WasapiExclusivePlayback>) {
    println!("Starting in shared mode");

    // 从选定的设备创建音频输出流
    let (_stream, stream_handle) =
        OutputStream::try_from_device(device).expect("Failed to create output stream from device");

    // 保持流的存活（泄露是简单的方法，让它在应用程序的生命周期内保持存活）
    Box::leak(Box::new(_stream));

    let sink = Sink::try_new(&stream_handle).expect("Failed to create sink");
    (sink, None)
}
