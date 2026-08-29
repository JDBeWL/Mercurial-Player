//! Mercurial Player - 主入口模块
//!
//! 这是一个基于 Tauri 的音乐播放器应用程序。
//! 支持 WASAPI 独占模式和共享模式播放。
//!
//! Copyright (C) 2026  JDBeWL
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful,
//! but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//! GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License
//! along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_setup;
mod app_state;

use cpal::traits::HostTrait;

use mercurial_player::{
    audio, config, config::ConfigManager, equalizer, media, plugins, system, updater,
};

#[cfg(windows)]
use mercurial_player::taskbar;

fn main() {
    // 初始化 cpal host
    let host = cpal::default_host();
    let device = if let Some(device) = host.default_output_device() {
        device
    } else {
        log::error!("No default output device available");
        eprintln!("错误: 未检测到可用的音频输出设备,应用无法启动。");
        std::process::exit(1);
    };
    let device_name = audio::device::get_device_friendly_name(&device)
        .unwrap_or_else(|| "Unknown Device".to_string());

    // 创建配置管理器
    let config_manager = ConfigManager::new();

    // 初始化配置文件
    if let Err(e) = config_manager.initialize_config_files() {
        log::error!("Failed to initialize config files: {e}");
    }

    // 从配置加载独占模式设置
    let (exclusive_mode_enabled, fade_enabled) = config_manager
        .load_config()
        .map(|c| (c.audio.exclusive_mode, c.audio.fade_enabled))
        .unwrap_or((false, true));

    log::info!(
        "Loaded exclusive mode from config: {exclusive_mode_enabled}, fade enabled: {fade_enabled}"
    );

    // 根据独占模式设置创建播放器
    let output = {
        let result = if exclusive_mode_enabled {
            app_setup::create_exclusive_mode_player(&device_name)
        } else {
            app_setup::create_shared_mode_player(&device)
        };
        match result {
            Ok(output) => output,
            Err(e) => {
                log::error!("Failed to initialize audio output: {e}");
                eprintln!("错误: 音频输出初始化失败,应用即将退出: {e}");
                std::process::exit(1);
            }
        }
    };

    // 创建应用程序状态
    let app_state = app_state::build_app_state(
        output,
        device_name,
        exclusive_mode_enabled,
        fade_enabled,
        config_manager,
    );

    tauri::Builder::default()
        .manage(app_state)
        .manage(updater::PendingUpdate::new())
        .setup(|app| {
            app_setup::init(app);
            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                // dev: Debug (含 debug!, 不含 trace!); release: Info
                // 关键: 关闭 wasapi crate 的 trace 日志,避免 WASAPI 消费线程被 I/O 阻塞导致音频毛刺
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .level_for("wasapi", log::LevelFilter::Warn)
                .level_for("symphonia", log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            // 文件系统命令
            media::commands::read_directory,
            media::commands::get_audio_files,
            media::commands::read_lyrics_file,
            media::commands::write_lyrics_file,
            media::commands::get_all_audio_files,
            media::commands::check_file_exists,
            // 元数据命令
            media::commands::get_track_metadata,
            media::commands::get_tracks_metadata_batch,
            media::commands::get_track_cover_path,
            media::commands::extract_cover,
            media::commands::clean_cover_cache_command,
            media::commands::set_cover_cache_path_command,
            media::commands::clear_metadata_cache_command,
            media::commands::get_metadata_cache_stats_command,
            media::commands::get_temp_dir_command,
            // Tantivy 搜索命令
            media::commands::search_tracks_command,
            media::commands::get_index_doc_count_command,
            media::commands::rebuild_tantivy_index_command,
            media::commands::clear_tantivy_index_command,
            media::commands::commit_tantivy_index_command,
            // 网易云音乐API命令
            media::commands::netease_search_songs,
            media::commands::netease_get_lyrics,
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
            system::commands::get_external_fonts,
            system::commands::get_font_cache_stats,
            system::commands::clear_font_caches,
            system::commands::get_platform,
            system::commands::get_screen_refresh_rate,
            system::commands::get_display_refresh_rates,
            system::commands::open_external_url,
            // 音频设备命令
            audio::commands::get_audio_devices,
            audio::commands::set_audio_device,
            audio::commands::get_current_audio_device,
            audio::commands::toggle_exclusive_mode,
            audio::commands::get_exclusive_mode,
            audio::commands::set_target_fps,
            audio::commands::set_fade_enabled,
            audio::commands::get_fade_enabled,
            // 上次播放会话恢复命令
            audio::commands::resume_last_session,
            audio::commands::save_last_session,
            audio::commands::clear_last_session,
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
            // 插件命令
            plugins::commands::list_plugins,
            plugins::commands::read_plugin_manifest,
            plugins::commands::read_plugin_main,
            plugins::commands::install_plugin,
            plugins::commands::uninstall_plugin,
            plugins::commands::get_plugins_directory,
            plugins::commands::open_plugins_directory,
            plugins::commands::save_screenshot,
            plugins::commands::open_screenshots_directory,
            // 任务栏命令（Windows Only）
            #[cfg(windows)]
            taskbar::commands::update_taskbar_state,
            #[cfg(windows)]
            taskbar::commands::set_taskbar_stopped,
            // 桌面歌词命令（Windows Only）
            #[cfg(windows)]
            taskbar::desktop_lyrics::show_desktop_lyrics,
            #[cfg(windows)]
            taskbar::desktop_lyrics::hide_desktop_lyrics,
            #[cfg(windows)]
            taskbar::desktop_lyrics::update_desktop_lyric,
            #[cfg(windows)]
            taskbar::desktop_lyrics::set_desktop_lyrics_locked,
            #[cfg(windows)]
            taskbar::desktop_lyrics::set_desktop_lyrics_font_size,
            #[cfg(windows)]
            taskbar::desktop_lyrics::set_desktop_lyrics_font_family,
            #[cfg(windows)]
            taskbar::desktop_lyrics::set_desktop_lyrics_color_preset,
            #[cfg(windows)]
            taskbar::desktop_lyrics::is_desktop_lyrics_visible,
            // 系统版本命令（保留 get_app_version 用于前端显示）
            system::commands::get_app_version,
            // 应用更新命令（多线程分片下载）
            updater::updater_check,
            updater::updater_download,
            updater::updater_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
