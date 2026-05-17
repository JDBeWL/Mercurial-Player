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

use mercurial_player::{
    AppState, PlayerState, audio,
    config,
    config::ConfigManager,
    equalizer,
    equalizer::{Equalizer, GlobalEqualizer},
    media, plugins, system,
};

#[cfg(windows)]
use mercurial_player::audio::{WasapiExclusivePlayback, DeviceMonitor};

#[cfg(not(windows))]
use mercurial_player::audio::DeviceMonitor;

#[cfg(windows)]
use mercurial_player::taskbar;

use cpal::traits::HostTrait;
use rodio::stream::{MixerDeviceSink, DeviceSinkBuilder};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

/// 跨平台的播放器类型别名
#[cfg(windows)]
type PlatformPlayer = WasapiExclusivePlayback;
#[cfg(not(windows))]
type PlatformPlayer = Placeholder;

/// 非 Windows 平台的占位类型
#[cfg(not(windows))]
#[derive(Debug)]
struct Placeholder;

fn main() {
    // 初始化 cpal host
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No default output device available");
    let device_name = audio::device::get_device_friendly_name(&device)
        .unwrap_or_else(|| "Unknown Device".to_string());

    // 创建配置管理器
    let config_manager = ConfigManager::new();

    // 初始化配置文件
    if let Err(e) = config_manager.initialize_config_files() {
        log::error!("Failed to initialize config files: {e}");
    }

    // 从配置加载独占模式设置
    let exclusive_mode_enabled = config_manager
        .load_config()
        .map(|c| c.audio.exclusive_mode)
        .unwrap_or(false);

    log::info!("Loaded exclusive mode from config: {exclusive_mode_enabled}");

    // 根据独占模式设置创建播放器
    let (sink, output_stream, wasapi_player) = {
        if exclusive_mode_enabled {
            create_exclusive_mode_player(&device_name)
        } else {
            create_shared_mode_player(&device)
        }
    };

    // 创建应用程序状态
    let app_state = AppState {
        player: PlayerState {
            sink: Arc::new(Mutex::new(sink)),
            output_stream: Arc::new(Mutex::new(Some(output_stream))),
            current_source: Arc::new(Mutex::new(None)),
            current_path: Arc::new(Mutex::new(None)),
            target_volume: Arc::new(Mutex::new(1.0)),
            current_device_name: Arc::new(Mutex::new(device_name.clone())),
            exclusive_mode: Arc::new(Mutex::new(
                exclusive_mode_enabled && {
                    #[cfg(windows)]
                    { wasapi_player.is_some() }
                    #[cfg(not(windows))]
                    { false }
                },
            )),
            waveform_data: Arc::new(Mutex::new(Vec::with_capacity(1024))),
            spectrum_data: Arc::new(Mutex::new(vec![0.0; 128])),
            wasapi_player: {
                #[cfg(windows)]
                {
                    Arc::new(Mutex::new(wasapi_player))
                }
                #[cfg(not(windows))]
                {
                    Arc::new(Mutex::new(None))
                }
            },
            decode_thread_stop: Arc::new(AtomicBool::new(false)),
            decode_thread_id: Arc::new(AtomicU64::new(0)),
            equalizer: Arc::new(Mutex::new(Equalizer::new(48000, 2))),
            device_monitor: Arc::new(Mutex::new(DeviceMonitor::new(device_name))),
            target_fps: Arc::new(AtomicU64::new(60)), // 默认60fps
            enable_vertical_sync: Arc::new(AtomicBool::new(false)), // 默认关闭垂直同步
        },
        config_manager,
        equalizer: GlobalEqualizer::new(),
    };

    tauri::Builder::default()
        .manage(app_state)
        .setup(|app| {
            use tauri::Manager;

            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }

            // 启动设备监听器
            {
                let state: tauri::State<AppState> = app.state();
                let mut monitor = state.player.device_monitor.lock().unwrap();
                monitor.start(app.handle().clone());
                log::info!("Device monitor started");
            }

            // 清理封面缓存
            {
                use mercurial_player::media::metadata;
                let state: tauri::State<AppState> = app.state();
                let max_cache_size_mb = state
                    .config_manager
                    .load_config()
                    .ok()
                    .map(|config| config.general.cover_cache_size_mb);
                
                match metadata::clean_cover_cache(max_cache_size_mb) {
                    Ok(count) => {
                        if count > 0 {
                            log::info!("应用启动时清理了 {} 个封面缓存文件", count);
                        }
                    }
                    Err(e) => {
                        log::warn!("清理封面缓存失败: {}", e);
                    }
                }
            }

            // 初始化Windows任务栏缩略图工具栏
            #[cfg(windows)]
            {
                let window = app.get_webview_window("main").unwrap();
                let app_handle = app.handle().clone();

                // 延迟初始化任务栏，确保窗口已完全创建
                std::thread::spawn(move || {
                    // 等待窗口完全初始化
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    // 初始化COM库
                    #[allow(unsafe_code)]
                    {
                        unsafe {
                            let _ = windows::Win32::System::Com::CoInitializeEx(
                                None,
                                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                            );
                        }
                    }

                    // 获取窗口句柄
                    if let Ok(hwnd) = window.hwnd() {
                        let hwnd_value = hwnd.0 as isize;

                        // 初始化任务栏
                        if let Err(e) = taskbar::init_taskbar(hwnd_value) {
                            log::error!("Failed to initialize taskbar: {e}");
                        } else {
                            log::info!("Taskbar initialized successfully");

                            // 设置窗口消息钩子来处理按钮点击
                            setup_taskbar_hook(hwnd_value, app_handle);
                        }
                    }
                });
            }

            Ok(())
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::new().build())
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
            system::commands::get_platform,
            system::commands::get_screen_refresh_rate,
            system::commands::open_external_url,
            // 音频设备命令
            audio::commands::get_audio_devices,
            audio::commands::set_audio_device,
            audio::commands::get_current_audio_device,
            audio::commands::toggle_exclusive_mode,
            audio::commands::get_exclusive_mode,
            audio::commands::set_target_fps,
            audio::commands::set_vertical_sync,
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
            taskbar::desktop_lyrics::set_desktop_lyrics_color_preset,
            #[cfg(windows)]
            taskbar::desktop_lyrics::is_desktop_lyrics_visible,
            // 系统版本命令（保留 get_app_version 用于前端显示）
            system::commands::get_app_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 创建独占模式播放器
#[cfg(windows)]
fn create_exclusive_mode_player(device_name: &str) -> (rodio::Player, MixerDeviceSink, Option<PlatformPlayer>) {
    log::info!("Starting in WASAPI exclusive mode");

    // 创建一个空的rodio sink
    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .expect("Failed to create default device sink builder")
        .open_stream()
        .expect("Failed to create default mixer sink");
    let player = rodio::Player::connect_new(mixer_sink.mixer());

    // 创建 WASAPI 独占播放器
    let wasapi_playback = WasapiExclusivePlayback::new();
    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_name)) => {
            log::info!(
                "WASAPI Exclusive initialized: {actual_name} @ {sample_rate}Hz, {channels} channels"
            );
            (player, mixer_sink, Some(wasapi_playback))
        }
        Err(e) => {
            log::error!("Failed to initialize WASAPI exclusive mode: {e}");
            log::warn!("Falling back to shared mode");
            (player, mixer_sink, None)
        }
    }
}

/// 创建独占模式播放器（非Windows平台回退到共享模式）
#[cfg(not(windows))]
fn create_exclusive_mode_player(_device_name: &str) -> (rodio::Player, MixerDeviceSink, Option<PlatformPlayer>) {
    log::warn!("Exclusive mode is only supported on Windows, falling back to shared mode");
    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .expect("Failed to create default device sink builder")
        .open_stream()
        .expect("Failed to create default mixer sink");
    let player = rodio::Player::connect_new(mixer_sink.mixer());
    (player, mixer_sink, None)
}

/// 设置任务栏按钮点击钩子
#[cfg(windows)]
#[allow(unsafe_code)] // Windows API交互需要unsafe
fn setup_taskbar_hook(hwnd: isize, app_handle: tauri::AppHandle) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, SetWindowLongPtrW, GWLP_WNDPROC, WM_COMMAND, WNDPROC,
    };

    // 存储原始窗口过程和app handle
    static ORIGINAL_WNDPROC: OnceLock<isize> = OnceLock::new();
    static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

    let _ = APP_HANDLE.set(app_handle);

    // 自定义窗口过程
    unsafe extern "system" fn custom_wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // 检查是否是任务栏按钮点击消息
        if msg == WM_COMMAND {
            let cmd_id = (wparam.0 & 0xFFFF) as u32;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u32;

            // THBN_CLICKED = 0x1800
            if notify_code == 0x1800 {
                if let Some(app) = APP_HANDLE.get() {
                    use tauri::Emitter;

                    match cmd_id {
                        0 => {
                            // BTN_PREVIOUS
                            log::debug!("Taskbar: Previous button clicked");
                            let _ = app.emit("taskbar-previous", ());
                        }
                        1 => {
                            // BTN_PLAY_PAUSE
                            log::debug!("Taskbar: Play/Pause button clicked");
                            let _ = app.emit("taskbar-play-pause", ());
                        }
                        2 => {
                            // BTN_NEXT
                            log::debug!("Taskbar: Next button clicked");
                            let _ = app.emit("taskbar-next", ());
                        }
                        _ => {}
                    }
                }
            }
        }

        // 调用原始窗口过程
        if let Some(&original) = ORIGINAL_WNDPROC.get() {
            // 将存储的原始窗口过程指针转换回WNDPROC类型
            unsafe {
                let original_proc: WNDPROC = std::mem::transmute(original);
                CallWindowProcW(original_proc, hwnd, msg, wparam, lparam)
            }
        } else {
            LRESULT(0)
        }
    }

    // 替换窗口过程
    unsafe {
        let hwnd = HWND(hwnd as *mut std::ffi::c_void);
        let original = SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            (custom_wndproc as *const () as usize).cast_signed(),
        );
        let _ = ORIGINAL_WNDPROC.set(original);
        log::info!("Taskbar hook installed");
    }
}

/// 创建共享模式播放器
fn create_shared_mode_player(device: &cpal::Device) -> (rodio::Player, MixerDeviceSink, Option<PlatformPlayer>) {
    log::info!("Starting in shared mode");

    // 从选定的设备创建音频输出流
    let mixer_sink = DeviceSinkBuilder::from_device(device.clone())
        .expect("Failed to create device sink builder")
        .open_stream()
        .expect("Failed to create mixer sink from device");

    let player = rodio::Player::connect_new(mixer_sink.mixer());

    (player, mixer_sink, None)
}
