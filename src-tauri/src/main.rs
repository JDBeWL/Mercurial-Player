#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_decoder;
mod audio_device;
mod config;
mod config_commands;
mod error;
mod filesystem;
mod metadata;
mod playback;
mod system;
mod wasapi_player;
mod wasapi_exclusive;
mod window_commands;

// 重新导出所有命令
pub use audio_device::*;
pub use config_commands::*;
pub use filesystem::*;
pub use metadata::*;
pub use playback::*;
pub use system::*;
pub use window_commands::*;

use crate::audio_decoder::SymphoniaSource;
use crate::config::ConfigManager;
use crate::wasapi_exclusive::WasapiExclusivePlayback;
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, Sink};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

pub struct PlayerState {
    pub sink: Arc<Mutex<Sink>>,
    pub current_source: Arc<Mutex<Option<SymphoniaSource>>>,
    pub current_path: Arc<Mutex<Option<String>>>,
    pub target_volume: Arc<Mutex<f32>>,
    pub current_device_name: Arc<Mutex<String>>,
    pub exclusive_mode: Arc<Mutex<bool>>,
    pub waveform_data: Arc<Mutex<Vec<f32>>>,
    pub spectrum_data: Arc<Mutex<Vec<f32>>>,
    pub wasapi_player: Arc<Mutex<Option<WasapiExclusivePlayback>>>,
    /// 用于停止解码线程的标志
    pub decode_thread_stop: Arc<AtomicBool>,
    /// 当前解码线程的 ID（用于区分不同的播放会话）
    pub decode_thread_id: Arc<AtomicU64>,
}

pub struct AppState {
    pub player: PlayerState,
    pub config_manager: ConfigManager,
}

fn main() {
    // 初始化cpal host
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
        eprintln!("Failed to initialize config files: {}", e);
    }

    // 从配置加载独占模式设置
    let exclusive_mode_enabled = config_manager
        .load_config()
        .map(|c| c.audio.exclusive_mode)
        .unwrap_or(false);

    println!(
        "Loaded exclusive mode from config: {}",
        exclusive_mode_enabled
    );

    // 根据独占模式设置创建播放器
    let (sink, wasapi_player) = if exclusive_mode_enabled {
        // 独占模式：创建 WASAPI 独占播放器，使用空的 rodio sink
        println!("Starting in WASAPI exclusive mode");
        
        // 创建一个空的 rodio sink（使用默认设备，但不会实际使用）
        let (_stream, stream_handle) =
            OutputStream::try_default().expect("Failed to create default output stream");
        Box::leak(Box::new(_stream));
        let sink = Sink::try_new(&stream_handle).expect("Failed to create sink");
        
        // 创建 WASAPI 独占播放器
        let wasapi_playback = WasapiExclusivePlayback::new();
        match wasapi_playback.initialize(Some(&device_name)) {
            Ok((sample_rate, channels, actual_name)) => {
                println!(
                    "WASAPI Exclusive initialized: {} @ {}Hz, {} channels",
                    actual_name, sample_rate, channels
                );
                (sink, Some(wasapi_playback))
            }
            Err(e) => {
                eprintln!("Failed to initialize WASAPI exclusive mode: {}", e);
                eprintln!("Falling back to shared mode");
                (sink, None)
            }
        }
    } else {
        // 共享模式：使用 rodio
        println!("Starting in shared mode");
        
        // 从选定的设备创建音频输出流
        let (_stream, stream_handle) =
            OutputStream::try_from_device(&device).expect("Failed to create output stream from device");

        // 我们需要保持流的存活，但它不是Send或Sync。
        // 泄露它是一种简单的方法，让它在应用程序的生命周期内保持存活。
        Box::leak(Box::new(_stream));

        let sink = Sink::try_new(&stream_handle).expect("Failed to create sink");
        (sink, None)
    };

    // 创建应用程序状态
    let app_state = AppState {
        player: PlayerState {
            sink: Arc::new(Mutex::new(sink)),
            current_source: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(None)),
            target_volume: Arc::new(Mutex::new(1.0)),
            current_device_name: Arc::new(Mutex::new(device_name)),
            exclusive_mode: Arc::new(Mutex::new(exclusive_mode_enabled && wasapi_player.is_some())),
            waveform_data: Arc::new(Mutex::new(Vec::with_capacity(1024))),
            spectrum_data: Arc::new(Mutex::new(vec![0.0; 128])),
            wasapi_player: Arc::new(Mutex::new(wasapi_player)),
            decode_thread_stop: Arc::new(AtomicBool::new(false)),
            decode_thread_id: Arc::new(AtomicU64::new(0)),
        },
        config_manager,
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
            read_directory,
            get_audio_files,
            read_lyrics_file,
            get_all_audio_files,
            check_file_exists,
            // 元数据命令
            get_track_metadata,
            // 播放命令
            play_track,
            pause_track,
            resume_track,
            set_volume,
            get_playback_status,
            seek_track,
            seek_track,
            is_track_finished,
            get_waveform_data,
            get_spectrum_data,
            // 配置命令
            initialize_config_files,
            load_config,
            save_config,
            export_config,
            import_config,
            reset_config,
            // 音乐目录命令
            add_music_directory,
            remove_music_directory,
            set_music_directories,
            get_music_directories,
            // 系统命令
            get_system_info,
            get_system_fonts,
            // 音频设备命令
            get_audio_devices,
            set_audio_device,
            get_current_audio_device,
            toggle_exclusive_mode,
            get_exclusive_mode,
            // 窗口命令
            set_mini_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
