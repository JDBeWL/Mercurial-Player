//! 插件管理器
//! 处理插件的文件系统操作

use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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
pub fn get_plugins_dir() -> Result<PathBuf, String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    
    let exe_dir = exe_path.parent()
        .ok_or("无法获取可执行文件目录")?;
    
    let plugins_dir = exe_dir.join("plugins");
    
    // 确保目录存在
    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir)
            .map_err(|e| format!("无法创建插件目录: {e}"))?;
    }
    
    Ok(plugins_dir)
}

/// 列出所有插件目录
pub fn list_plugin_dirs() -> Result<Vec<String>, String> {
    let plugins_dir = get_plugins_dir()?;
    
    let mut plugin_dirs = Vec::new();
    
    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 检查是否有 manifest.json
                let manifest_path = path.join("manifest.json");
                if manifest_path.exists() && let Some(name) = path.file_name() {
                    plugin_dirs.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    
    Ok(plugin_dirs)
}

/// 读取插件清单
pub fn read_manifest(plugin_name: &str) -> Result<PluginManifest, String> {
    let plugins_dir = get_plugins_dir()?;
    let manifest_path = plugins_dir.join(plugin_name).join("manifest.json");
    
    let content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("无法读取插件清单: {e}"))?;
    
    let manifest: PluginManifest = serde_json::from_str(&content)
        .map_err(|e| format!("无法解析插件清单: {e}"))?;
    
    Ok(manifest)
}

/// 读取插件主文件
pub fn read_main_file(plugin_name: &str, main_file: &str) -> Result<String, String> {
    let plugins_dir = get_plugins_dir()?;
    let main_path = plugins_dir.join(plugin_name).join(main_file);
    
    fs::read_to_string(&main_path)
        .map_err(|e| format!("无法读取插件主文件: {e}"))
}

/// 安装插件
pub fn install_plugin_from_path(source_path: &str) -> Result<String, String> {
    let source = PathBuf::from(source_path);
    
    if !source.exists() {
        return Err("源路径不存在".to_string());
    }
    
    // 读取源目录的 manifest
    let manifest_path = source.join("manifest.json");
    if !manifest_path.exists() {
        return Err("插件缺少 manifest.json".to_string());
    }
    
    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("无法读取清单: {e}"))?;
    
    let manifest: PluginManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| format!("无法解析清单: {e}"))?;
    
    // 复制到插件目录
    let plugins_dir = get_plugins_dir()?;
    let target_dir = plugins_dir.join(&manifest.id);
    
    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)
            .map_err(|e| format!("无法删除旧版本: {e}"))?;
    }
    
    copy_dir_recursive(&source, &target_dir)?;
    
    Ok(manifest.id)
}

/// 卸载插件
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), String> {
    let plugins_dir = get_plugins_dir()?;
    let plugin_dir = plugins_dir.join(plugin_id);
    
    if plugin_dir.exists() {
        fs::remove_dir_all(&plugin_dir)
            .map_err(|e| format!("无法删除插件: {e}"))?;
    }
    
    Ok(())
}

/// 递归复制目录
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    fs::create_dir_all(dst)
        .map_err(|e| format!("无法创建目录: {e}"))?;
    
    for entry in fs::read_dir(src).map_err(|e| format!("无法读取目录: {e}"))? {
        let entry = entry.map_err(|e| format!("无法读取条目: {e}"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("无法复制文件: {e}"))?;
        }
    }
    
    Ok(())
}
