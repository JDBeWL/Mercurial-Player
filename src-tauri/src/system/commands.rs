//! 系统相关的 Tauri 命令
//!
//! 包含系统信息获取和窗口管理功能。

use std::collections::HashMap;
use tauri::{command, AppHandle, LogicalSize, Manager, Size};

/// 迷你模式窗口尺寸
const MINI_SIZE: LogicalSize<f64> = LogicalSize { width: 300.0, height: 100.0 };
/// 默认窗口尺寸
const DEFAULT_SIZE: LogicalSize<f64> = LogicalSize { width: 1250.0, height: 720.0 };
/// 最小窗口尺寸
const MIN_SIZE: LogicalSize<f64> = LogicalSize { width: 1200.0, height: 700.0 };

/// 获取系统信息
#[command]
pub fn get_system_info() -> Result<HashMap<String, String>, String> {
    let mut info = HashMap::new();

    info.insert("os".to_string(), std::env::consts::OS.to_string());
    info.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    info.insert("family".to_string(), std::env::consts::FAMILY.to_string());

    if let Some(music_dir) = dirs::audio_dir() {
        info.insert("music_dir".to_string(), music_dir.to_string_lossy().to_string());
    }

    Ok(info)
}

/// 获取系统可用的字体列表
#[command]
pub fn get_system_fonts() -> Result<Vec<String>, String> {
    let mut fonts = vec![
        "system-ui".to_string(),
        "Roboto".to_string(),
        "Arial".to_string(),
        "Helvetica".to_string(),
        "Times New Roman".to_string(),
        "Noto Sans".to_string(),
        "Segoe UI".to_string(),
        "PingFang SC".to_string(),
        "Microsoft YaHei".to_string(),
        "Consolas".to_string(),
    ];

    fonts.sort();
    fonts.dedup();

    Ok(fonts)
}

/// 设置迷你模式
#[command]
pub async fn set_mini_mode(app_handle: AppHandle, enable: bool) -> Result<(), String> {
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

fn enable_mini_mode(window: &tauri::WebviewWindow) -> Result<(), String> {
    let mini_size = Size::Logical(MINI_SIZE);

    window.set_min_size(Some(mini_size)).map_err(|e| e.to_string())?;
    window.set_max_size(Some(mini_size)).map_err(|e| e.to_string())?;
    window.set_size(mini_size).map_err(|e| e.to_string())?;
    window.set_resizable(false).map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())?;

    Ok(())
}

fn disable_mini_mode(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.set_always_on_top(false).map_err(|e| e.to_string())?;
    window.set_resizable(true).map_err(|e| e.to_string())?;
    window.set_max_size(None::<Size>).map_err(|e| e.to_string())?;
    window.set_size(Size::Logical(DEFAULT_SIZE)).map_err(|e| e.to_string())?;
    window.set_min_size(Some(Size::Logical(MIN_SIZE))).map_err(|e| e.to_string())?;
    window.center().map_err(|e| e.to_string())?;

    Ok(())
}

/// 获取当前运行平台
#[command]
pub fn get_platform() -> &'static str {
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
