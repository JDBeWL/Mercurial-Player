//! Tauri 应用启动装配：setup 回调、播放器创建与任务栏钩子。

use crate::{AppState, system};

#[cfg(windows)]
use crate::audio::WasapiExclusivePlayback;
#[cfg(windows)]
use crate::taskbar;

use rodio::stream::DeviceSinkBuilder;

use crate::app_state::{AudioOutput, PlatformPlayer};
use crate::error::AppError;

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
                    log::info!(
                        "Migrated {name} from {} to {}",
                        src.display(),
                        dst.display()
                    );
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

    // Android：缓存与配置文件迁移到应用沙箱目录
    // 桌面端默认用系统临时目录 + 主程序同级 data/（免安装包体积、便携化）；
    // Android 的 /tmp 多数情况不可写、current_exe() 在只读 APK 内，统一收敛到 app 数据目录。
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        // JNI 反向调用（通知栏 / MediaSession / 耳机线控）需要进程级 AppHandle
        crate::android::set_app_handle(app.handle());
        if let Ok(data_dir) = app.path().app_data_dir() {
            // config.json / library-cache.json 改写到 <app_data>/data
            crate::config::set_data_dir_override(data_dir.clone());
            log::info!("Android data dir override: {}", data_dir.display());
        }
        if let Ok(cache_dir) = app.path().app_cache_dir() {
            let media_cache = cache_dir.join("mercurial-player");
            if let Some(parent) = media_cache.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn!("创建 Android 缓存目录失败: {e}");
                }
            }
            // 元数据缓存 + 封面缓存统一走自定义缓存路径（metadata_cache_path/cover_cache_dir 优先读它）
            if let Err(e) = crate::media::metadata::set_cover_cache_path(Some(
                media_cache.to_string_lossy().to_string(),
            )) {
                log::warn!("设置 Android 缓存路径失败: {e}");
            }
            // 封面临入经 asset 协议提供前端，须放开访问范围
            if let Err(e) = app
                .asset_protocol_scope()
                .allow_directory(&media_cache, true)
            {
                log::warn!("放行 Android 缓存目录到 asset 范围失败: {e}");
            }
        }
    }

    // 启动设备监听器（Android 上为降级 no-op，见 device_monitor.rs）
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
        use crate::media::metadata;
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

/// 创建独占模式播放器（Android：AAudio 独占 / USB DAC 位完美）
/// AAudio 要的是系统设备 id（原因见 [`crate::audio::aaudio::device`]），故让 `initialize` 自己找当前 USB 设备。
#[cfg(target_os = "android")]
pub fn create_exclusive_mode_player(_device_name: &str) -> Result<AudioOutput, AppError> {
    // 共享模式的 sink 照旧创建：USB 拔出后要能立刻回落到它，
    // 否则设置页关掉开关时会出现"没有播放器可用"的空窗
    let mixer_sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("Failed to create default device sink builder: {e}"))?
        .open_stream()
        .map_err(|e| format!("Failed to create default mixer sink: {e}"))?;
    let player = rodio::Player::connect_new(mixer_sink.mixer());

    let aaudio = crate::audio::aaudio::AaudioExclusivePlayer::new();
    match aaudio.initialize(None) {
        Ok((sample_rate, channels, device)) => {
            log::info!("AAudio 独占就绪: {device} @ {sample_rate}Hz, {channels} ch");
            Ok(AudioOutput {
                sink: player,
                mixer_sink,
                wasapi_player: Some(aaudio),
            })
        }
        Err(e) => {
            // 没有 USB DAC（或设备被占用）时不能让应用起不来：回落共享模式，
            // 由设置页把"未检测到 USB 音频设备"如实显示出来
            log::warn!("AAudio 独占初始化失败，回落共享模式: {e}");
            Ok(AudioOutput {
                sink: player,
                mixer_sink,
                wasapi_player: None,
            })
        }
    }
}

/// 创建独占模式播放器（非 Windows / 非 Android 平台回退到共享模式）
#[cfg(not(any(windows, target_os = "android")))]
pub fn create_exclusive_mode_player(_device_name: &str) -> Result<AudioOutput, AppError> {
    log::warn!(
        "Exclusive mode is only supported on Windows / Android, falling back to shared mode"
    );
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
