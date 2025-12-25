//! 配置管理相关的 Tauri 命令
//!
//! 这个模块包含所有与配置管理相关的功能，包括加载、保存、导入、导出等。

use super::manager::AppConfig;
use crate::AppState;
use std::path::Path;
use tauri::{command, State};

/// 验证路径是否安全（不在敏感目录中）
fn is_path_safe(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    
    // 规范化路径
    let canonical = path.canonicalize()
        .map_err(|_| "无法解析路径，请确保目录存在".to_string())?;
    let path_str = canonical.to_string_lossy().to_lowercase();
    
    // Windows 敏感目录
    #[cfg(target_os = "windows")]
    {
        let forbidden = [
            "c:\\windows",
            "c:\\program files",
            "c:\\program files (x86)",
            "c:\\programdata",
            "\\.ssh",
            "\\.gnupg",
            "\\appdata\\roaming\\microsoft",
        ];
        for pattern in &forbidden {
            if path_str.contains(pattern) {
                return Err(format!("安全限制：不允许添加系统敏感目录"));
            }
        }
    }
    
    // macOS/Linux 敏感目录
    #[cfg(not(target_os = "windows"))]
    {
        let forbidden = [
            "/etc",
            "/usr",
            "/bin",
            "/sbin",
            "/var",
            "/system",
            "/.ssh",
            "/.gnupg",
            "/.config",
        ];
        for pattern in &forbidden {
            if path_str.contains(pattern) {
                return Err(format!("安全限制：不允许添加系统敏感目录"));
            }
        }
    }
    
    // 确保是目录
    if !canonical.is_dir() {
        return Err("指定的路径不是一个目录".to_string());
    }
    
    Ok(())
}

/// 初始化配置文件
#[command]
pub fn initialize_config_files(state: State<AppState>) -> Result<(), String> {
    state.config_manager.initialize_config_files()
}

/// 加载配置
#[command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, String> {
    state.config_manager.load_config()
}

/// 保存配置
#[command]
pub fn save_config(state: State<AppState>, config: AppConfig) -> Result<(), String> {
    state.config_manager.save_config(&config)
}

/// 导出配置到指定路径
#[command]
pub fn export_config(state: State<AppState>, config: AppConfig, file_path: String) -> Result<(), String> {
    state.config_manager.export_config(&config, &file_path)
}

/// 从指定路径导入配置
#[command]
pub fn import_config(state: State<AppState>, file_path: String) -> Result<AppConfig, String> {
    state.config_manager.import_config(&file_path)
}

/// 重置配置为默认值
#[command]
pub fn reset_config(state: State<AppState>) -> Result<AppConfig, String> {
    state.config_manager.reset_config()
}

/// 添加音乐目录
#[command]
pub fn add_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, String> {
    // 验证路径安全性
    is_path_safe(&path)?;
    
    let mut config = state.config_manager.load_config()?;
    if !config.music_directories.contains(&path) {
        config.music_directories.push(path);
        state.config_manager.save_config(&config)?;
    }
    Ok(config.music_directories)
}

/// 移除音乐目录
#[command]
pub fn remove_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, String> {
    let mut config = state.config_manager.load_config()?;
    config.music_directories.retain(|p| p != &path);
    state.config_manager.save_config(&config)?;
    Ok(config.music_directories)
}

/// 设置音乐目录列表
#[command]
pub fn set_music_directories(state: State<AppState>, paths: Vec<String>) -> Result<Vec<String>, String> {
    // 验证所有路径的安全性
    for path in &paths {
        is_path_safe(path)?;
    }
    
    let mut config = state.config_manager.load_config()?;
    config.music_directories = paths;
    state.config_manager.save_config(&config)?;
    Ok(config.music_directories)
}

/// 获取当前音乐目录列表
#[command]
pub fn get_music_directories(state: State<AppState>) -> Result<Vec<String>, String> {
    let config = state.config_manager.load_config()?;
    Ok(config.music_directories)
}
