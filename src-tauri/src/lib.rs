//! Mercurial Player 库入口：模块导出、共享状态定义与 `run()`。

/// 取锁；中毒时记录 error 后继续使用其中的数据。
///
/// 适用于 `Mutex::lock()`、`RwLock::read()`、`RwLock::write()`，三者均返回
/// `Result<T, PoisonError<T>>`。release 构建是 `panic = "abort"`，进程不会带着中毒锁
/// 继续跑，所以这条分支只可能在 dev 构建命中——命中即说明另有线程 panic 过，数据一致性
/// 已无从保证，因此按 error 记录而不是声称"已自动恢复"。
/// 命令边界别用它，改用 [`LockOrErr`] 把失败如实返回给前端。
macro_rules! lock_or_log {
    ($lock:expr) => {
        match $lock {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::error!("锁中毒(其它线程曾 panic),继续使用其数据: {poisoned}");
                poisoned.into_inner()
            }
        }
    };
}

pub mod android;
pub mod audio;
pub mod config;
pub mod equalizer;
pub mod error;
pub mod http_client;
pub mod lyrics;
pub mod media;
pub mod plugins;
pub mod security;
pub mod system;

mod app_setup;
mod app_state;

// 更新器依赖桌面安装流程，移动端未验证前不参与编译
#[cfg(desktop)]
pub mod updater;

// 桌面歌词悬浮窗（Direct2D 渲染，仅 Windows）
#[cfg(windows)]
pub mod desktop_lyrics;

#[cfg(windows)]
pub mod taskbar;

// 独占播放器的平台别名：Windows = WASAPI，Android = AAudio，其余为占位实现
use crate::app_state::PlatformPlayer;

use audio::{DeviceMonitor, PlaybackQueue};
use config::ConfigManager;
use equalizer::GlobalEqualizer;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

/// 无独占能力平台（非 Windows、非 Android）的占位类型
#[cfg(not(any(windows, target_os = "android")))]
#[derive(Debug)]
pub struct Placeholder;

#[cfg(not(any(windows, target_os = "android")))]
impl Placeholder {
    /// 占位方法：没有独占能力的平台上调用独占接口时返回错误
    pub fn stop(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn clear_buffer(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause_no_fade(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume_no_fade(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn stop_with_fade_out(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause_with_fade_out(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume_with_fade_in(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }
}

/// 音频输出相关状态
///
/// 包含音频 sink、输出流、音量、设备、独占模式等与音频输出直接相关的字段
pub struct AudioOutputState {
    /// 音频输出 sink
    pub sink: Arc<Mutex<rodio::Player>>,
    /// 音频输出流（必须长期持有，否则会静音）
    pub output_stream: Arc<Mutex<Option<rodio::MixerDeviceSink>>>,
    /// 目标音量
    pub target_volume: Arc<Mutex<f32>>,
    /// 当前音频设备名称
    pub current_device_name: Arc<Mutex<String>>,
    /// 是否启用独占模式
    pub exclusive_mode: Arc<Mutex<bool>>,
    /// 独占/直出播放器：字段名沿用早期只有 WASAPI 时的叫法，实际装的是 [`crate::app_state::PlatformPlayer`]
    pub wasapi_player: Arc<Mutex<Option<PlatformPlayer>>>,
}

/// 当前播放曲目状态
pub struct TrackState {
    /// 当前播放文件路径
    pub current_path: Arc<Mutex<Option<String>>>,
}

/// 可视化相关状态
pub struct VisualizationState {
    /// 频谱数据（用于可视化）
    pub spectrum_data: Arc<Mutex<Vec<f32>>>,
    /// 目标刷新率（用于可视化FFT计算，默认60fps）
    pub target_fps: Arc<AtomicU64>,
    /// 频谱计算门控：可视化面板在屏、且应用在前台才算 FFT 并发送事件。
    /// 面板是 `spectrum-update` 唯一的订阅者，两者任一不成立时算出来都没人消费。
    pub spectrum_gate: Arc<audio::spectrum::SpectrumGate>,
}

/// 解码线程管理
pub struct DecodeThreadState {
    /// 解码线程代际计数器(每次切歌递增,旧线程检测到变化即退出)
    pub generation: Arc<AtomicU64>,
    /// 当前解码线程 ID（用于区分不同的播放会话）
    pub id: Arc<AtomicU64>,
}

/// 淡入淡出控制
///
/// generation 用于取消陈旧的 fade 线程;
/// enabled 控制是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
pub struct FadeControl {
    /// 共享模式淡入淡出代际计数器(每次新的 fade 操作递增,用于取消陈旧的 fade 线程)
    pub generation: Arc<AtomicU32>,
    /// 是否启用淡入淡出(运行时读取,避免每次访问配置文件)
    pub enabled: Arc<AtomicBool>,
}

/// 播放器状态，按职责域分组
pub struct PlayerState {
    /// 音频输出 (sink/流/音量/设备/独占模式/独占播放器)
    pub output: AudioOutputState,
    /// 当前曲目路径
    pub track: TrackState,
    /// 频谱数据与目标帧率
    pub visualization: VisualizationState,
    /// 解码线程管理
    pub decode: DecodeThreadState,
    /// 设备监听器
    pub device_monitor: Arc<Mutex<DeviceMonitor>>,
    /// 播放队列（Android 后台自动切歌；桌面端不启用自动推进）
    pub queue: Arc<Mutex<PlaybackQueue>>,
    /// 淡入淡出控制
    pub fade: FadeControl,
}

/// 应用全局状态
pub struct AppState {
    /// 播放器状态
    pub player: PlayerState,
    /// 配置管理器
    pub config_manager: ConfigManager,
    /// 全局均衡器
    pub equalizer: GlobalEqualizer,
}

use cpal::traits::HostTrait;

/// 应用启动入口。
///
/// 桌面由 `main.rs` 调用，移动端由 `mobile_entry_point` 生成的 JNI `Rust.create()` 调用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Android：current_exe() 位于只读 APK 内，必须在首个 ConfigManager 创建前
    // 把数据目录 override 指向应用沙箱（config.json 等落盘依赖它）
    #[cfg(target_os = "android")]
    {
        match android::saf::get_app_data_dir() {
            Ok(Some(dir)) if !dir.is_empty() => {
                if config::set_data_dir_override(std::path::PathBuf::from(&dir)) {
                    log::info!("Android data dir override (early): {dir}");
                }
            }
            other => log::warn!("Failed to resolve Android app data dir early: {other:?}"),
        }
    }

    // 创建配置管理器并加载配置(独占模式 / 淡入淡出 / 记忆的输出设备都来自这里)
    let config_manager = ConfigManager::new();

    // 初始化配置文件
    if let Err(e) = config_manager.initialize_config_files() {
        log::error!("Failed to initialize config files: {e}");
    }

    let config = match config_manager.load_config() {
        Ok(config) => Some(config),
        Err(e) => {
            log::warn!("读取配置失败，本次启动按默认设置: {e}");
            None
        }
    };
    let fade_enabled = config
        .as_ref()
        .map(|c| c.audio.fade_enabled)
        .unwrap_or(true);

    // Android 上独占开关是 `usb_dac_exclusive`（必须插着 USB DAC 才有意义），
    // 桌面端沿用 `exclusive_mode`；这里合并成一个启动期判定。
    #[cfg(target_os = "android")]
    let exclusive_mode_enabled = config
        .as_ref()
        .map(|c| c.audio.usb_dac_exclusive && audio::aaudio::usb_dac_available().unwrap_or(false))
        .unwrap_or(false);
    #[cfg(not(target_os = "android"))]
    let exclusive_mode_enabled = config
        .as_ref()
        .map(|c| c.audio.exclusive_mode)
        .unwrap_or(false);

    log::info!(
        "Loaded exclusive mode from config: {exclusive_mode_enabled}, fade enabled: {fade_enabled}"
    );

    // 初始化 cpal host,并解析实际使用的输出设备:
    // 用户曾在设置页手动选择过设备时,用落盘的平台原生标识恢复(设备仍在线才生效),
    // 否则跟随系统默认输出设备。标识各平台不同(WASAPI endpoint ID / CoreAudio
    // DeviceUID / ALSA PCM 名),由 audio::device 统一解析。
    let host = cpal::default_host();
    let preferred_id = config
        .as_ref()
        .and_then(|c| c.audio.preferred_device_id.clone());

    let restored = preferred_id
        .as_deref()
        .and_then(audio::device::resolve_preferred_device);

    let (device, device_name) = if let Some(dev) = restored {
        let name = audio::device::get_device_friendly_name(&dev)
            .unwrap_or_else(|| "Unknown Device".to_string());
        log::info!("Restoring preferred audio device from config: {name}");
        (dev, name)
    } else {
        if let Some(id) = preferred_id.as_deref() {
            log::info!("Preferred audio device '{id}' is not available, using default device");
        }
        let dev = host.default_output_device().unwrap_or_else(|| {
            log::error!("No default output device available");
            eprintln!("错误: 未检测到可用的音频输出设备,应用无法启动。");
            std::process::exit(1);
        });
        let name = audio::device::get_device_friendly_name(&dev)
            .unwrap_or_else(|| "Unknown Device".to_string());
        (dev, name)
    };

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

    let builder = tauri::Builder::default()
        .manage(app_state)
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
        .plugin(tauri_plugin_store::Builder::new().build());

    // 桌面端专属插件：global-shortcut 官方不支持 Android/iOS（前端也只在桌面注册），
    // updater 的移动端安装流程未验证
    #[cfg(desktop)]
    let builder = builder.manage(updater::PendingUpdate::new());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    builder
        .invoke_handler(tauri::generate_handler![
            // 文件系统命令
            media::commands::read_directory,
            media::commands::get_audio_files,
            media::commands::read_lyrics_file,
            media::commands::write_lyrics_file,
            media::commands::get_all_audio_files,
            media::commands::check_file_exists,
            // 音乐目录命令
            config::commands::add_music_directory,
            config::commands::remove_music_directory,
            config::commands::set_music_directories,
            config::commands::get_music_directories,
            // Android SAF 目录授权命令（桌面端为 no-op / None）
            media::commands::saf_request_pick,
            media::commands::saf_get_saved_tree,
            media::commands::saf_get_pick_state,
            media::commands::saf_clear_saved_tree,
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
            media::commands::flush_metadata_cache_command,
            // 在线歌词命令
            lyrics::commands::netease_search_songs,
            lyrics::commands::netease_get_lyrics,
            lyrics::commands::lyrics_search_candidates,
            // 播放命令
            audio::commands::play_track,
            audio::commands::pause_track,
            audio::commands::resume_track,
            audio::commands::set_volume,
            audio::commands::seek_track,
            // 播放队列与媒体控制（Android 后台播放；桌面端 auto_advance=false 时不改变行为）
            audio::commands::set_play_queue,
            audio::commands::media_control,
            audio::commands::background_heartbeat,
            // Android：USB DAC 独占（位完美）输出
            audio::commands::get_audio_route,
            audio::commands::set_usb_dac_exclusive,
            // 配置命令
            config::commands::load_config,
            config::commands::save_config,
            config::commands::export_config,
            config::commands::import_config,
            // 系统命令
            system::commands::get_system_info,
            system::commands::get_system_fonts,
            system::commands::get_external_fonts,
            system::commands::get_font_cache_stats,
            system::commands::clear_font_caches,
            system::commands::get_platform,
            // 应用内界面字号（Android 走 WebView textZoom；桌面端 no-op）
            system::commands::set_app_font_scale,
            system::commands::set_system_ui_hidden,
            // 显示器刷新率查询依赖 display-info（经 wayland 依赖链），仅桌面端可用
            #[cfg(desktop)]
            system::commands::get_screen_refresh_rate,
            #[cfg(desktop)]
            system::commands::get_display_refresh_rates,
            system::commands::open_external_url,
            // 音频设备命令
            audio::commands::get_audio_devices,
            audio::commands::set_audio_device,
            audio::commands::get_current_audio_device,
            audio::commands::toggle_exclusive_mode,
            audio::commands::get_exclusive_mode,
            audio::commands::set_target_fps,
            audio::commands::set_visualizer_visible,
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
            plugins::commands::uninstall_plugin,
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
            desktop_lyrics::show_desktop_lyrics,
            #[cfg(windows)]
            desktop_lyrics::hide_desktop_lyrics,
            #[cfg(windows)]
            desktop_lyrics::update_desktop_lyric,
            #[cfg(windows)]
            desktop_lyrics::set_desktop_lyrics_locked,
            #[cfg(windows)]
            desktop_lyrics::set_desktop_lyrics_font_size,
            #[cfg(windows)]
            desktop_lyrics::set_desktop_lyrics_font_family,
            #[cfg(windows)]
            desktop_lyrics::set_desktop_lyrics_color_preset,
            // 前端日志落盘
            system::logging::write_log,
            // 系统版本命令（保留 get_app_version 用于前端显示）
            system::commands::get_app_version,
            // 便携化数据文件路径（config.json / library-cache.json 存放位置）
            system::commands::resolve_data_file,
            // 应用更新命令（多线程分片下载，桌面端）
            #[cfg(desktop)]
            updater::updater_check,
            #[cfg(desktop)]
            updater::updater_download,
            #[cfg(desktop)]
            updater::updater_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
