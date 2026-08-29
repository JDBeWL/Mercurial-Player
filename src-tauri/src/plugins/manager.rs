//! 插件管理器
//! 处理插件的文件系统操作
use crate::error::AppError;

use crate::security::{
    is_safe_relative_path, is_simple_filename, is_valid_plugin_id, is_within_dir,
};
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

/// 安装插件
pub fn install_plugin_from_path(source_path: &str) -> Result<String, AppError> {
    let source = PathBuf::from(source_path);

    if !source.exists() {
        return Err(AppError::Plugin("源路径不存在".to_string()));
    }

    // 读取源目录的 manifest
    let manifest_path = source.join("manifest.json");
    if !manifest_path.exists() {
        return Err(AppError::Plugin("插件缺少 manifest.json".to_string()));
    }

    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| AppError::Plugin(format!("无法读取清单: {e}")))?;

    let manifest: PluginManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| AppError::Plugin(format!("无法解析清单: {e}")))?;

    // 校验插件 ID，防止 manifest.id 携带路径穿越导致任意目录删除/写入
    if !is_valid_plugin_id(&manifest.id) {
        return Err(AppError::Plugin(format!("非法的插件 ID: {}", manifest.id)));
    }

    // 复制到插件目录
    let plugins_dir = get_plugins_dir()?;
    let target_dir = plugins_dir.join(&manifest.id);

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)
            .map_err(|e| AppError::Plugin(format!("无法删除旧版本: {e}")))?;
    }

    copy_dir_recursive(&source, &target_dir)?;

    Ok(manifest.id)
}

/// 卸载插件
pub fn uninstall_plugin(plugin_id: &str) -> Result<(), AppError> {
    // 校验插件 ID，防止路径穿越导致任意目录递归删除
    if !is_simple_filename(plugin_id) {
        return Err(AppError::Plugin("非法的插件 ID".to_string()));
    }

    let plugins_dir = get_plugins_dir()?;
    let target_dir = plugins_dir.join(plugin_id);

    if target_dir.exists() {
        fs::remove_dir_all(&target_dir)
            .map_err(|e| AppError::Plugin(format!("无法删除插件: {e}")))?;
    }

    Ok(())
}

/// 递归复制目录（带深度限制和符号链接防护）
fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<(), AppError> {
    copy_dir_recursive_inner(src, dst, 0)
}

const MAX_COPY_DEPTH: u32 = 32;

fn copy_dir_recursive_inner(src: &PathBuf, dst: &PathBuf, depth: u32) -> Result<(), AppError> {
    if depth > MAX_COPY_DEPTH {
        return Err(AppError::Plugin(format!(
            "目录嵌套深度超过限制 ({MAX_COPY_DEPTH})"
        )));
    }

    // 防止符号链接：只复制真实目录
    let metadata =
        fs::symlink_metadata(src).map_err(|e| AppError::Plugin(format!("无法读取元数据: {e}")))?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Plugin("不允许复制符号链接".to_string()));
    }

    fs::create_dir_all(dst).map_err(|e| AppError::Plugin(format!("无法创建目录: {e}")))?;

    for entry in fs::read_dir(src).map_err(|e| format!("无法读取目录: {e}"))? {
        let entry = entry.map_err(|e| AppError::Plugin(format!("无法读取条目: {e}")))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        // 检查是否为符号链接
        let entry_meta = fs::symlink_metadata(&src_path)
            .map_err(|e| AppError::Plugin(format!("无法读取元数据: {e}")))?;
        if entry_meta.file_type().is_symlink() {
            log::warn!("跳过符号链接: {}", src_path.display());
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive_inner(&src_path, &dst_path, depth + 1)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .map_err(|e| AppError::Plugin(format!("无法复制文件: {e}")))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::Path;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mercurial-plugin-mgr-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 创建目录符号链接;Windows 无特权环境下可能失败,调用方应跳过用例
    fn try_symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(target, link)
        }
    }

    #[test]
    fn test_copy_dir_recursive_copies_regular_tree() {
        let src = temp_dir("copy-src");
        let dst = temp_dir("copy-dst");
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub").join("b.txt"), "bbb").unwrap();

        copy_dir_recursive(&src, &(dst.join("plugin"))).unwrap();

        assert_eq!(
            fs::read_to_string(dst.join("plugin").join("a.txt")).unwrap(),
            "aaa"
        );
        assert_eq!(
            fs::read_to_string(dst.join("plugin").join("sub").join("b.txt")).unwrap(),
            "bbb"
        );
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn test_copy_dir_recursive_rejects_symlink_source() {
        let real = temp_dir("link-target");
        let src_parent = temp_dir("link-src");
        let link = src_parent.join("link");

        if try_symlink_dir(&real, &link).is_err() {
            // Windows 无开发者模式/管理员特权时无法创建符号链接,跳过
            return;
        }

        let dst = temp_dir("link-dst");
        let err = copy_dir_recursive(&link, &(dst.join("plugin"))).unwrap_err();
        assert!(err.to_string().contains("符号链接"), "实际错误: {err}");

        let _ = fs::remove_dir_all(&real);
        let _ = fs::remove_dir_all(&src_parent);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn test_copy_dir_recursive_skips_symlink_entries() {
        let src = temp_dir("entry-link-src");
        let outside = temp_dir("entry-link-outside");
        fs::write(outside.join("secret.txt"), "should-not-copy").unwrap();

        fs::write(src.join("ok.txt"), "fine").unwrap();
        let link = src.join("escape");
        if try_symlink_dir(&outside, &link).is_err() {
            return;
        }

        let dst = temp_dir("entry-link-dst");
        copy_dir_recursive(&src, &(dst.join("plugin"))).unwrap();

        // 普通文件已复制,符号链接条目被跳过 (不复制、不报错)
        assert_eq!(
            fs::read_to_string(dst.join("plugin").join("ok.txt")).unwrap(),
            "fine"
        );
        assert!(!dst.join("plugin").join("escape").exists());

        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn test_copy_dir_recursive_enforces_depth_limit() {
        // 构造超过 MAX_COPY_DEPTH 层的嵌套目录链
        let root = temp_dir("deep-src");
        let mut cur = root.clone();
        for i in 0..(MAX_COPY_DEPTH + 5) {
            cur = cur.join(format!("level{i}"));
            fs::create_dir_all(&cur).unwrap();
        }
        let dst = temp_dir("deep-dst");

        let err = copy_dir_recursive(&root, &(dst.join("out"))).unwrap_err();
        assert!(
            err.to_string().contains("嵌套深度超过限制"),
            "实际错误: {err}"
        );

        let _ = fs::remove_dir_all(&dst);
        let _ = fs::remove_dir_all(&root);
    }
}
