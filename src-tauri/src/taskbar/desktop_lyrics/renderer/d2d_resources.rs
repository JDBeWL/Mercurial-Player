//! Direct2D / DirectWrite 资源生命周期管理。
//!
//! 持有渲染线程的 [`Direct2DState`]：D2D 工厂、DC 渲染目标、文本格式缓存、
//! 外部字体名字索引与内存字体加载器，负责这些资源的创建、缓存与按代数重建。
//! 所有 unsafe 方法都必须在已初始化 COM（`CoInitializeEx`）的
//! desktop-lyrics 渲染线程上调用。

use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteInMemoryFontFileLoader, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::{PCWSTR, w};

use super::super::SHARED_STATE;
use super::super::fonts::{
    ExternalFontCollection, ExternalFontIndex, build_external_font_index, build_memory_loader,
    family_has_system_face, try_load_external_font,
};

thread_local! {
    pub(super) static D2D_STATE: RefCell<Option<Direct2DState>> = const { RefCell::new(None) };
}

pub(super) struct Direct2DState {
    pub(super) dwrite_factory: IDWriteFactory,
    pub(super) dc_render_target: ID2D1DCRenderTarget,
    /// 键为 (字体族, 字号)：字体设置运行时可变，族名必须参与缓存键
    text_format_cache: HashMap<(String, i32), IDWriteTextFormat>,
    /// fonts/ 目录外部字体的名字索引（懒构建，随 SharedLyricState::font_generation 重建）
    external_index: Option<ExternalFontIndex>,
    /// 内存字体加载器（懒创建，创建时自动注册到 factory；字体文件引用
    /// 与其绑定，需在渲染线程存活期间一直持有）
    memory_loader: Option<IDWriteInMemoryFontFileLoader>,
    /// 已按需加载的外部字体：前端族名（小写）→ 自定义字体集合
    external_collections: HashMap<String, ExternalFontCollection>,
}

impl Direct2DState {
    /// # Safety
    /// 创建 Direct2D / DirectWrite 工厂与 DC 渲染目标。必须在已初始化 COM
    /// （`CoInitializeEx`）的线程上调用，且仅在同一线程内使用单线程工厂。
    pub(super) unsafe fn new() -> windows::core::Result<Self> {
        // SAFETY: 调用者保证当前线程已执行 CoInitializeEx
        let d2d_factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        // SAFETY: 同上
        let dwrite_factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let target_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: target_props 是栈上局部变量，&raw const 取其地址传给 Win32 后不再使用
        let dc_render_target =
            unsafe { d2d_factory.CreateDCRenderTarget(&raw const target_props)? };
        // SAFETY: dc_render_target 刚创建，调用设置方法符合 COM 契约
        unsafe { dc_render_target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
        Ok(Self {
            dwrite_factory,
            dc_render_target,
            text_format_cache: HashMap::new(),
            external_index: None,
            memory_loader: None,
            external_collections: HashMap::new(),
        })
    }

    /// 懒构建/按代数重建外部字体名字索引。
    /// 指向的文件可能已变化，重建时同时丢弃按需加载的集合与文本格式缓存
    fn ensure_external_index(&mut self) {
        // 阻塞锁：try_lock 失败时读不到代数，会把回退格式的结果缓存下来
        // 且永远不会再重建，这里必须保证读到真实代数
        let Some(generation) = SHARED_STATE
            .get()
            .map(|state| lock_or_log!(state.lock()).font_generation)
        else {
            return;
        };
        let stale = match &self.external_index {
            Some(index) => index.generation != generation,
            None => true,
        };
        if stale {
            self.external_index = build_external_font_index(generation);
            self.external_collections.clear();
            self.text_format_cache.clear();
        }
    }

    /// 懒创建内存字体加载器（需要 IDWriteFactory5，Windows 10 1709+；
    /// 不可用时外部字体功能整体禁用，回落系统字体）。
    /// 加载器必须注册到 factory 后才能创建字体文件引用
    fn ensure_memory_loader(&mut self) -> Option<&IDWriteInMemoryFontFileLoader> {
        if self.memory_loader.is_none() {
            // SAFETY: Direct2DState 方法仅在已初始化 COM 的渲染线程（D2D_STATE）调用
            self.memory_loader = unsafe { build_memory_loader(&self.dwrite_factory) };
        }
        self.memory_loader.as_ref()
    }

    /// 解析 family 实际使用的字体集合与族名：系统已安装的字体优先
    /// （与前端 @font-face 中 local() 优先的语义一致），否则按需加载
    /// fonts/ 目录下同名族的外部字体，都不存在时交给系统回退
    fn resolve_font_source(&mut self, family: &str) -> (Option<IDWriteFontCollection>, String) {
        if family_has_system_face(&self.dwrite_factory, family) {
            return (None, family.to_string());
        }
        self.ensure_external_index();
        let key = family.to_lowercase();
        if let Some(entry) = self.external_collections.get(&key) {
            return (
                Some(entry.collection.clone()),
                entry.internal_family.clone(),
            );
        }
        let Some(paths) = self
            .external_index
            .as_ref()
            .and_then(|index| index.candidates.get(&key))
            .cloned()
        else {
            return (None, family.to_string());
        };
        let Some(loader) = self.ensure_memory_loader().cloned() else {
            return (None, family.to_string());
        };
        for path in &paths {
            // SAFETY: 当前处于 D2D_STATE 渲染线程上下文（COM 已初始化）
            if let Some(loaded) =
                unsafe { try_load_external_font(&self.dwrite_factory, &loader, path, family) }
            {
                self.external_collections.insert(
                    key,
                    ExternalFontCollection {
                        collection: loaded.collection.clone(),
                        internal_family: loaded.internal_family.clone(),
                    },
                );
                return (Some(loaded.collection), loaded.internal_family);
            }
        }
        (None, family.to_string())
    }

    /// # Safety
    /// 创建/缓存 `IDWriteTextFormat`，必须在已初始化 COM 的同一线程调用。
    /// `font_size_scaled` 会被 `max(1)` 钳制，避免传入 0 或负数。
    unsafe fn text_format(
        &mut self,
        font_size_scaled: i32,
        family: &str,
    ) -> windows::core::Result<IDWriteTextFormat> {
        let cache_key = (family.to_string(), font_size_scaled);
        if let Some(format) = self.text_format_cache.get(&cache_key) {
            return Ok(format.clone());
        }
        let (collection, resolved_family) = self.resolve_font_source(family);
        // SAFETY: family_wide 以 NUL 结尾且在本调用期间存活；
        // font_size 经 max(1) 保证为正
        let family_wide: Vec<u16> = resolved_family
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let format = unsafe {
            self.dwrite_factory.CreateTextFormat(
                PCWSTR::from_raw(family_wide.as_ptr()),
                collection.as_ref(),
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size_scaled.max(1) as f32,
                w!("zh-cn"),
            )?
        };
        // SAFETY: format 刚创建，设置对齐/换行属性符合 COM 契约
        unsafe {
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        }
        self.text_format_cache.insert(cache_key, format.clone());
        Ok(format)
    }

    /// # Safety
    /// 创建 `IDWriteTextLayout`，`text` 必须是有效的 UTF-16 切片（允许含尾部 NUL），
    /// width/height 经 `max(1.0)` 钳制。必须在已初始化 COM 的同一线程调用。
    pub(super) unsafe fn create_layout(
        &mut self,
        text: &[u16],
        font_size_scaled: i32,
        width: f32,
        height: f32,
        family: &str,
    ) -> windows::core::Result<IDWriteTextLayout> {
        let text = trim_utf16_nul(text);
        // SAFETY: 转发到 text_format，契约一致
        let format = unsafe { self.text_format(font_size_scaled, family)? };
        // SAFETY: text 是 &[u16]，PCWSTR 要求的 NUL 由 CreateTextLayout 内部处理
        unsafe {
            self.dwrite_factory
                .CreateTextLayout(text, &format, width.max(1.0), height.max(1.0))
        }
    }
}

/// 去掉文本切片尾部的单个 UTF-16 NUL 终止符（若有）
pub(super) fn trim_utf16_nul(text: &[u16]) -> &[u16] {
    if text.last().copied() == Some(0) {
        &text[..text.len().saturating_sub(1)]
    } else {
        text
    }
}
