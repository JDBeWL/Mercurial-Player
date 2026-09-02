//! 字体二进制解析与命名（TTC/OTC 集合成员提取、族名/字重解析）。

use std::collections::HashSet;
use std::path::Path;

pub(super) fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

pub(super) fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// 从 TTC/OTC 集合中提取指定成员为独立字体文件。
///
/// 定位成员的 sfnt 表目录，逐表原样拷贝数据、重建目录偏移。
/// 表数据未被修改，各表自身的校验和依然有效；浏览器不校验
/// head.checkSumAdjustment，因此无需重算整体校验和。
pub(super) fn extract_collection_member(data: &[u8], index: usize) -> Option<Vec<u8>> {
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
pub(super) fn extract_sfnt_directory(data: &[u8], dir_offset: usize) -> Option<Vec<u8>> {
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
pub(super) struct CollectionMemberMeta {
    pub(super) family: String,
    pub(super) weight: u16,
    pub(super) variable: bool,
    /// CFF 轮廓（决定提取文件的扩展名 otf / ttf）
    pub(super) cff: bool,
}

/// 解析集合内全部成员的元信息；非集合数据视为单成员
pub(super) fn collection_member_metas(data: &[u8]) -> Vec<CollectionMemberMeta> {
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
pub(super) fn face_family_name(face: &ttf_parser::Face) -> Option<String> {
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
pub(super) fn sanitize_file_name(s: &str) -> String {
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
pub(super) fn member_file_name(meta: &CollectionMemberMeta, used: &mut HashSet<String>) -> String {
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
