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
pub fn read_plugin_manifest(path: String) -> Result<PluginManifest, String> {
    manager::read_manifest(&path)
}

/// 读取插件主文件
#[command]
pub fn read_plugin_main(path: String, main: String) -> Result<String, String> {
    let main_file = if main.is_empty() { "index.js".to_string() } else { main };
    manager::read_main_file(&path, &main_file)
}

/// 安装插件
#[command]
pub fn install_plugin(source: String) -> InstallResult {
    match manager::install_plugin_from_path(&source) {
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
pub fn uninstall_plugin(plugin_id: String) -> Result<(), String> {
    manager::uninstall_plugin(&plugin_id)
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
            .map_err(|e| format!("无法打开目录: {}", e))?;
    }
    
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&plugins_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {}", e))?;
    }
    
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&plugins_dir)
            .spawn()
            .map_err(|e| format!("无法打开目录: {}", e))?;
    }
    
    Ok(())
}
