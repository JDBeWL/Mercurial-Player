//! 外部字体扫描与系统字体枚举（fonts/ 目录、TTC 提取缓存、OS 字体表）。

use crate::error::AppError;
use std::collections::HashSet;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

#[cfg(target_os = "windows")]
use winreg::RegKey;
#[cfg(target_os = "windows")]
use winreg::enums::HKEY_LOCAL_MACHINE;

use super::parse::{
    collection_member_metas, extract_collection_member, member_file_name, sanitize_file_name,
};

/// 外部字体文件（位于软件同级 fonts/ 目录）
#[derive(Serialize)]
pub struct ExternalFont {
    /// 文件名（含扩展名），字体族名与字重由前端按命名约定解析
    pub name: String,
    /// 文件绝对路径
    pub path: String,
}

/// 支持的外部字体文件扩展名
const FONT_EXTENSIONS: [&str; 4] = ["ttf", "otf", "woff", "woff2"];

/// 支持的字体集合文件扩展名（一个文件内含多个字体成员，扫描时提取为单字体缓存文件）
const COLLECTION_EXTENSIONS: [&str; 2] = ["ttc", "otc"];

/// 获取外部字体目录路径（与可执行文件同级，约定为 fonts/，不存在时自动创建）
pub fn get_external_fonts_dir() -> Result<PathBuf, AppError> {
    let exe_path = std::env::current_exe().map_err(|e| format!("无法获取可执行文件路径: {e}"))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;
    let fonts_dir = exe_dir.join("fonts");

    // 确保目录存在（与 plugins/ 目录同样的惯例）
    if !fonts_dir.exists() {
        fs::create_dir_all(&fonts_dir).map_err(|e| format!("无法创建字体目录: {e}"))?;
    }

    Ok(fonts_dir)
}

/// 列出软件同级 fonts/ 目录下的全部字体文件
pub fn list_external_fonts() -> Result<Vec<ExternalFont>, AppError> {
    let fonts_dir = get_external_fonts_dir()?;

    let mut fonts = Vec::new();
    let entries = fs::read_dir(&fonts_dir).map_err(|e| format!("无法读取字体目录: {e}"))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        // TTC/OTC 集合：提取成员为单字体缓存文件后按普通外部字体返回
        if COLLECTION_EXTENSIONS.contains(&ext.as_str()) {
            match extract_collection_to_cache(&path) {
                Ok(members) => fonts.extend(members),
                Err(e) => log::warn!("跳过字体集合 {}: {e}", path.display()),
            }
            continue;
        }

        if !FONT_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        fonts.push(ExternalFont {
            name: name.to_string_lossy().to_string(),
            path: path.to_string_lossy().to_string(),
        });
    }

    // 按文件名排序保证选择器中顺序稳定
    fonts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fonts)
}

// ============================================================================
// TTC/OTC 字体集合支持
// ============================================================================

/// 字体缓存统计（供设置页展示与清理）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontCacheStats {
    /// TTC/OTC 集合提取缓存占用（字节）
    pub extract_cache_bytes: u64,
}

/// 递归统计目录大小（字节）
fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        dir_size(&path)
                    } else {
                        entry.metadata().map(|m| m.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

/// 统计字体提取缓存目录的总大小（字节）
pub fn font_extract_cache_size() -> u64 {
    let Ok(dir) = get_font_extract_cache_dir() else {
        return 0;
    };
    dir_size(&dir)
}

/// 清空字体集合提取缓存目录（TTC/OTC 提取出的单字体成员文件），
/// 返回释放的字节数。下次扫描 fonts/ 目录时按需重新提取
pub fn clear_font_extract_cache() -> Result<u64, AppError> {
    let dir = get_font_extract_cache_dir()?;
    let freed = dir_size(&dir);
    let entries = fs::read_dir(&dir).map_err(|e| format!("无法读取字体提取缓存目录: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(freed)
}

/// 字体集合成员提取缓存目录（temp/mercurial-player/font-extract）
pub fn get_font_extract_cache_dir() -> Result<PathBuf, AppError> {
    let dir = std::env::temp_dir()
        .join("mercurial-player")
        .join("font-extract");
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建字体提取缓存目录: {e}"))?;
    Ok(dir)
}

/// 提取字体集合的全部成员到缓存目录，返回可注册的外部字体列表。
/// 缓存键含源文件大小与修改时间，源文件变化后自动重新提取
fn extract_collection_to_cache(path: &Path) -> Result<Vec<ExternalFont>, AppError> {
    let bytes = fs::read(path).map_err(|e| format!("无法读取字体集合: {e}"))?;
    let metas = collection_member_metas(&bytes);
    if metas.is_empty() {
        return Err("集合内没有可解析的字体成员".to_string().into());
    }

    // 缓存键：文件名 + 大小 + 修改时间
    let file_meta = fs::metadata(path).map_err(|e| format!("无法读取文件元信息: {e}"))?;
    let mtime = file_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok());
    let mut hasher = DefaultHasher::new();
    path.file_name().hash(&mut hasher);
    file_meta.len().hash(&mut hasher);
    if let Some(t) = mtime {
        t.as_secs().hash(&mut hasher);
        t.subsec_nanos().hash(&mut hasher);
    }
    let stem = path
        .file_stem()
        .map(|s| sanitize_file_name(&s.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "collection".to_string());
    let cache_root = get_font_extract_cache_dir()?;
    let key_dir = cache_root.join(format!("{stem}-{:016x}", hasher.finish()));

    if !key_dir.exists() {
        // 清理同一源文件的旧版本缓存
        let prefix = format!("{stem}-");
        if let Ok(entries) = fs::read_dir(&cache_root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir()
                    && p != key_dir
                    && entry.file_name().to_string_lossy().starts_with(&prefix)
                {
                    let _ = fs::remove_dir_all(&p);
                }
            }
        }

        fs::create_dir_all(&key_dir).map_err(|e| format!("无法创建提取缓存目录: {e}"))?;
        let mut used = HashSet::new();
        for (i, meta) in metas.iter().enumerate() {
            let Some(member_bytes) = extract_collection_member(&bytes, i) else {
                log::warn!("跳过字体集合 {} 的第 {i} 个成员：提取失败", path.display());
                continue;
            };
            let name = member_file_name(meta, &mut used);
            let out_path = key_dir.join(&name);
            if let Err(e) = fs::write(&out_path, &member_bytes) {
                log::warn!("无法写入提取的字体 {}: {e}", out_path.display());
            }
        }
    }

    // 缓存目录中的文件即全部成员，文件名已按前端约定生成
    let mut fonts = Vec::new();
    let entries = fs::read_dir(&key_dir).map_err(|e| format!("无法读取提取缓存目录: {e}"))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        if let Some(name) = p.file_name() {
            fonts.push(ExternalFont {
                name: name.to_string_lossy().to_string(),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    fonts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(fonts)
}

/// 获取系统已安装的字体列表
pub fn get_system_fonts() -> Result<Vec<String>, AppError> {
    #[cfg(target_os = "windows")]
    {
        get_windows_fonts()
    }

    #[cfg(target_os = "macos")]
    {
        get_macos_fonts()
    }

    #[cfg(target_os = "linux")]
    {
        get_linux_fonts()
    }
}

#[cfg(target_os = "windows")]
fn get_windows_fonts() -> Result<Vec<String>, AppError> {
    let mut fonts = HashSet::new();

    // 读取注册表中的字体信息
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    // Windows 字体注册表路径
    let font_key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts")
        .map_err(|e| format!("Failed to open fonts registry key: {e}"))?;

    // 遍历所有字体条目
    for (name, _) in font_key.enum_values().filter_map(Result::ok) {
        // 字体名称格式通常是 "Font Name (TrueType)" 或 "Font Name & Font Name Bold (TrueType)"
        // 提取实际的字体名称
        if let Some(font_name) = extract_font_name(&name) {
            fonts.insert(font_name);
        }
    }

    // 转换为排序的 Vec
    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();

    Ok(font_list)
}

#[cfg(target_os = "macos")]
fn get_macos_fonts() -> Result<Vec<String>, AppError> {
    // macOS 字体枚举尚未实现:返回伪造的字体名会误导用户,这里显式报错而非静默降级
    Err(AppError::msg(
        "macOS font enumeration is not implemented yet",
    ))
}

#[cfg(target_os = "linux")]
fn get_linux_fonts() -> Result<Vec<String>, AppError> {
    use std::process::Command;

    // 使用 fc-list 命令获取字体列表
    let output = Command::new("fc-list")
        .args([":", "family"])
        .output()
        .map_err(|e| format!("Failed to execute fc-list: {e}"))?;

    if !output.status.success() {
        return Err(AppError::msg("fc-list command failed"));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut fonts = HashSet::new();

    // fc-list 输出格式：每行一个字体族名称
    for line in output_str.lines() {
        let font_name = line.trim();
        if !font_name.is_empty() {
            // 有些字体名称包含多个变体，用逗号分隔
            for name in font_name.split(',') {
                fonts.insert(name.trim().to_string());
            }
        }
    }

    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();

    Ok(font_list)
}

/// 从注册表字体名称中提取实际的字体名称
/// 例如：
/// - "Arial (TrueType)" -> "Arial"
/// - "Microsoft YaHei & Microsoft YaHei UI (TrueType)" -> "Microsoft YaHei"
/// - "Segoe UI Bold (TrueType)" -> "Segoe UI"
#[cfg(target_os = "windows")]
pub(super) fn extract_font_name(registry_name: &str) -> Option<String> {
    // 移除括号及其内容
    let name = registry_name.split('(').next()?.trim();

    // 如果包含 &，取第一个名称
    let name = name.split('&').next()?.trim();

    // 移除字体样式后缀（Bold, Italic, Light 等）
    let name = remove_font_style_suffix(name);

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 移除字体样式后缀
#[cfg(target_os = "windows")]
fn remove_font_style_suffix(name: &str) -> &str {
    let suffixes = [
        " Bold",
        " Italic",
        " Light",
        " Regular",
        " Medium",
        " Semibold",
        " Black",
        " Thin",
        " ExtraLight",
        " ExtraBold",
        " Heavy",
    ];

    for suffix in &suffixes {
        if let Some(pos) = name.rfind(suffix) {
            // 确保后缀在末尾
            if pos + suffix.len() == name.len() {
                return name[..pos].trim();
            }
        }
    }

    name
}
