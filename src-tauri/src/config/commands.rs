//! 配置管理相关的 Tauri 命令
//!
//! 这个模块包含所有与配置管理相关的功能，包括加载、保存、导入、导出等。
use crate::error::AppError;

use super::manager::AppConfig;
use crate::AppState;
use crate::security::is_sensitive_path;
use std::path::Path;
use tauri::{State, command};

/// 验证路径是否安全（不在敏感目录中）
fn is_path_safe(path: &str) -> Result<(), AppError> {
    // 先对原始输入做词法检查
    if is_sensitive_path(path) {
        return Err(AppError::Path(
            "安全限制：不允许添加系统敏感目录".to_string(),
        ));
    }

    let path = Path::new(path);

    // 规范化路径
    let canonical = path
        .canonicalize()
        .map_err(|_| AppError::Path("无法解析路径，请确保目录存在".to_string()))?;

    // canonicalize 可能解析出输入中未显现的敏感位置（如 junction/symlink），需复查
    let mut canonical_str = canonical.to_string_lossy().to_string();
    if let Some(stripped) = canonical_str.strip_prefix(r"\\?\") {
        canonical_str = stripped.to_string();
    }
    if is_sensitive_path(&canonical_str) {
        return Err(AppError::Path(
            "安全限制：不允许添加系统敏感目录".to_string(),
        ));
    }

    // 确保是目录
    if !canonical.is_dir() {
        return Err(AppError::Path("指定的路径不是一个目录".to_string()));
    }

    Ok(())
}

/// 验证配置文件路径安全：必须为 .json 文件且不在敏感目录中
fn is_config_file_path_safe(path: &str) -> Result<(), AppError> {
    if is_sensitive_path(path) {
        return Err(AppError::Path(
            "安全限制：不允许在敏感目录中操作配置文件".to_string(),
        ));
    }
    if !path.to_lowercase().ends_with(".json") {
        return Err(AppError::Path("配置文件必须为 .json 文件".to_string()));
    }
    Ok(())
}

/// 加载配置
#[command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, AppError> {
    state.config_manager.load_config()
}

/// 保存配置
///
/// `last_session` 由后端 save_last_session / clear_last_session 独立管理,
/// 前端负载不含该字段;若直接落盘会把已记录的播放会话抹掉,这里沿用现有值。
#[command]
pub fn save_config(state: State<AppState>, mut config: AppConfig) -> Result<(), AppError> {
    if config.last_session.is_none() {
        config.last_session = state.config_manager.load_config()?.last_session;
    }
    state.config_manager.save_config(&config)
}

/// 导出配置到指定路径
#[command]
pub fn export_config(
    state: State<AppState>,
    config: AppConfig,
    file_path: String,
) -> Result<(), AppError> {
    is_config_file_path_safe(&file_path)?;
    state.config_manager.export_config(&config, &file_path)
}

/// 从指定路径导入配置
#[command]
pub fn import_config(state: State<AppState>, file_path: String) -> Result<AppConfig, AppError> {
    is_config_file_path_safe(&file_path)?;
    state.config_manager.import_config(&file_path)
}

/// 添加音乐目录
#[command]
pub fn add_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, AppError> {
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
pub fn remove_music_directory(
    state: State<AppState>,
    path: String,
) -> Result<Vec<String>, AppError> {
    let mut config = state.config_manager.load_config()?;
    config.music_directories.retain(|p| p != &path);
    state.config_manager.save_config(&config)?;
    Ok(config.music_directories)
}

/// 设置音乐目录列表
#[command]
pub fn set_music_directories(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Vec<String>, AppError> {
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
pub fn get_music_directories(state: State<AppState>) -> Result<Vec<String>, AppError> {
    let config = state.config_manager.load_config()?;
    Ok(config.music_directories)
}
