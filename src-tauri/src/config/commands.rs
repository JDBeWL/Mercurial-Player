//! 配置管理相关的 Tauri 命令
//!
//! 这个模块包含所有与配置管理相关的功能，包括加载、保存、导入、导出等。

use super::manager::AppConfig;
use crate::AppState;
use tauri::{command, State};

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
