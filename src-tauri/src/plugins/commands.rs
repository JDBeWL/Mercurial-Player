//! 插件系统 Tauri 命令

use super::manager::{
    self, PluginManifest,
};
use serde::{Deserialize, Serialize};
use tauri::command;

/// 安装结果
#[derive(Debug, Serialize, Deserialize)]
pub struct InstallResult {
    pub success: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

/// 列出所有插件
#[command]
pub fn list_plugins() -> Result<Vec<String>, String> {
    manager::list_plugin_dirs()
}

/// 读取插件清单
#[command]
pub fn read_plugin_manifest(path: &str) -> Result<PluginManifest, String> {
    manager::read_manifest(path)
}

/// 读取插件主文件
#[command]
pub fn read_plugin_main(path: &str, main: &str) -> Result<String, String> {
    let main_file = if main.is_empty() { "index.js" } else { main };
    manager::read_main_file(path, main_file)
}

/// 安装插件
#[command]
#[must_use]
pub fn install_plugin(source: &str) -> InstallResult {
    match manager::install_plugin_from_path(source) {
        Ok(plugin_id) => InstallResult {
            success: true,
            path: Some(plugin_id),
            error: None,
        },
        Err(e) => InstallResult {
            success: false,
            path: None,
            error: Some(e),
        },
    }
}

/// 卸载插件
#[command]
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    manager::uninstall_plugin(plugin_id)
}

/// 获取插件目录路径
#[command]
pub fn get_plugins_directory() -> Result<String, String> {
    manager::get_plugins_dir()
        .map(|p| p.to_string_lossy().to_string())
}

/// 在文件管理器中打开插件目录
#[command]
pub fn open_plugins_directory() -> Result<(), String> {
    let plugins_dir = manager::get_plugins_dir()?;
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&plugins_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&plugins_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&plugins_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    Ok(())
}

/// 保存截图到程序目录下的 screenshots 文件夹
#[command]
pub fn save_screenshot(filename: &str, data: Vec<u8>) -> Result<String, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent()
        .ok_or("无法获取可执行文件目录")?;
    
    let screenshots_dir = exe_dir.join("screenshots");
    
    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir)
            .map_err(|e| format!("无法创建截图目录: {e}"))?;
    }
    
    let file_path = screenshots_dir.join(filename);
    
    std::fs::write(&file_path, &data)
        .map_err(|e| format!("无法保存截图: {e}"))?;
    
    Ok(file_path.to_string_lossy().to_string())
}

/// 打开截图目录
#[command]
pub fn open_screenshots_directory() -> Result<(), String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent()
        .ok_or("无法获取可执行文件目录")?;
    
    let screenshots_dir = exe_dir.join("screenshots");
    
    // 创建目录（如果不存在）
    if !screenshots_dir.exists() {
        std::fs::create_dir_all(&screenshots_dir)
            .map_err(|e| format!("无法创建截图目录: {e}"))?;
    }
    
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&screenshots_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&screenshots_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&screenshots_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {e}"))?;
    }
    
    Ok(())
}
