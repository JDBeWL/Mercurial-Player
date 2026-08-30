//! Tauri 应用启动装配
//!
//! 包含 setup 回调、播放器创建与任务栏钩子,保持 main() 精简。

use mercurial_player::{AppState, system};

#[cfg(windows)]
use mercurial_player::audio::WasapiExclusivePlayback;
#[cfg(windows)]
use mercurial_player::taskbar;

use rodio::stream::DeviceSinkBuilder;

use crate::app_state::{AudioOutput, PlatformPlayer};
use mercurial_player::error::AppError;

/// Tauri setup 回调主体
pub fn init(app: &tauri::App) {
    use tauri::Manager;

    // 轮转前端日志:上一轮运行的 mercurial-player.log → -prev.log
    system::logging::init_log_rotation();

    // 一次性迁移:把旧版 Roaming 目录下的 store 文件搬到主程序同级 data/
    // (目标已存在时跳过,避免用旧数据覆盖新数据)
    {
        use tauri::Manager;
        let migrate = |name: &str| -> bool {
            let Ok(app_data) = app.path().app_data_dir() else {
                return false;
            };
            let src = app_data.join(name);
            if !src.exists() {
                return false;
            }
            let dst = match std::env::current_exe() {
                Ok(p) => match p.parent() {
                    Some(dir) => dir.join("data").join(name),
                    None => return false,
                },
                Err(_) => return false,
            };
            if dst.exists() {
                return false;
            }
            if let Some(parent) = dst.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn!("Failed to create data dir for {name}: {e}");
                    return false;
                }
            }
            match std::fs::copy(&src, &dst) {
                Ok(_) => {
                    log::info!("Migrated {name} from {} to {}", src.display(), dst.display());
                    true
                }
                Err(e) => {
                    log::warn!("Failed to migrate {name}: {e}");
                    false
                }
            }
        };
        migrate("config.json");
        migrate("library-cache.json");
    }

    #[cfg(debug_assertions)]
    {
        let window = app.get_webview_window("main").unwrap();
        window.open_devtools();
    }

    // 放开外部字体目录的 asset 协议访问（软件同级 fonts/，供前端动态注册 @font-face）
    if let Ok(fonts_dir) = system::fonts::get_external_fonts_dir() {
        if let Err(e) = app.asset_protocol_scope().allow_directory(fonts_dir, true) {
            log::warn!("Failed to allow fonts dir in asset scope: {e}");
        }
    }

    // 字体集合（TTC/OTC）成员提取缓存目录同样经 asset 协议提供给前端
    if let Ok(extract_dir) = system::fonts::get_font_extract_cache_dir() {
        if let Err(e) = app
            .asset_protocol_scope()
            .allow_directory(extract_dir, true)
        {
            log::warn!("Failed to allow font extract cache dir in asset scope: {e}");
        }
    }

    // 启动设备监听器
    {
        let state: tauri::State<AppState> = app.state();
        // 锁中毒时自动恢复而非 panic
        let mut monitor = match state.player.device_monitor.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("锁中毒, 自动恢复: {poisoned}");
                poisoned.into_inner()
            }
        };
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
                    log::info!("应用启动时清理了 {count} 个封面缓存文件");
                }
            }
            Err(e) => {
                log::warn!("清理封面缓存失败: {e}");
            }
        }
    }

    // 初始化Windows任务栏缩略图工具栏
    #[cfg(windows)]
    {
        let Some(window) = app.get_webview_window("main") else {
            log::error!("Main window not found, skipping taskbar initialization");
            return;
        };
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
}

/// 创建独占模式播放器
#[cfg(windows)]
pub fn create_exclusive_mode_player(device_name: &str) -> Result<AudioOutput, AppError> {
    log::info!("Starting in WASAPI exclusive mode");

    // 创建一个空的rodio sink
    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("Failed to create default device sink builder: {e}"))?
        .open_stream()
        .map_err(|e| format!("Failed to create default mixer sink: {e}"))?;
    let player = rodio::Player::connect_new(mixer_sink.mixer());

    // 创建 WASAPI 独占播放器
    let wasapi_playback = WasapiExclusivePlayback::new();
    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_name)) => {
            log::info!(
                "WASAPI Exclusive initialized: {actual_name} @ {sample_rate}Hz, {channels} channels"
            );
            Ok(AudioOutput {
                sink: player,
                mixer_sink,
                wasapi_player: Some(wasapi_playback),
            })
        }
        Err(e) => {
            log::error!("Failed to initialize WASAPI exclusive mode: {e}");
            log::warn!("Falling back to shared mode");
            Ok(AudioOutput {
                sink: player,
                mixer_sink,
                wasapi_player: None,
            })
        }
    }
}

/// 创建独占模式播放器（非Windows平台回退到共享模式）
#[cfg(not(windows))]
pub fn create_exclusive_mode_player(_device_name: &str) -> Result<AudioOutput, AppError> {
    log::warn!("Exclusive mode is only supported on Windows, falling back to shared mode");
    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("Failed to create default device sink builder: {e}"))?
        .open_stream()
        .map_err(|e| format!("Failed to create default mixer sink: {e}"))?;
    let player = rodio::Player::connect_new(mixer_sink.mixer());
    Ok(AudioOutput {
        sink: player,
        mixer_sink,
        wasapi_player: None,
    })
}

/// 创建共享模式播放器
pub fn create_shared_mode_player(device: &cpal::Device) -> Result<AudioOutput, AppError> {
    log::info!("Starting in shared mode");

    // 从选定的设备创建音频输出流
    let mixer_sink = DeviceSinkBuilder::from_device(device.clone())
        .map_err(|e| format!("Failed to create device sink builder: {e}"))?
        .open_stream()
        .map_err(|e| format!("Failed to create mixer sink from device: {e}"))?;

    let player = rodio::Player::connect_new(mixer_sink.mixer());

    Ok(AudioOutput {
        sink: player,
        mixer_sink,
        wasapi_player: Option::<PlatformPlayer>::None,
    })
}

/// 设置任务栏按钮点击钩子
#[cfg(windows)]
#[allow(unsafe_code)] // Windows API交互需要unsafe
fn setup_taskbar_hook(hwnd: isize, app_handle: tauri::AppHandle) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, SetWindowLongPtrW, WM_COMMAND, WNDPROC,
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
