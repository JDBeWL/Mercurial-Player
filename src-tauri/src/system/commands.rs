//! 系统相关的 Tauri 命令
//!
//! 包含系统信息获取和窗口管理功能。
use crate::error::AppError;

use crate::security::is_simple_filename;
use crate::AppState;
use std::collections::HashMap;
use tauri::{AppHandle, LogicalSize, Manager, Size, State, command};

/// 获取应用版本信息
#[command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 解析便携化数据文件路径(主程序同级 data/ 下,如 config.json、library-cache.json)
///
/// plugin-store 的 JS `load()` 传入绝对路径时会原样使用(基础目录被绝对路径覆盖),
/// 因此前端用本命令解析存储文件完整路径,即可把 store 落到程序目录而非 Roaming。
#[command]
pub fn resolve_data_file(file: String) -> Result<String, AppError> {
    if !is_simple_filename(&file) {
        return Err(AppError::msg("非法的数据文件名"));
    }
    let exe_path =
        std::env::current_exe().map_err(|e| AppError::msg(format!("无法获取可执行文件路径: {e}")))?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::msg("无法获取可执行文件目录"))?;
    let dir = exe_dir.join("data");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(&file).to_string_lossy().to_string())
}

/// 迷你模式窗口尺寸
const MINI_SIZE: LogicalSize<f64> = LogicalSize {
    width: 300.0,
    height: 100.0,
};
/// 默认窗口尺寸
const DEFAULT_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1250.0,
    height: 720.0,
};
/// 最小窗口尺寸
const MIN_SIZE: LogicalSize<f64> = LogicalSize {
    width: 1200.0,
    height: 700.0,
};

/// 获取系统信息
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

/// 获取系统可用的字体列表
#[command]
pub fn get_system_fonts() -> Result<Vec<String>, AppError> {
    // 尝试获取真实的系统字体
    match super::fonts::get_system_fonts() {
        Ok(mut fonts) => {
            // 添加一些通用的 Web 字体作为后备
            let fallback_fonts = vec![
                "system-ui".to_string(),
                "sans-serif".to_string(),
                "serif".to_string(),
                "monospace".to_string(),
            ];

            // 合并并去重
            fonts.extend(fallback_fonts);
            fonts.sort();
            fonts.dedup();

            Ok(fonts)
        }
        Err(e) => {
            // 如果获取系统字体失败，返回一些常见字体作为后备
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

/// 获取软件同级 fonts/ 目录下的外部字体文件列表
#[command]
pub fn get_external_fonts() -> Result<Vec<super::fonts::ExternalFont>, AppError> {
    super::fonts::list_external_fonts()
}

/// 获取字体相关缓存的统计信息（TTC/OTC 提取缓存占用）
#[command]
pub fn get_font_cache_stats() -> super::fonts::FontCacheStats {
    super::fonts::FontCacheStats {
        extract_cache_bytes: super::fonts::font_extract_cache_size(),
    }
}

/// 清理字体缓存：磁盘上的 TTC/OTC 提取缓存 + 桌面歌词的外部字体
/// 内存缓存（名字索引/已加载字体集/文本格式），返回清理后的统计。
/// 提取缓存会在下次扫描 fonts/ 目录时按需重建
#[command]
pub fn clear_font_caches() -> Result<super::fonts::FontCacheStats, AppError> {
    super::fonts::clear_font_extract_cache()?;
    #[cfg(windows)]
    {
        // 桌面歌词未初始化时无内存缓存可清，忽略错误
        let _ = crate::taskbar::desktop_lyrics::invalidate_font_caches();
    }
    Ok(super::fonts::FontCacheStats {
        extract_cache_bytes: super::fonts::font_extract_cache_size(),
    })
}

/// 设置迷你模式
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
    window.set_always_on_top(true).map_err(|e| e.to_string())?;

    Ok(())
}

fn disable_mini_mode(window: &tauri::WebviewWindow) -> Result<(), AppError> {
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
    window.center().map_err(|e| e.to_string())?;

    Ok(())
}

/// 获取当前运行平台
#[command]
#[must_use]
pub const fn get_platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

/// 显示器刷新率信息
#[derive(Debug, serde::Serialize)]
pub struct DisplayRefreshRates {
    /// 窗口当前所在显示器的刷新率
    pub current: u32,
    /// 当前分辨率下该显示器支持的全部刷新率挡位（升序，不超过 MAX_TARGET_FPS）
    pub available: Vec<u32>,
}

/// 默认刷新率（显示器信息不可用时回落）
const DEFAULT_REFRESH_RATE: u32 = 60;

/// 找到窗口中心点所在的显示器；未命中时回落主显示器，再回落第一个显示器
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

fn display_frequency(display: &display_info::DisplayInfo) -> u32 {
    if display.frequency > 0.0 {
        display.frequency.round() as u32
    } else {
        DEFAULT_REFRESH_RATE
    }
}

/// 枚举显示器在当前分辨率下支持的全部刷新率（升序去重）。
/// display_info 0.5 只暴露当前模式，多挡位需按平台 API 自行枚举；
/// 非 Windows 平台暂无现成依赖，回落为仅当前挡位。
#[cfg(windows)]
#[allow(unsafe_code)] // EnumDisplaySettingsExW 是 unsafe Win32 API
fn enumerate_refresh_rates(display: &display_info::DisplayInfo) -> Vec<u32> {
    use std::collections::BTreeSet;
    use windows::Win32::Graphics::Gdi::{
        DEVMODEW, ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsExW,
    };
    use windows::core::PCWSTR;

    let mut device: Vec<u16> = display.name.encode_utf16().collect();
    device.push(0); // 以 NUL 结尾的宽字符串

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

#[cfg(not(windows))]
fn enumerate_refresh_rates(_display: &display_info::DisplayInfo) -> Vec<u32> {
    Vec::new()
}

/// 获取窗口所在显示器的刷新率（跨屏移动窗口后返回值跟随变化）
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

/// 获取窗口所在显示器支持的刷新率挡位（用于目标帧率选项，免去硬编码猜测）
/// available 只保留不超过 MAX_TARGET_FPS 的挡位，与 set_target_fps 的钳制一致；
/// current 始终为屏幕真实刷新率（仅用于展示，不受上限过滤）
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

    // 从配置读取白名单（兼容旧配置）
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
