//! 插件管理器
//! 处理插件的文件系统操作
use crate::error::AppError;

use crate::security::{is_safe_relative_path, is_simple_filename, is_within_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 插件清单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub main: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "default_auto_activate")]
    pub auto_activate: bool,
}

const fn default_auto_activate() -> bool {
    true
}

/// 获取插件目录路径（与可执行文件同级）
pub fn get_plugins_dir() -> Result<PathBuf, AppError> {
    let exe_path = std::env::current_exe()
        .map_err(|e| AppError::Plugin(format!("无法获取可执行文件路径: {e}")))?;

    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;

    let plugins_dir = exe_dir.join("plugins");

    // 确保目录存在
    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| AppError::Plugin(format!("无法创建插件目录: {e}")))?;
    }

    Ok(plugins_dir)
}

/// 列出所有插件目录
pub fn list_plugin_dirs() -> Result<Vec<String>, AppError> {
    let plugins_dir = get_plugins_dir()?;

    let mut plugin_dirs = Vec::new();

    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 检查是否有 manifest.json
                let manifest_path = path.join("manifest.json");
                if manifest_path.exists()
                    && let Some(name) = path.file_name()
                {
                    plugin_dirs.push(name.to_string_lossy().to_string());
                }
            }
        }
    }

    Ok(plugin_dirs)
}

/// 读取插件清单
///
/// 目录名必须与清单 id 一致:插件身份(卸载、权限、存储归属)按 id 认定,
/// 不一致会让恶意插件顶替他人身份。
pub fn read_manifest(plugin_name: &str) -> Result<PluginManifest, AppError> {
    if !is_simple_filename(plugin_name) {
        return Err(AppError::Plugin("非法的插件目录名".to_string()));
    }

    let plugins_dir = get_plugins_dir()?;
    let manifest_path = plugins_dir.join(plugin_name).join("manifest.json");

    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| AppError::Plugin(format!("无法读取插件清单: {e}")))?;

    let manifest: PluginManifest = serde_json::from_str(&content)
        .map_err(|e| AppError::Plugin(format!("无法解析插件清单: {e}")))?;

    if manifest.id != plugin_name {
        return Err(AppError::Plugin(format!(
            "插件目录名与清单 id 不一致,拒绝加载: {plugin_name} != {}",
            manifest.id
        )));
    }

    Ok(manifest)
}

/// 读取插件主文件
pub fn read_main_file(plugin_name: &str, main_file: &str) -> Result<String, AppError> {
    if !is_simple_filename(plugin_name) {
        return Err(AppError::Plugin("非法的插件目录名".to_string()));
    }
    if !is_safe_relative_path(main_file) {
        return Err(AppError::Plugin("非法的插件文件路径".to_string()));
    }

    let plugins_dir = get_plugins_dir()?;
    let main_path = plugins_dir.join(plugin_name).join(main_file);

    // 防止符号链接逃逸：解析真实路径后必须仍在插件目录内
    if !is_within_dir(&main_path, &plugins_dir) {
        return Err(AppError::Plugin("插件文件路径超出插件目录范围".to_string()));
    }

    fs::read_to_string(&main_path).map_err(|e| AppError::Plugin(format!("无法读取插件主文件: {e}")))
}

/// 卸载插件
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), AppError> {
    // 校验插件 ID，防止路径穿越导致任意目录递归删除
    if !is_simple_filename(plugin_id) {
        return Err(AppError::Plugin("非法的插件 ID".to_string()));
    }

    let plugins_dir = get_plugins_dir()?;
    let target_dir = plugins_dir.join(plugin_id);

    if !target_dir.exists() {
        return Ok(());
    }

    // 删除前先让目录自证身份:清单 id 与目录名不一致(或清单读不出)时拒绝,
    // 否则声明他人 id 的插件能让"卸载"删掉别人的目录
    read_manifest(plugin_id)?;

    fs::remove_dir_all(&target_dir).map_err(|e| AppError::Plugin(format!("无法删除插件: {e}")))?;

    Ok(())
}
