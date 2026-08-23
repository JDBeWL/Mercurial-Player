//! 系统字体查询模块
//!
//! 提供获取系统已安装字体列表，以及扫描软件同级 fonts/ 目录外部字体的功能。
//! TTC/OTC 字体集合会在此处提取为单字体缓存文件，供前端按普通字体注册

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
pub fn get_external_fonts_dir() -> Result<PathBuf, String> {
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
pub fn list_external_fonts() -> Result<Vec<ExternalFont>, String> {
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

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// 从 TTC/OTC 集合中提取指定成员为独立字体文件。
///
/// 定位成员的 sfnt 表目录，逐表原样拷贝数据、重建目录偏移。
/// 表数据未被修改，各表自身的校验和依然有效；浏览器不校验
/// head.checkSumAdjustment，因此无需重算整体校验和。
fn extract_collection_member(data: &[u8], index: usize) -> Option<Vec<u8>> {
    if data.len() < 12 || &data[0..4] != b"ttcf" {
        return None;
    }
    // ttcf 头布局：tag(4) + version(4) + numFonts(4) + 各成员目录偏移（从 12 起）
    let num_fonts = be_u32(data, 8)? as usize;
    if index >= num_fonts {
        return None;
    }
    let dir_offset = be_u32(data, 12 + index * 4)? as usize;
    extract_sfnt_directory(data, dir_offset)
}

/// 将 data 中位于 dir_offset 的 sfnt 表目录重建为独立字体文件
fn extract_sfnt_directory(data: &[u8], dir_offset: usize) -> Option<Vec<u8>> {
    if dir_offset + 12 > data.len() {
        return None;
    }
    let version: [u8; 4] = data.get(dir_offset..dir_offset + 4)?.try_into().ok()?;
    // sfnt 目录布局：version(4) + numTables(2) + searchRange(2) + entrySelector(2) + rangeShift(2)
    let num_tables = be_u16(data, dir_offset + 4)? as usize;
    let dir_len = 12 + num_tables * 16;
    if dir_offset + dir_len > data.len() {
        return None;
    }

    // 读取表记录（tag, 原校验和, 原始偏移, 长度）并校验范围。
    // 记录在源文件中本就按 tag 升序排列（sfnt 规范），保持原顺序
    let mut records = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = dir_offset + 12 + i * 16;
        let tag: [u8; 4] = data.get(rec..rec + 4)?.try_into().ok()?;
        let checksum: [u8; 4] = data.get(rec + 4..rec + 8)?.try_into().ok()?;
        let offset = be_u32(data, rec + 8)? as usize;
        let len = be_u32(data, rec + 12)? as usize;
        let end = offset.checked_add(len)?;
        if end > data.len() {
            return None;
        }
        records.push((tag, checksum, offset, len));
    }

    // 二分查找参数按规范计算（浏览器实际不依赖这些字段）
    let mut entry_selector = 0usize;
    while (1usize << (entry_selector + 1)) <= num_tables {
        entry_selector += 1;
    }
    let search_range = (1usize << entry_selector) * 16;
    // 饱和减法：畸形输入下 numTables*16 可能小于 searchRange
    let range_shift = (num_tables * 16).saturating_sub(search_range);

    let payload_len: usize = records.iter().map(|&(_, _, _, l)| (l + 3) & !3).sum();
    let mut out = Vec::with_capacity(dir_len + payload_len);
    out.extend_from_slice(&version);
    out.extend_from_slice(&(num_tables as u16).to_be_bytes());
    out.extend_from_slice(&(search_range as u16).to_be_bytes());
    out.extend_from_slice(&(entry_selector as u16).to_be_bytes());
    out.extend_from_slice(&(range_shift as u16).to_be_bytes());

    // 先写占位表记录，再拷贝表数据并回填实际偏移
    let records_start = out.len();
    out.resize(records_start + num_tables * 16, 0);
    for (i, &(tag, checksum, offset, len)) in records.iter().enumerate() {
        let new_offset = out.len();
        out.extend_from_slice(data.get(offset..offset + len)?);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        let rec = records_start + i * 16;
        out[rec..rec + 4].copy_from_slice(&tag);
        out[rec + 4..rec + 8].copy_from_slice(&checksum);
        out[rec + 8..rec + 12].copy_from_slice(&(new_offset as u32).to_be_bytes());
        out[rec + 12..rec + 16].copy_from_slice(&(len as u32).to_be_bytes());
    }

    Some(out)
}

/// 集合成员的元信息（读取自字体内部的 name / OS/2 / fvar 表）
struct CollectionMemberMeta {
    family: String,
    weight: u16,
    variable: bool,
    /// CFF 轮廓（决定提取文件的扩展名 otf / ttf）
    cff: bool,
}

/// 解析集合内全部成员的元信息；非集合数据视为单成员
fn collection_member_metas(data: &[u8]) -> Vec<CollectionMemberMeta> {
    let count = ttf_parser::fonts_in_collection(data).unwrap_or(1);
    (0..count)
        .filter_map(|i| ttf_parser::Face::parse(data, i).ok())
        .map(|face| {
            let family = face_family_name(&face)
                .unwrap_or_else(|| format!("Collection Font {}", face.weight().to_number()));
            CollectionMemberMeta {
                family,
                weight: face.weight().to_number(),
                variable: face.is_variable(),
                cff: face.tables().cff.is_some(),
            }
        })
        .collect()
}

/// 从 name 表取字体族名：优先 Windows 平台英文名（区域变体族名互不冲突），
/// 回退任意语言首个可解码名称
fn face_family_name(face: &ttf_parser::Face) -> Option<String> {
    let ids = [
        ttf_parser::name_id::TYPOGRAPHIC_FAMILY,
        ttf_parser::name_id::FAMILY,
    ];
    let decode = |name: ttf_parser::name::Name| name.to_string().filter(|s| !s.trim().is_empty());
    for &id in &ids {
        for name in face.names() {
            if name.name_id == id
                && name.platform_id == ttf_parser::PlatformId::Windows
                && name.language_id == 0x0409
            {
                if let Some(s) = decode(name) {
                    return Some(s);
                }
            }
        }
    }
    for &id in &ids {
        for name in face.names() {
            if name.name_id == id {
                if let Some(s) = decode(name) {
                    return Some(s);
                }
            }
        }
    }
    None
}

/// 剔除文件名非法字符与引号（前端按文件名解析族名时会剔除引号，保持一致）
fn sanitize_file_name(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(
                c,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\''
            )
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// 成员缓存文件名，遵循前端命名约定：
/// `族名-字重.ttf`，可变字体为 `族名-VF.ttf`
fn member_file_name(meta: &CollectionMemberMeta, used: &mut HashSet<String>) -> String {
    let family = sanitize_file_name(&meta.family);
    let ext = if meta.cff { "otf" } else { "ttf" };
    let base = if meta.variable {
        format!("{family}-VF.{ext}")
    } else {
        format!("{family}-{}.{ext}", meta.weight)
    };
    // 同族同字重的重复成员（极少见）追加序号避免覆盖
    let mut name = base.clone();
    let mut n = 2;
    while !used.insert(name.clone()) {
        let stem = Path::new(&base)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| base.clone());
        name = format!("{stem}-{n}.{ext}");
        n += 1;
    }
    name
}

/// 前端 parseFontFileName 认可的英文字重后缀（文件名族名解析需与之保持一致）
const WEIGHT_NAME_SUFFIXES: [&str; 15] = [
    "extralight",
    "ultralight",
    "semibold",
    "demibold",
    "extrabold",
    "ultrabold",
    "thin",
    "light",
    "regular",
    "normal",
    "book",
    "medium",
    "bold",
    "black",
    "heavy",
];

/// 按前端命名约定（parseFontFileName）从字体文件名解析 @font-face 族名。
/// 桌面歌词的 DirectWrite 侧用它把前端族名映射到字体内部族名：
/// 依次剔除末尾的 `-数字字重` / `-英文字重名` 后缀（`-VF` 是否剔除以原始
/// 文件名结尾为准，与前端行为一致），再剔除所有引号
pub fn frontend_family_from_file_name(file_name: &str) -> String {
    let stem = match file_name.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => file_name,
    };
    let variable = stem.to_lowercase().ends_with("-vf");
    let cut = stem
        .rsplit_once('-')
        .map(|(_, tail)| {
            let tail_lower = tail.to_lowercase();
            if (tail_lower.len() == 3 && tail_lower.bytes().all(|b| b.is_ascii_digit()))
                || WEIGHT_NAME_SUFFIXES.contains(&tail_lower.as_str())
            {
                tail.len() + 1
            } else {
                0
            }
        })
        .unwrap_or(0);
    let stem = &stem[..stem.len() - cut];
    let stem = if variable && stem.len() >= 3 {
        &stem[..stem.len() - 3]
    } else {
        stem
    };
    stem.replace(['"', '\''], "")
}

/// 字体文件内全部面的内部族名（去重、保持出现顺序）。
/// 供桌面歌词建立 前端族名 → 内部族名 的映射；无法解析的文件返回空
pub fn internal_font_families(data: &[u8]) -> Vec<String> {
    let mut families = Vec::new();
    for meta in collection_member_metas(data) {
        if !families.contains(&meta.family) {
            families.push(meta.family);
        }
    }
    families
}

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
pub fn clear_font_extract_cache() -> Result<u64, String> {
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
pub fn get_font_extract_cache_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir()
        .join("mercurial-player")
        .join("font-extract");
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建字体提取缓存目录: {e}"))?;
    Ok(dir)
}

/// 提取字体集合的全部成员到缓存目录，返回可注册的外部字体列表。
/// 缓存键含源文件大小与修改时间，源文件变化后自动重新提取
fn extract_collection_to_cache(path: &Path) -> Result<Vec<ExternalFont>, String> {
    let bytes = fs::read(path).map_err(|e| format!("无法读取字体集合: {e}"))?;
    let metas = collection_member_metas(&bytes);
    if metas.is_empty() {
        return Err("集合内没有可解析的字体成员".to_string());
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
pub fn get_system_fonts() -> Result<Vec<String>, String> {
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
fn get_windows_fonts() -> Result<Vec<String>, String> {
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
fn get_macos_fonts() -> Result<Vec<String>, String> {
    use std::process::Command;

    // 使用 system_profiler 获取字体列表
    let output = Command::new("system_profiler")
        .args(["SPFontsDataType", "-json"])
        .output()
        .map_err(|e| format!("Failed to execute system_profiler: {e}"))?;

    if !output.status.success() {
        return Err("system_profiler command failed".to_string());
    }

    let _json_str = String::from_utf8_lossy(&output.stdout);

    // 简单解析 JSON（实际项目中应使用 serde_json）
    let mut fonts = HashSet::new();

    // 这里需要根据实际的 JSON 结构解析
    // 暂时返回一些常见的 macOS 字体
    fonts.insert("SF Pro".to_string());
    fonts.insert("Helvetica Neue".to_string());
    fonts.insert("Arial".to_string());
    fonts.insert("Times New Roman".to_string());
    fonts.insert("Courier New".to_string());

    let mut font_list: Vec<String> = fonts.into_iter().collect();
    font_list.sort();

    Ok(font_list)
}

#[cfg(target_os = "linux")]
fn get_linux_fonts() -> Result<Vec<String>, String> {
    use std::process::Command;

    // 使用 fc-list 命令获取字体列表
    let output = Command::new("fc-list")
        .args([":", "family"])
        .output()
        .map_err(|e| format!("Failed to execute fc-list: {e}"))?;

    if !output.status.success() {
        return Err("fc-list command failed".to_string());
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
fn extract_font_name(registry_name: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 sfnt 字体字节（表数据任意），base 为该字体在集合文件中的绝对偏移
    fn build_sfnt(base: usize, tables: &[([u8; 4], &[u8])]) -> Vec<u8> {
        let num = tables.len();
        let mut out = Vec::new();
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&(num as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]); // searchRange/entrySelector/rangeShift 占位
        let records_start = out.len();
        out.resize(records_start + num * 16, 0);
        for (i, &(tag, data)) in tables.iter().enumerate() {
            let offset = base + out.len();
            out.extend_from_slice(data);
            while out.len() % 4 != 0 {
                out.push(0);
            }
            let rec = records_start + i * 16;
            out[rec..rec + 4].copy_from_slice(&tag);
            out[rec + 4..rec + 8].copy_from_slice(&[0xAB; 4]); // checkSum 占位
            out[rec + 8..rec + 12].copy_from_slice(&(offset as u32).to_be_bytes());
            out[rec + 12..rec + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        }
        out
    }

    /// 将多个成员的表目录包装成 ttcf 集合文件
    fn build_collection(members: &[&[([u8; 4], &[u8])]]) -> Vec<u8> {
        let header_len = 12 + 4 * members.len();
        let mut bases = Vec::with_capacity(members.len());
        let mut cur = header_len;
        for m in members {
            let payload: usize = m.iter().map(|(_, d)| (d.len() + 3) & !3).sum();
            bases.push(cur);
            cur += 12 + m.len() * 16 + payload;
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"ttcf");
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&(members.len() as u32).to_be_bytes());
        for b in &bases {
            out.extend_from_slice(&(*b as u32).to_be_bytes());
        }
        for (m, b) in members.iter().zip(&bases) {
            out.extend_from_slice(&build_sfnt(*b, m));
        }
        out
    }

    /// 读取提取结果中指定 tag 的表数据
    fn table_bytes(out: &[u8], tag: [u8; 4]) -> (usize, Vec<u8>) {
        let num = be_u16(out, 4).unwrap() as usize;
        for i in 0..num {
            let rec = 12 + i * 16;
            if out[rec..rec + 4] == tag {
                let offset = be_u32(out, rec + 8).unwrap() as usize;
                let len = be_u32(out, rec + 12).unwrap() as usize;
                return (offset, out[offset..offset + len].to_vec());
            }
        }
        panic!("表 {tag:?} 不存在");
    }

    #[test]
    fn extract_member_roundtrip() {
        let member0: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"beta"), (*b"NAM1", b"alpha")];
        let member1: &[([u8; 4], &[u8])] = &[(*b"CMAP", b"gamma!"), (*b"HEA1", b"beta-longer")];
        let data = build_collection(&[member0, member1]);

        for (index, expected) in [
            (0usize, vec![b"HEA1", b"NAM1"]),
            (1, vec![b"CMAP", b"HEA1"]),
        ] {
            let out = extract_collection_member(&data, index).unwrap();
            // 输出是独立 sfnt 而非集合
            assert_eq!(&out[0..4], &[0, 1, 0, 0]);
            assert_eq!(be_u16(&out, 4).unwrap() as usize, expected.len());
            // 各表数据与偏移对齐保持正确
            for tag in expected {
                let (offset, bytes) = table_bytes(&out, *tag);
                assert_eq!(offset % 4, 0, "表 {tag:?} 未按 4 字节对齐");
                assert!(!bytes.is_empty());
            }
        }

        // 成员 1 的表内容与源一致，校验和占位被保留
        let out = extract_collection_member(&data, 1).unwrap();
        assert_eq!(table_bytes(&out, *b"CMAP").1, b"gamma!");
        assert_eq!(table_bytes(&out, *b"HEA1").1, b"beta-longer");
        let num = be_u16(&out, 4).unwrap() as usize;
        for i in 0..num {
            assert_eq!(&out[12 + i * 16 + 4..12 + i * 16 + 8], &[0xAB; 4]);
        }
    }

    #[test]
    fn frontend_family_from_file_name_conventions() {
        // 与前端 bundledFonts.test.ts 的解析用例保持一致
        assert_eq!(
            frontend_family_from_file_name("Noto Sans SC-VF.woff2"),
            "Noto Sans SC"
        );
        assert_eq!(
            frontend_family_from_file_name("975Maru SC.ttf"),
            "975Maru SC"
        );
        // Google Fonts 命名（英文字重名）与 Adobe 数字字重命名
        assert_eq!(
            frontend_family_from_file_name("SourceHanSansSC-Bold.otf"),
            "SourceHanSansSC"
        );
        assert_eq!(frontend_family_from_file_name("Family-700.ttf"), "Family");
        assert_eq!(
            frontend_family_from_file_name("Roboto-Regular.ttf"),
            "Roboto"
        );
        // 数字字重优先于英文字重名；引号剔除
        assert_eq!(frontend_family_from_file_name("X-300.ttf"), "X");
        assert_eq!(frontend_family_from_file_name("My'Font.ttf"), "MyFont");
        // -VF 判定以原始文件名结尾为准：字重后缀后的 -VF 不剔除
        assert_eq!(frontend_family_from_file_name("X-VF-700.ttf"), "X-VF");
        assert_eq!(frontend_family_from_file_name("X-700-VF.ttf"), "X-700");
        // 无后缀 / 无扩展名
        assert_eq!(frontend_family_from_file_name("Any Font.ttf"), "Any Font");
        assert_eq!(frontend_family_from_file_name("NoExt"), "NoExt");
    }

    #[test]
    fn internal_font_families_rejects_unparseable() {
        // fixture 表数据是任意的，Face::parse 无法解析 → 返回空
        let member: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"x")];
        let single = build_sfnt(0, member);
        assert!(internal_font_families(&single).is_empty());
        assert!(internal_font_families(b"garbage").is_empty());
    }

    #[test]
    fn extract_rejects_invalid_input() {
        assert!(extract_collection_member(&[], 0).is_none());
        assert!(extract_collection_member(b"not a font at all..", 0).is_none());
        let member: &[([u8; 4], &[u8])] = &[(*b"HEA1", b"x")];
        let data = build_collection(&[member]);
        assert!(extract_collection_member(&data, 1).is_none());
    }

    #[test]
    fn member_file_name_conventions() {
        let mut used = HashSet::new();
        let regular = CollectionMemberMeta {
            family: "Source Han Sans SC".to_string(),
            weight: 700,
            variable: false,
            cff: false,
        };
        assert_eq!(
            member_file_name(&regular, &mut used),
            "Source Han Sans SC-700.ttf"
        );

        let vf = CollectionMemberMeta {
            family: "My Font".to_string(),
            weight: 400,
            variable: true,
            cff: true,
        };
        assert_eq!(member_file_name(&vf, &mut used), "My Font-VF.otf");

        // 非法字符与引号剔除
        let dirty = CollectionMemberMeta {
            family: "A:B*c?\"d'e".to_string(),
            weight: 400,
            variable: false,
            cff: false,
        };
        assert_eq!(member_file_name(&dirty, &mut used), "ABcde-400.ttf");

        // 同族同字重去重
        let mut used2 = HashSet::new();
        assert_eq!(
            member_file_name(&regular, &mut used2),
            "Source Han Sans SC-700.ttf"
        );
        assert_eq!(
            member_file_name(&regular, &mut used2),
            "Source Han Sans SC-700-2.ttf"
        );
    }

    /// 用系统自带真实 TTC（微软雅黑）验证提取结果可被正常解析。
    /// 环境中不存在该文件时跳过；同时写出提取文件供外部工具复核
    #[test]
    #[cfg(target_os = "windows")]
    fn extract_real_system_ttc() {
        let path = PathBuf::from("C:\\Windows\\Fonts\\msyh.ttc");
        let Ok(data) = fs::read(&path) else {
            return;
        };
        let out = extract_collection_member(&data, 0).expect("提取 msyh.ttc 成员 0 失败");
        let face = ttf_parser::Face::parse(&out, 0).expect("提取结果无法被 ttf-parser 解析");
        assert!(face.number_of_glyphs() > 1000, "字形数量异常");
        let family = face_family_name(&face).expect("读取族名失败");
        assert!(
            family.contains("YaHei") || family.contains("雅黑"),
            "族名异常: {family}"
        );

        let families = internal_font_families(&data);
        assert!(!families.is_empty(), "真实 TTC 应解析出内部族名");

        let dump = std::env::temp_dir().join("mercurial-player-test-msyh-member0.ttf");
        fs::write(&dump, &out).expect("写出提取结果失败");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn test_extract_font_name() {
        assert_eq!(
            extract_font_name("Arial (TrueType)"),
            Some("Arial".to_string())
        );

        assert_eq!(
            extract_font_name("Microsoft YaHei & Microsoft YaHei UI (TrueType)"),
            Some("Microsoft YaHei".to_string())
        );

        assert_eq!(
            extract_font_name("Segoe UI Bold (TrueType)"),
            Some("Segoe UI".to_string())
        );

        assert_eq!(
            extract_font_name("Consolas (TrueType)"),
            Some("Consolas".to_string())
        );
    }
}
