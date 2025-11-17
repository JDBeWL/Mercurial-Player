#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod audio_decoder;
mod config;

use std::sync::{Arc, Mutex};
use rodio::{OutputStream, Sink};
use crate::config::ConfigManager;
use crate::audio_decoder::SymphoniaSource;

pub struct PlayerState {
    pub sink: Arc<Mutex<Sink>>,
    pub current_source: Arc<Mutex<Option<SymphoniaSource>>>,
    pub current_path: Arc<Mutex<Option<String>>>,
    pub target_volume: Arc<Mutex<f32>>,
}

pub struct AppState {
    pub player: PlayerState,
    pub config_manager: ConfigManager,
}

fn main() {
    // Create the audio stream and sink at startup
    let (_stream, stream_handle) = OutputStream::try_default().expect("Failed to create output stream");
    // We need to keep the stream alive, but it's not Send or Sync.
    // Leaking it is a simple way to keep it alive for the lifetime of the app.
    Box::leak(Box::new(_stream));

    let sink = Sink::try_new(&stream_handle).expect("Failed to create sink");

    // Create the config manager
    let config_manager = ConfigManager::new();
    
    // Initialize config files on startup
    if let Err(e) = config_manager.initialize_config_files() {
        eprintln!("Failed to initialize config files: {}", e);
    }

    // Create the managed state
    let app_state = AppState {
        player: PlayerState {
            sink: Arc::new(Mutex::new(sink)),
            current_source: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(None)),
            target_volume: Arc::new(Mutex::new(1.0)),
        },
        config_manager,
    };

    tauri::Builder::default()
        .manage(app_state) // Add the state to the app
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
            // Existing commands
            commands::read_directory,
            commands::get_audio_files,
            commands::read_lyrics_file,
            commands::get_track_metadata,
            commands::get_all_audio_files,
            
            // New audio control commands
            commands::play_track,
            commands::pause_track,
            commands::resume_track,
            commands::set_volume,
            commands::get_playback_status,
            commands::seek_track,
            commands::is_track_finished,

            // File system commands
            commands::check_file_exists,

            // Config management commands
            commands::initialize_config_files,
            commands::load_config,
            commands::save_config,
            commands::export_config,
            commands::import_config,
            commands::reset_config,
            // Music directory management commands
            commands::add_music_directory,
            commands::remove_music_directory,
            commands::set_music_directories,
            commands::get_music_directories,
            // Font management commands
            commands::get_system_fonts,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}