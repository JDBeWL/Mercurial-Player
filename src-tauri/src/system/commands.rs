//! 系统相关的 Tauri 命令：应用与系统信息、mini 模式、字体列表与缓存、刷新率、外链白名单。
use crate::error::AppError;

use crate::AppState;
use crate::security::is_simple_filename;
use std::collections::HashMap;
use tauri::{AppHandle, LogicalSize, Manager, Size, State, command};

#[command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 解析便携化数据文件路径（主程序同级 `data/` 下）。
/// plugin-store 的 JS `load()` 会把绝对路径原样使用，前端因此可把 store 落到程序目录而非 Roaming。
#[command]
pub fn resolve_data_file(file: String) -> Result<String, AppError> {
    if !is_simple_filename(&file) {
        return Err(AppError::msg("非法的数据文件名"));
    }
    // Android 覆盖：current_exe() 在只读 APK 内，改写应用数据目录
    let dir = if let Some(override_dir) = crate::config::data_dir_override() {
        override_dir.join("data")
    } else {
        let exe_path = std::env::current_exe()
            .map_err(|e| AppError::msg(format!("无法获取可执行文件路径: {e}")))?;
        let exe_dir = exe_path
            .parent()
            .ok_or_else(|| AppError::msg("无法获取可执行文件目录"))?;
        exe_dir.join("data")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(&file).to_string_lossy().to_string())
}

/// 解析缓存文件路径（缓存存储路径下，见 `metadata::cache::cache_dir`）。
///
/// 曲库扫描缓存等"可随时丢弃重建"的文件走这里；config、上次播放会话等用户数据
/// 仍走 `resolve_data_file`（数据目录），不随缓存路径切换。
/// 新调用方必须把文件名登记进 `metadata::cache::CACHE_ARTIFACT_FILES`，
/// 否则切换缓存路径时该文件不会被迁移到新目录。
#[command]
pub fn resolve_cache_file(file: String) -> Result<String, AppError> {
    if !is_simple_filename(&file) {
        return Err(AppError::msg("非法的缓存文件名"));
    }
    let dir = crate::media::metadata::cache_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(&file).to_string_lossy().to_string())
}

/// 迷你模式窗口尺寸
const MINI_SIZE: LogicalSize<f64> = LogicalSize {
    width: 300.0,
    height: 100.0,
};
const DEFAULT_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1250.0,
    height: 720.0,
};
const MIN_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1200.0,
    height: 700.0,
};

#[command]
pub fn get_system_info() -> Result<HashMap<String, String>, AppError> {
    let mut info = HashMap::new();

    info.insert("os".to_string(), std::env::consts::OS.to_string());
    info.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    info.insert("family".to_string(), std::env::consts::FAMILY.to_string());

    if let Some(music_dir) = dirs::audio_dir() {
        info.insert(
            "music_dir".to_string(),
            music_dir.to_string_lossy().to_string(),
        );
    }

    Ok(info)
}

#[command]
pub fn get_system_fonts() -> Result<Vec<String>, AppError> {
    match super::fonts::get_system_fonts() {
        Ok(mut fonts) => {
            let fallback_fonts = vec![
                "system-ui".to_string(),
                "sans-serif".to_string(),
                "serif".to_string(),
                "monospace".to_string(),
            ];

            fonts.extend(fallback_fonts);
            fonts.sort();
            fonts.dedup();

            Ok(fonts)
        }
        Err(e) => {
            log::warn!("Failed to get system fonts: {e}");
            Ok(vec![
                "system-ui".to_string(),
                "sans-serif".to_string(),
                "serif".to_string(),
                "monospace".to_string(),
                "Arial".to_string(),
                "Helvetica".to_string(),
                "Times New Roman".to_string(),
                "Courier New".to_string(),
                "Verdana".to_string(),
                "Georgia".to_string(),
                "Palatino".to_string(),
                "Garamond".to_string(),
                "Comic Sans MS".to_string(),
                "Trebuchet MS".to_string(),
                "Impact".to_string(),
            ])
        }
    }
}

#[command]
pub fn get_external_fonts() -> Result<Vec<super::fonts::ExternalFont>, AppError> {
    super::fonts::list_external_fonts()
}

#[command]
pub fn get_font_cache_stats() -> super::fonts::FontCacheStats {
    super::fonts::FontCacheStats {
        extract_cache_bytes: super::fonts::font_extract_cache_size(),
    }
}

/// 清理 TTC/OTC 提取缓存与桌面歌词字体内存缓存；提取缓存会在下次扫描 `fonts/` 时重建。
#[command]
pub fn clear_font_caches() -> Result<super::fonts::FontCacheStats, AppError> {
    super::fonts::clear_font_extract_cache()?;
    #[cfg(windows)]
    {
        // 桌面歌词未初始化时无内存缓存可清，忽略错误
        let _ = crate::desktop_lyrics::invalidate_font_caches();
    }
    Ok(super::fonts::FontCacheStats {
        extract_cache_bytes: super::fonts::font_extract_cache_size(),
    })
}

#[command]
pub fn set_mini_mode(app_handle: AppHandle, enable: bool) -> Result<(), AppError> {
    let window = app_handle
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    if enable {
        enable_mini_mode(&window)?;
    } else {
        disable_mini_mode(&window)?;
    }

    Ok(())
}

fn enable_mini_mode(window: &tauri::WebviewWindow) -> Result<(), AppError> {
    let mini_size = Size::Logical(MINI_SIZE);

    window
        .set_min_size(Some(mini_size))
        .map_err(|e| e.to_string())?;
    window
        .set_max_size(Some(mini_size))
        .map_err(|e| e.to_string())?;
    window.set_size(mini_size).map_err(|e| e.to_string())?;
    window.set_resizable(false).map_err(|e| e.to_string())?;
    // Windows/macOS/Linux 才有置顶语义；移动端窗口无此概念
    #[cfg(desktop)]
    window.set_always_on_top(true).map_err(|e| e.to_string())?;

    Ok(())
}

fn disable_mini_mode(window: &tauri::WebviewWindow) -> Result<(), AppError> {
    #[cfg(desktop)]
    window.set_always_on_top(false).map_err(|e| e.to_string())?;
    window.set_resizable(true).map_err(|e| e.to_string())?;
    window
        .set_max_size(None::<Size>)
        .map_err(|e| e.to_string())?;
    window
        .set_size(Size::Logical(DEFAULT_SIZE))
        .map_err(|e| e.to_string())?;
    window
        .set_min_size(Some(Size::Logical(MIN_SIZE)))
        .map_err(|e| e.to_string())?;
    #[cfg(desktop)]
    window.center().map_err(|e| e.to_string())?;

    Ok(())
}

#[command]
#[must_use]
pub const fn get_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "unknown"
    }
}

/// 设置界面字号倍率（`1.0` = 设计稿原始大小）。
/// Android 把 WebView 的 `textZoom` 写成 `100 * 倍率` 以覆盖系统字号；桌面端为 no-op。
#[command]
pub fn set_app_font_scale(scale: f32) -> Result<(), AppError> {
    crate::android::font_scale::set_app_font_scale(scale)
}

/// 隐藏/恢复 Android 系统栏（状态栏、导航栏）；桌面端为 no-op。
#[command]
pub fn set_system_ui_hidden(
    hide_status_bars: bool,
    hide_navigation_bars: bool,
) -> Result<(), AppError> {
    crate::android::system_ui::set_system_ui_hidden(hide_status_bars, hide_navigation_bars)
}

#[cfg(desktop)]
#[derive(Debug, serde::Serialize)]
pub struct DisplayRefreshRates {
    /// 窗口当前所在显示器的刷新率
    pub current: u32,
    /// 当前分辨率下支持的全部刷新率挡位（升序，不超过 `MAX_TARGET_FPS`）
    pub available: Vec<u32>,
}

#[cfg(desktop)]
const DEFAULT_REFRESH_RATE: u32 = 60;

/// 找到窗口中心点所在的显示器；未命中时回落主显示器，再回落第一个显示器
#[cfg(desktop)]
fn find_window_display(window: &tauri::WebviewWindow) -> Option<display_info::DisplayInfo> {
    let displays = display_info::DisplayInfo::all().ok()?;
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let cx = pos.x + size.width as i32 / 2;
    let cy = pos.y + size.height as i32 / 2;
    displays
        .iter()
        .find(|d| cx >= d.x && cx < d.x + d.width as i32 && cy >= d.y && cy < d.y + d.height as i32)
        .or_else(|| displays.iter().find(|d| d.is_primary))
        .or_else(|| displays.first())
        .cloned()
}

#[cfg(desktop)]
fn display_frequency(display: &display_info::DisplayInfo) -> u32 {
    if display.frequency > 0.0 {
        display.frequency.round() as u32
    } else {
        DEFAULT_REFRESH_RATE
    }
}

/// 枚举当前分辨率下支持的刷新率（升序去重）；display_info 0.5 只暴露当前模式。
#[cfg(desktop)]
#[cfg(windows)]
#[allow(unsafe_code)]
fn enumerate_refresh_rates(display: &display_info::DisplayInfo) -> Vec<u32> {
    use std::collections::BTreeSet;
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsExW,
    };
    use windows::core::PCWSTR;

    let mut device: Vec<u16> = display.name.encode_utf16().collect();
    device.push(0);

    let mut rates = BTreeSet::new();
    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..DEVMODEW::default()
    };
    for index in 0..4096_u32 {
        let ok = unsafe {
            EnumDisplaySettingsExW(
                PCWSTR(device.as_ptr()),
                ENUM_DISPLAY_SETTINGS_MODE(index),
                std::ptr::addr_of_mut!(mode),
                ENUM_DISPLAY_SETTINGS_FLAGS(0),
            )
        }
        .as_bool();
        if !ok {
            break;
        }
        // 只保留当前分辨率下的挡位，避免混入其他分辨率的模式
        if mode.dmPelsWidth == display.width
            && mode.dmPelsHeight == display.height
            && mode.dmDisplayFrequency > 0
            && mode.dmDisplayFrequency != 1
        // 1Hz 是驱动用于"未指定"的哨兵值
        {
            rates.insert(mode.dmDisplayFrequency);
        }
    }
    rates.into_iter().collect()
}

/// 非 Windows 没有现成的枚举依赖，返回空列表，由调用方补上当前挡位
#[cfg(desktop)]
#[cfg(not(windows))]
fn enumerate_refresh_rates(_display: &display_info::DisplayInfo) -> Vec<u32> {
    Vec::new()
}

/// 获取窗口所在显示器的刷新率（跨屏移动窗口后返回值跟随变化）
#[cfg(desktop)]
#[command]
pub fn get_screen_refresh_rate(window: tauri::WebviewWindow) -> Result<u32, AppError> {
    if let Some(display) = find_window_display(&window) {
        let rate = display_frequency(&display);
        log::info!("Detected screen refresh rate: {rate} Hz");
        Ok(rate)
    } else {
        log::info!("No display info available, using default {DEFAULT_REFRESH_RATE} Hz");
        Ok(DEFAULT_REFRESH_RATE)
    }
}

/// 获取窗口所在显示器支持的刷新率挡位；`available` 只保留不超过 `MAX_TARGET_FPS` 的挡位
/// （与 `set_target_fps` 钳制一致），`current` 为真实刷新率、不受上限过滤。
#[cfg(desktop)]
#[command]
pub fn get_display_refresh_rates(
    window: tauri::WebviewWindow,
) -> Result<DisplayRefreshRates, AppError> {
    match find_window_display(&window) {
        Some(display) => {
            let current = display_frequency(&display);
            let mut available = enumerate_refresh_rates(&display);
            if !available.contains(&current) {
                available.push(current);
            }
            available.sort_unstable();
            available.dedup();
            available.retain(|&rate| rate <= crate::audio::commands::MAX_TARGET_FPS);
            if available.is_empty() {
                // 高刷屏挡位全被过滤时回落到上限值
                available.push(current.min(crate::audio::commands::MAX_TARGET_FPS));
            }
            log::info!(
                "Display {} refresh rates: current {current} Hz, available {:?}",
                display.name,
                available
            );
            Ok(DisplayRefreshRates { current, available })
        }
        None => Ok(DisplayRefreshRates {
            current: DEFAULT_REFRESH_RATE,
            available: vec![DEFAULT_REFRESH_RATE],
        }),
    }
}

/// 安全打开外部链接（仅允许 HTTPS 且主机在白名单内）
#[command]
pub fn open_external_url(state: State<AppState>, url: String) -> Result<(), AppError> {
    let parsed = tauri_plugin_http::reqwest::Url::parse(&url)
        .map_err(|e| AppError::msg(format!("Invalid URL: {e}")))?;

    if parsed.scheme() != "https" {
        return Err(AppError::msg("Only HTTPS URLs are allowed"));
    }

    let host = parsed
        .host_str()
        .ok_or("URL host is missing")?
        .to_ascii_lowercase();

    // 读取配置白名单；配置读取失败时使用内置列表
    let allowed_hosts = state
        .config_manager
        .load_config()
        .map(|cfg| cfg.general.external_url_allowed_hosts)
        .unwrap_or_else(|_| {
            vec![
                "github.com".to_string(),
                "github.io".to_string(),
                "tauri.app".to_string(),
                "vuejs.org".to_string(),
                "intlify.dev".to_string(),
                "docs.rs".to_string(),
                "gnu.org".to_string(),
                "vitejs.dev".to_string(),
                "typescriptlang.org".to_string(),
                "vitest.dev".to_string(),
            ]
        });

    if !allowed_hosts.iter().any(|allowed| {
        host.eq_ignore_ascii_case(allowed) || host.ends_with(&format!(".{allowed}").to_lowercase())
    }) {
        return Err(AppError::msg(format!("Host not allowed: {host}")));
    }

    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|e| format!("Failed to open URL: {e}"))?;

    Ok(())
}
