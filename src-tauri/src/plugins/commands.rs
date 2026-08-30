//! 插件系统 Tauri 命令
use crate::error::AppError;

use super::manager::{self, PluginManifest};
use crate::security::{has_allowed_extension, is_simple_filename};
use tauri::command;

/// 允许的截图文件扩展名
const SCREENSHOT_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// 列出所有插件
#[command]
pub fn list_plugins() -> Result<Vec<String>, AppError> {
    manager::list_plugin_dirs()
}

/// 读取插件清单
#[command]
pub fn read_plugin_manifest(path: &str) -> Result<PluginManifest, AppError> {
    manager::read_manifest(path)
}

/// 读取插件主文件
#[command]
pub fn read_plugin_main(path: &str, main: &str) -> Result<String, AppError> {
    let main_file = if main.is_empty() { "index.js" } else { main };
    manager::read_main_file(path, main_file)
}

/// 卸载插件
#[command]
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), AppError> {
    manager::uninstall_plugin(plugin_id)
}

/// 在文件管理器中打开插件目录
#[command]
pub fn open_plugins_directory() -> Result<(), AppError> {
    let plugins_dir = manager::get_plugins_dir()?;

    tauri_plugin_opener::reveal_item_in_dir(&plugins_dir)
        .map_err(|e| format!("无法打开目录: {e}"))?;

    Ok(())
}

/// 保存截图到程序目录下的 screenshots 文件夹
#[command]
pub fn save_screenshot(filename: &str, data: Vec<u8>) -> Result<String, AppError> {
    // 校验文件名：必须为简单文件名且为图片扩展名，防止路径穿越任意写
    if !is_simple_filename(filename) || !has_allowed_extension(filename, &SCREENSHOT_EXTENSIONS) {
        return Err(AppError::Plugin(
            "非法的截图文件名（仅允许 png/jpg/jpeg/webp）".to_string(),
        ));
    }

    let exe_path = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;

    let screenshots_dir = exe_dir.join("screenshots");

    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir).map_err(|e| format!("无法创建截图目录: {e}"))?;
    }

    let file_path = screenshots_dir.join(filename);

    std::fs::write(&file_path, &data).map_err(|e| format!("无法保存截图: {e}"))?;

    Ok(file_path.to_string_lossy().to_string())
}

/// 打开截图目录
#[command]
pub fn open_screenshots_directory() -> Result<(), AppError> {
    let exe_path = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;

    let screenshots_dir = exe_dir.join("screenshots");

    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir).map_err(|e| format!("无法创建截图目录: {e}"))?;
    }

    tauri_plugin_opener::reveal_item_in_dir(&screenshots_dir)
        .map_err(|e| format!("无法打开目录: {e}"))?;

    Ok(())
}
