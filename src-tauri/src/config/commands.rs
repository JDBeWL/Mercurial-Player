//! 配置管理相关的 Tauri 命令：加载/保存/导入导出配置与音乐目录列表维护。
use crate::android::saf;
use crate::error::AppError;

use super::manager::AppConfig;
use crate::AppState;
use crate::security::is_sensitive_path;
use std::path::Path;
use tauri::{State, command};

/// 验证音乐目录路径是否安全（不在敏感目录中）
///
/// 词法 + canonicalize 复查的完整规则见 [`crate::media::filesystem::validate_media_path`]；
/// Android SAF 的 `content://` URI 无需本地校验，直接放行。
fn is_path_safe(path: &str) -> Result<(), AppError> {
    if saf::is_content_uri(path) {
        return Ok(());
    }

    if is_sensitive_path(path) {
        return Err(AppError::Path(
            "安全限制：不允许添加系统敏感目录".to_string(),
        ));
    }

    let path = Path::new(path);

    let canonical = path
        .canonicalize()
        .map_err(|_| AppError::Path("无法解析路径，请确保目录存在".to_string()))?;

    let mut canonical_str = canonical.to_string_lossy().to_string();
    if let Some(stripped) = canonical_str.strip_prefix(r"\\?\") {
        canonical_str = stripped.to_string();
    }
    if is_sensitive_path(&canonical_str) {
        return Err(AppError::Path(
            "安全限制：不允许添加系统敏感目录".to_string(),
        ));
    }

    if !canonical.is_dir() {
        return Err(AppError::Path("指定的路径不是一个目录".to_string()));
    }

    Ok(())
}

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

#[command]
pub fn load_config(state: State<AppState>) -> Result<AppConfig, AppError> {
    state.config_manager.load_config()
}

/// 保存前端提交的整包配置
///
/// `last_session` 与 `audio.preferred_device_id` 由后端独立写、前端负载不含，
/// 为空时沿用现值，否则整包落盘会抹掉它们。
#[command]
pub fn save_config(state: State<AppState>, mut config: AppConfig) -> Result<(), AppError> {
    // 读-改-写必须在同一把写锁内，见 ConfigManager::write_lock
    state.config_manager.update_config(|current| {
        if config.last_session.is_none() {
            config.last_session.clone_from(&current.last_session);
        }
        if config.audio.preferred_device_id.is_none() {
            config
                .audio
                .preferred_device_id
                .clone_from(&current.audio.preferred_device_id);
        }
        *current = config;
    })
}

#[command]
pub fn export_config(
    state: State<AppState>,
    config: AppConfig,
    file_path: String,
) -> Result<(), AppError> {
    is_config_file_path_safe(&file_path)?;
    state.config_manager.export_config(&config, &file_path)
}

#[command]
pub fn import_config(state: State<AppState>, file_path: String) -> Result<AppConfig, AppError> {
    is_config_file_path_safe(&file_path)?;
    state.config_manager.import_config(&file_path)
}

#[command]
pub fn add_music_directory(state: State<AppState>, path: String) -> Result<Vec<String>, AppError> {
    is_path_safe(&path)?;

    // 走 update_config 而不是分步读写，理由见 ConfigManager::write_lock
    state.config_manager.update_config(|config| {
        if !config.music_directories.contains(&path) {
            config.music_directories.push(path);
        }
        config.music_directories.clone()
    })
}

#[command]
pub fn remove_music_directory(
    state: State<AppState>,
    path: String,
) -> Result<Vec<String>, AppError> {
    state.config_manager.update_config(|config| {
        config.music_directories.retain(|p| p != &path);
        config.music_directories.clone()
    })
}

#[command]
pub fn set_music_directories(
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<Vec<String>, AppError> {
    for path in &paths {
        is_path_safe(path)?;
    }

    state.config_manager.update_config(|config| {
        config.music_directories = paths;
        config.music_directories.clone()
    })
}

#[command]
pub fn get_music_directories(state: State<AppState>) -> Result<Vec<String>, AppError> {
    let config = state.config_manager.load_config()?;
    Ok(config.music_directories)
}
