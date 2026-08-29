#![allow(unsafe_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use super::FONT_CACHE;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteInMemoryFontFileLoader,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_NORMAL,
    HFONT,
};
use windows::core::{Interface, PCWSTR, w};

/// fonts/ 目录外部字体的名字索引：前端族名（小写）→ 候选文件路径。
/// 只记录路径不驻留字体数据，被歌词实际选中的字体才按需加载，
/// 避免像 Super OTC 这类动辄数百 MB 的字体集合整包进入内存
pub(super) struct ExternalFontIndex {
    /// 前端族名（小写）→ 候选文件（常规字重/VF 优先，按优先级升序）
    pub(super) candidates: HashMap<String, Vec<String>>,
    /// 构建时的字体设置代数，用于判断是否需要重建
    pub(super) generation: u32,
}

/// 单个外部字体文件加载出的 DirectWrite 自定义字体集合
pub(super) struct ExternalFontCollection {
    pub(super) collection: IDWriteFontCollection,
    /// CreateTextFormat 应使用的内部族名（字体 name 表的族名，
    /// 可能与前端按文件名解析出的族名不同）
    pub(super) internal_family: String,
}

/// 桌面歌词按常规字重请求文本格式：VF 可变字体优先，其次常规字重
/// （无后缀 / -400 / -regular 等），最后其余字重
fn font_weight_priority(stem: &str) -> u8 {
    let lower = stem.to_lowercase();
    if lower.ends_with("-vf") {
        return 0;
    }
    let Some((_, last)) = lower.rsplit_once('-') else {
        return 1; // 无字重后缀 = Regular
    };
    match last {
        "400" | "regular" | "normal" | "book" => 1,
        // 其余字重后缀（-700、-bold 等）偏离常规字重
        _ => 2,
    }
}

/// 构建外部字体名字索引：只扫描文件名，不读取字体内容。
/// woff/woff2 为 Web 压缩容器，DirectWrite 无法读取，跳过
pub(super) fn build_external_font_index(generation: u32) -> Option<ExternalFontIndex> {
    let entries = crate::system::fonts::list_external_fonts().ok()?;
    let mut candidates: HashMap<String, Vec<(u8, String)>> = HashMap::new();
    for font in entries {
        let ext = font
            .path
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_lowercase();
        if matches!(ext.as_str(), "woff" | "woff2") {
            continue;
        }
        let family = crate::system::fonts::frontend_family_from_file_name(&font.name);
        if family.is_empty() {
            continue;
        }
        let stem = font
            .name
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(&font.name);
        candidates
            .entry(family.to_lowercase())
            .or_default()
            .push((font_weight_priority(stem), font.path));
    }
    if candidates.is_empty() {
        return None;
    }
    let candidates: HashMap<String, Vec<String>> = candidates
        .into_iter()
        .map(|(family, mut files)| {
            files.sort_by_key(|(priority, _)| *priority);
            (family, files.into_iter().map(|(_, path)| path).collect())
        })
        .collect();
    Some(ExternalFontIndex {
        candidates,
        generation,
    })
}

/// 创建并注册 DirectWrite 内存字体加载器。
/// 头文件要求客户端自行调用 RegisterFontFileLoader 注册，未注册的
/// 加载器创建字体文件引用会返回 E_INVALIDARG。
/// 不注销：字体文件引用仅在 loader 保持注册期间有效，注册关系
/// 随工厂存活到进程结束，渲染线程重建时新 loader 单独注册即可
///
/// # Safety
/// 必须在已初始化 COM（`CoInitializeEx`）的渲染线程上调用
pub(super) unsafe fn build_memory_loader(
    dwrite: &IDWriteFactory,
) -> Option<IDWriteInMemoryFontFileLoader> {
    // SAFETY: 调用者保证当前线程已初始化 COM
    let loader = unsafe {
        dwrite
            .cast::<IDWriteFactory5>()
            .ok()?
            .CreateInMemoryFontFileLoader()
            .ok()?
    };
    // SAFETY: loader 刚创建且有效
    match unsafe { dwrite.RegisterFontFileLoader(&loader) } {
        Ok(()) => Some(loader),
        Err(e) => {
            log::warn!("桌面歌词: 注册内存字体加载器失败: {e}");
            None
        }
    }
}

/// 把一个外部字体文件加载为 DirectWrite 自定义字体集合。
/// ownerObject 传 NULL 时 DirectWrite 会复制字体数据，
/// 调用方的临时缓冲在调用后即可释放
///
/// # Safety
/// 必须在已初始化 COM（`CoInitializeEx`）的渲染线程上调用
pub(super) unsafe fn try_load_external_font(
    dwrite: &IDWriteFactory,
    loader: &IDWriteInMemoryFontFileLoader,
    path: &str,
    family: &str,
) -> Option<ExternalFontCollection> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("桌面歌词: 读取外部字体 {path} 失败: {e}");
            return None;
        }
    };
    let internal_families = crate::system::fonts::internal_font_families(&bytes);
    if internal_families.is_empty() {
        log::warn!("桌面歌词: 外部字体 {path} 无法解析内部族名");
        return None;
    }
    let factory5: IDWriteFactory5 = dwrite.cast().ok()?;
    // SAFETY: 调用者保证当前线程已初始化 COM；字体数据被复制管理
    let file_ref = match unsafe {
        loader.CreateInMemoryFontFileReference(
            dwrite,
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            None,
        )
    } {
        Ok(file_ref) => file_ref,
        Err(e) => {
            log::warn!("桌面歌词: 外部字体 {path} 创建字体文件引用失败: {e}");
            return None;
        }
    };
    // SAFETY: 同上
    let builder = unsafe { factory5.CreateFontSetBuilder().ok()? };
    // SAFETY: file_ref 刚创建且有效
    if let Err(e) = unsafe { builder.AddFontFile(&file_ref) } {
        log::warn!("桌面歌词: 外部字体 {path} 加入字体集失败: {e}");
        return None;
    }
    // SAFETY: builder 已成功加入字体文件
    let font_set = unsafe { builder.CreateFontSet().ok()? };
    let collection = unsafe { factory5.CreateFontCollectionFromFontSet(&font_set).ok()? };
    // 优先使用与前端族名一致的内部族名，否则取第一个
    let internal_family = internal_families
        .iter()
        .find(|f| f.eq_ignore_ascii_case(family))
        .cloned()
        .or_else(|| internal_families.first().cloned())?;
    Some(ExternalFontCollection {
        collection: collection.into(),
        internal_family,
    })
}

/// 系统字体集合是否包含该族名（DirectWrite 匹配大小写不敏感）
pub(super) fn family_has_system_face(dwrite: &IDWriteFactory, family: &str) -> bool {
    if family.is_empty() {
        return false;
    }
    let wide: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let mut system: Option<IDWriteFontCollection> = None;
    // SAFETY: system 为栈上局部变量，仅在调用期间被写入；wide 以 NUL 结尾
    unsafe {
        if dwrite
            .GetSystemFontCollection(&raw mut system, false)
            .is_err()
        {
            return false;
        }
    }
    let Some(system) = system else {
        return false;
    };
    let mut index = 0u32;
    let mut exists = windows::core::BOOL::default();
    // SAFETY: index/exists 为栈上局部变量；wide 以 NUL 结尾
    unsafe {
        system
            .FindFamilyName(
                PCWSTR::from_raw(wide.as_ptr()),
                &raw mut index,
                &raw mut exists,
            )
            .is_ok()
            && exists.as_bool()
    }
}

pub(super) fn get_cached_font(font_size_scaled: i32) -> HFONT {
    let cache = FONT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = cache.lock() {
        if let Some(font) = guard.get(&font_size_scaled).copied() {
            return HFONT(font as *mut core::ffi::c_void);
        }

        // SAFETY: w!("Microsoft YaHei") 是 NUL 结尾的 UTF-16 字面量；
        // font_size_scaled 可能为负/零，GDI 会按其绝对值处理，无 UB
        let font = unsafe {
            CreateFontW(
                font_size_scaled,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(6),
                0,
                w!("Microsoft YaHei"),
            )
        };
        guard.insert(font_size_scaled, font.0 as usize);
        font
    } else {
        // SAFETY: 同上分支
        unsafe {
            CreateFontW(
                font_size_scaled,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                FONT_CHARSET(1),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(6),
                0,
                w!("Microsoft YaHei"),
            )
        }
    }
}
