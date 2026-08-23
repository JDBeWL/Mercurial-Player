//! 桌面歌词显示模块
//!
//! 在屏幕底部显示一个置顶窗口，展示当前播放歌词。
//! 使用 Direct2D 渲染，确保文字边缘平滑无毛刺。
//! 支持点击穿透（锁定模式）和顶部拖拽（解锁模式）。
//! 悬浮时显示锁定按钮和关闭按钮。
//! 支持双行歌词。
//! 字体族跟随前端歌词字体设置（原文/译文可分别指定）：
//! 优先使用系统已安装字体，未安装时尝试把 fonts/ 目录的外部字体
//! 加载为 DirectWrite 内存字体集后使用。

#![allow(unsafe_code)]

use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tauri::command;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_HIT_TEST_METRICS, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFactory5, IDWriteFontCollection, IDWriteInMemoryFontFileLoader,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::core::{Interface, PCWSTR, w};

use unicode_segmentation::UnicodeSegmentation;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, FONT_CHARSET,
    FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, FW_NORMAL, GetDC, HBRUSH, HFONT,
    HGDIOBJ, InvalidateRect, PAINTSTRUCT, ReleaseDC, RGBQUAD, SRCCOPY, ScreenToClient,
    SelectObject, SetBkMode, StretchBlt, TRANSPARENT,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GWL_EXSTYLE,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect, HCURSOR, HICON,
    HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTRANSPARENT, HWND_TOPMOST, IsWindowVisible, KillTimer,
    MSG, PostMessageW, PostQuitMessage, RegisterClassW, SET_WINDOW_POS_FLAGS, SW_HIDE,
    SW_SHOWNOACTIVATE, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    ULW_ALPHA, UpdateLayeredWindow, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_numerics::Vector2;

const WM_MOUSELEAVE: u32 = 0x02A3;
#[allow(non_camel_case_types)]
type DPI_AWARENESS_CONTEXT = *mut core::ffi::c_void;

/// # Safety
/// 调用 Win32 `SetThreadDpiAwarenessContext`，传入的句柄必须是有效的 DPI 上下文指针
/// （如 `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`）或 `NULL`。仅在拥有消息循环的
/// 桌面歌词线程上调用，避免跨线程干扰主窗口的 DPI 感知。
unsafe fn set_thread_dpi_awareness_context(ctx: DPI_AWARENESS_CONTEXT) -> DPI_AWARENESS_CONTEXT {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetThreadDpiAwarenessContext(dpiContext: DPI_AWARENESS_CONTEXT)
        -> DPI_AWARENESS_CONTEXT;
    }
    // SAFETY: ctx 来自常量或本函数调用者，参数语义符合 Win32 契约
    unsafe { SetThreadDpiAwarenessContext(ctx) }
}

/// # Safety
/// 调用 Win32 `GetDpiForWindow`，`hwnd` 必须是有效的窗口句柄。
unsafe fn get_dpi_for_window(hwnd: HWND) -> u32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForWindow(hwnd: HWND) -> u32;
    }
    // SAFETY: 调用者保证 hwnd 是已创建的窗口句柄
    unsafe { GetDpiForWindow(hwnd) }
}

const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: DPI_AWARENESS_CONTEXT =
    (-4isize) as *mut core::ffi::c_void;

const WM_DL_UPDATE: u32 = WM_USER + 0x100;
const HOVER_TIMER_ID: usize = 1001;
const CLOSE_BTN_SIZE: i32 = 28;
const CLOSE_BTN_MARGIN: i32 = 6;
const RESIZE_EDGE_WIDTH: i32 = 18;
const MARQUEE_SPEED_PX_PER_SEC: i32 = 42;

/// 配色方案。outline 为文字描边色：取与文字色同色系的深色
/// （深/浅预设用灰阶），比纯黑描边更柔和，同时保证任意背景可读
#[derive(Clone, Copy, Debug)]
struct ColorPreset {
    text_color: u32,
    highlight_color: u32,
    outline_color: u32,
    outline_alpha: f32,
}

const PRESET_DARK: ColorPreset = ColorPreset {
    text_color: 0x00_21_21_21,
    highlight_color: 0x00_02_88_D1,
    outline_color: 0x00_EC_EF_F1,
    outline_alpha: 0.9,
};

const PRESET_LIGHT: ColorPreset = ColorPreset {
    text_color: 0x00_FF_FF_FF,
    highlight_color: 0x00_FF_C5_3D,
    outline_color: 0x00_37_47_4F,
    outline_alpha: 0.8,
};

const PRESET_BLUE: ColorPreset = ColorPreset {
    text_color: 0x00_03_A9_F4,
    highlight_color: 0x00_81_D4_FA,
    outline_color: 0x00_01_57_9B,
    outline_alpha: 0.7,
};

const PRESET_PINK: ColorPreset = ColorPreset {
    text_color: 0x00_E9_1E_63,
    highlight_color: 0x00_FF_80_AB,
    outline_color: 0x00_88_0E_4F,
    outline_alpha: 0.7,
};

const PRESET_ORANGE: ColorPreset = ColorPreset {
    text_color: 0x00_FF_98_00,
    highlight_color: 0x00_FF_CC_80,
    outline_color: 0x00_BF_36_0C,
    outline_alpha: 0.7,
};

const PRESET_GREEN: ColorPreset = ColorPreset {
    text_color: 0x00_4C_AF_50,
    highlight_color: 0x00_A5_D6_A7,
    outline_color: 0x00_1B_5E_20,
    outline_alpha: 0.7,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricWord {
    text: String,
    start: f32,
    end: f32,
}

struct SharedLyricState {
    current_line: String,
    sub_line: String,
    prev_line: String,
    prev_sub_line: String,
    current_words: Vec<DesktopLyricWord>,
    font_size: i32,
    /// 原文歌词字体族（来自前端歌词字体设置，渲染前由前端同步）
    font_family: String,
    /// 译文字体族（空字符串 = 跟随原文）
    translation_font_family: String,
    /// 字体设置代数：变化时渲染线程重建外部字体内存字体集
    font_generation: u32,
    is_locked: bool,
    is_hovered: bool,
    is_playing: bool,
    color_preset: ColorPreset,
    /// 配色为 auto：按窗口背后背景亮度在深/浅文字间自动切换
    preset_auto: bool,
    /// auto 模式下当前使用浅色文字（背景偏暗时为 true）
    auto_light_text: bool,
    /// 上次背景采样时间（毫秒），限流采样频率
    last_bg_sample_ms: i64,
    fade_alpha: f32,
    fade_pending: bool,
    marquee_active: bool,
    marquee_start_ms: i64,
    visual_time: f32,
    target_time: f32,
    visual_time_last_ms: i64,
    lyric_progress: f32,
    smooth_lyric_progress: f32,
    render_pending: bool,
}

static FONT_CACHE: OnceLock<Mutex<HashMap<i32, usize>>> = OnceLock::new();

static SHARED_STATE: OnceLock<Arc<Mutex<SharedLyricState>>> = OnceLock::new();
static LYRICS_HWND: OnceLock<isize> = OnceLock::new();

thread_local! {
    static D2D_STATE: RefCell<Option<Direct2DState>> = const { RefCell::new(None) };
}

struct Direct2DState {
    dwrite_factory: IDWriteFactory,
    dc_render_target: ID2D1DCRenderTarget,
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

/// fonts/ 目录外部字体的名字索引：前端族名（小写）→ 候选文件路径。
/// 只记录路径不驻留字体数据，被歌词实际选中的字体才按需加载，
/// 避免像 Super OTC 这类动辄数百 MB 的字体集合整包进入内存
struct ExternalFontIndex {
    /// 前端族名（小写）→ 候选文件（常规字重/VF 优先，按优先级升序）
    candidates: HashMap<String, Vec<String>>,
    /// 构建时的字体设置代数，用于判断是否需要重建
    generation: u32,
}

/// 单个外部字体文件加载出的 DirectWrite 自定义字体集合
struct ExternalFontCollection {
    collection: IDWriteFontCollection,
    /// CreateTextFormat 应使用的内部族名（字体 name 表的族名，
    /// 可能与前端按文件名解析出的族名不同）
    internal_family: String,
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
fn build_external_font_index(generation: u32) -> Option<ExternalFontIndex> {
    let entries = crate::system::fonts::list_external_fonts().ok()?;
    let mut candidates: HashMap<String, Vec<(u8, String)>> = HashMap::new();
    for font in entries {
        let ext = font.path.rsplit('.').next().unwrap_or_default().to_lowercase();
        if matches!(ext.as_str(), "woff" | "woff2") {
            continue;
        }
        let family = crate::system::fonts::frontend_family_from_file_name(&font.name);
        if family.is_empty() {
            continue;
        }
        let stem = font.name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&font.name);
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
unsafe fn build_memory_loader(dwrite: &IDWriteFactory) -> Option<IDWriteInMemoryFontFileLoader> {
    // SAFETY: 调用者保证当前线程已初始化 COM
    let loader = unsafe {
        dwrite.cast::<IDWriteFactory5>()
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
unsafe fn try_load_external_font(
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
fn family_has_system_face(dwrite: &IDWriteFactory, family: &str) -> bool {
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
            .FindFamilyName(PCWSTR::from_raw(wide.as_ptr()), &raw mut index, &raw mut exists)
            .is_ok()
            && exists.as_bool()
    }
}

impl Direct2DState {
    /// # Safety
    /// 创建 Direct2D / DirectWrite 工厂与 DC 渲染目标。必须在已初始化 COM
    /// （`CoInitializeEx`）的线程上调用，且仅在同一线程内使用单线程工厂。
    unsafe fn new() -> windows::core::Result<Self> {
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
            return (Some(entry.collection.clone()), entry.internal_family.clone());
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
    unsafe fn create_layout(
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

fn trim_utf16_nul(text: &[u16]) -> &[u16] {
    if text.last().copied() == Some(0) {
        &text[..text.len().saturating_sub(1)]
    } else {
        text
    }
}

fn rect_to_d2d(rect: &RECT) -> D2D_RECT_F {
    D2D_RECT_F {
        left: rect.left as f32,
        top: rect.top as f32,
        right: rect.right as f32,
        bottom: rect.bottom as f32,
    }
}

fn rounded_rect_to_d2d(rect: &RECT, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect: rect_to_d2d(rect),
        radiusX: radius,
        radiusY: radius,
    }
}

fn d2d_color(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
        g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
        b: (rgb & 0xFF) as f32 / 255.0,
        a: alpha.clamp(0.0, 1.0),
    }
}

fn lerp_d2d_color(from: D2D1_COLOR_F, to: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    let t = t.clamp(0.0, 1.0);
    D2D1_COLOR_F {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

fn hit_test_text_x(layout: &IDWriteTextLayout, pos: u32, trailing: bool) -> Option<f32> {
    let mut x = 0.0f32;
    let mut y = 0.0f32;
    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
    // SAFETY: layout 是有效 COM 对象；x/y/metrics 均为栈上局部变量，
    // &raw mut 传给 Win32 写入后在本函数内读取，不存在别名冲突
    unsafe {
        layout
            .HitTestTextPosition(pos, trailing, &raw mut x, &raw mut y, &raw mut metrics)
            .ok()?;
    }
    Some(x)
}

fn progress_cluster_phase(text: &[u16], progress: f32) -> Option<f32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return None;
    }
    let progress = progress.clamp(0.0, 1.0);
    let decoded = String::from_utf16_lossy(text);
    let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(decoded.as_str(), true).collect();
    if graphemes.is_empty() {
        return None;
    }

    let exact = (graphemes.len() as f32) * progress;
    let highlighted = exact.floor() as usize;
    if highlighted >= graphemes.len() {
        return Some(1.0);
    }

    let phase = exact - highlighted as f32;
    Some(phase * phase * (3.0 - 2.0 * phase))
}

fn progress_clip_end_x(layout: &IDWriteTextLayout, text: &[u16], progress: f32) -> Option<f32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return None;
    }
    let progress = progress.clamp(0.0, 1.0);
    let decoded = String::from_utf16_lossy(text);
    let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(decoded.as_str(), true).collect();
    if graphemes.is_empty() {
        return None;
    }

    let exact = (graphemes.len() as f32) * progress;
    let highlighted = exact.floor() as usize;
    let current_t = exact - highlighted as f32;
    if highlighted >= graphemes.len() {
        return None;
    }

    let mut utf16_idx = 0usize;
    for (cluster_idx, grapheme) in graphemes.iter().enumerate() {
        let len = grapheme.encode_utf16().count();
        if cluster_idx == highlighted {
            let start = utf16_idx as u32;
            let end = (utf16_idx + len) as u32;
            let start_x = hit_test_text_x(layout, start, false)?;
            let end_x = hit_test_text_x(layout, end, false)
                .or_else(|| hit_test_text_x(layout, end.saturating_sub(1), true))?;
            let eased = current_t * current_t * (3.0 - 2.0 * current_t);
            return Some(start_x + (end_x - start_x) * eased);
        }
        utf16_idx += len;
    }

    None
}

fn karaoke_progress_from_words(words: &[DesktopLyricWord], visual_time: f32, fallback: f32) -> f32 {
    let valid: Vec<&DesktopLyricWord> = words
        .iter()
        .filter(|word| !word.text.is_empty() && word.end > word.start)
        .collect();
    if valid.is_empty() {
        return fallback.clamp(0.0, 1.0);
    }

    let first_start = valid[0].start;
    let last_end = valid[valid.len() - 1].end;
    if visual_time <= first_start {
        return 0.0;
    }
    if visual_time >= last_end {
        return 1.0;
    }

    let total_units: usize = valid
        .iter()
        .map(|word| UnicodeSegmentation::graphemes(word.text.as_str(), true).count())
        .sum();
    if total_units == 0 {
        return fallback.clamp(0.0, 1.0);
    }

    let mut passed_units = 0.0f32;
    for word in valid {
        let units = UnicodeSegmentation::graphemes(word.text.as_str(), true).count() as f32;
        if visual_time >= word.end {
            passed_units += units;
            continue;
        }
        if visual_time > word.start {
            let local = ((visual_time - word.start) / (word.end - word.start)).clamp(0.0, 1.0);
            passed_units += units * local;
        }
        break;
    }

    (passed_units / total_units as f32).clamp(0.0, 1.0)
}

fn draw_d2d_lyric_line(
    state: &mut Direct2DState,
    text: &[u16],
    rect: RECT,
    font_size_scaled: i32,
    family: &str,
    scale: i32,
    marquee_start_ms: i64,
    text_color: D2D1_COLOR_F,
    highlight_color: D2D1_COLOR_F,
    outline_color: D2D1_COLOR_F,
    base_brush: &ID2D1SolidColorBrush,
    highlight_brush: &ID2D1SolidColorBrush,
    clip_progress: f32,
    color_progress: f32,
    line_duration_ms: Option<i64>,
) -> Option<bool> {
    let text_w = measure_text_width_dwrite_with_state(
        state,
        text,
        rect.bottom - rect.top,
        font_size_scaled,
        family,
    )
    .unwrap_or_else(|| (rect.right - rect.left).max(1));
    let avail_w = (rect.right - rect.left).max(1);
    let base_speed = (MARQUEE_SPEED_PX_PER_SEC * scale / 96).max(12) as i64;
    let max_scroll = (text_w - avail_w).max(0);
    let (hold_ms, speed) = if max_scroll > 0 {
        if let Some(duration) = line_duration_ms {
            // 动态计算：确保在行结束前滚完
            let hold = if duration <= 1200 {
                100i64
            } else if duration <= 2500 {
                200
            } else {
                400.min(duration / 4)
            };
            let budget = (duration - hold).max(200);
            let needed = ((max_scroll as i64 * 1000 + budget - 1) / budget).max(base_speed);
            (hold, needed)
        } else {
            (900i64, base_speed)
        }
    } else {
        (900i64, base_speed)
    };
    let elapsed = (now_ms() - marquee_start_ms).max(0);
    let offset = if elapsed <= hold_ms {
        0
    } else {
        (((elapsed - hold_ms) * speed) / 1000).min(max_scroll as i64) as i32
    };
    let should_animate = offset < max_scroll;
    let draw_rect = if text_w <= avail_w {
        let left = rect.left + (avail_w - text_w) / 2;
        RECT {
            left,
            top: rect.top,
            right: left + text_w,
            bottom: rect.bottom,
        }
    } else {
        RECT {
            left: rect.left - offset,
            top: rect.top,
            right: rect.left - offset + text_w,
            bottom: rect.bottom,
        }
    };
    let width = (draw_rect.right - draw_rect.left).max(1) as f32;
    let height = (draw_rect.bottom - draw_rect.top).max(1) as f32;
    let origin = Vector2 {
        X: draw_rect.left as f32,
        Y: draw_rect.top as f32,
    };

    // 单个字符时不使用 clip 方式，直接用颜色插值过渡
    let decoded = String::from_utf16_lossy(trim_utf16_nul(text));
    let grapheme_count = UnicodeSegmentation::graphemes(decoded.as_str(), true).count();
    if grapheme_count <= 1 {
        let t = clip_progress.clamp(0.0, 1.0);
        let blended = lerp_d2d_color(text_color, highlight_color, t);
        // SAFETY: blended/outline_color 是栈上局部变量，&raw const 仅在该 COM 调用期间被读取
        let brush = unsafe {
            state
                .dc_render_target
                .CreateSolidColorBrush(&raw const blended, None)
                .ok()?
        };
        let outline_brush = unsafe {
            state
                .dc_render_target
                .CreateSolidColorBrush(&raw const outline_color, None)
                .ok()?
        };
        // SAFETY: create_layout 要求 COM 已初始化，调用方在 D2D_STATE 线程上下文
        let layout = unsafe {
            state
                .create_layout(text, font_size_scaled, width, height, family)
                .ok()?
        };
        // SAFETY: layout/outline_brush 均为有效 COM 对象
        unsafe {
            for o in outline_offsets(scale) {
                state.dc_render_target.DrawTextLayout(
                    Vector2 {
                        X: origin.X + o.0,
                        Y: origin.Y + o.1,
                    },
                    &layout,
                    &outline_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            state.dc_render_target.DrawTextLayout(
                origin,
                &layout,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        return Some(should_animate);
    }

    // SAFETY: 同上，create_layout 要求 COM 已初始化
    let base_layout = unsafe {
        state
            .create_layout(text, font_size_scaled, width, height, family)
            .ok()?
    };
    // SAFETY: 同上
    let overlay_layout = unsafe {
        state
            .create_layout(text, font_size_scaled, width, height, family)
            .ok()?
    };
    // 以描边色向 8 个方向偏移绘制底层文本，保证任意背景下的可读性
    let outline_brush = unsafe {
        state
            .dc_render_target
            .CreateSolidColorBrush(&raw const outline_color, None)
            .ok()?
    };
    // SAFETY: base_layout/outline_brush 均为有效 COM 对象
    unsafe {
        for o in outline_offsets(scale) {
            state.dc_render_target.DrawTextLayout(
                Vector2 {
                    X: origin.X + o.0,
                    Y: origin.Y + o.1,
                },
                &base_layout,
                &outline_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        state.dc_render_target.DrawTextLayout(
            origin,
            &base_layout,
            base_brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
        );
    }

    let current_phase = progress_cluster_phase(text, color_progress).unwrap_or(color_progress);
    let mut current_color = lerp_d2d_color(text_color, highlight_color, current_phase);
    current_color.a = (current_color.a * (0.35 + 0.65 * current_phase)).clamp(0.0, 1.0);
    // SAFETY: current_color 是栈上局部变量，&raw const 仅在该 COM 调用期间被读取
    let current_brush = unsafe {
        state
            .dc_render_target
            .CreateSolidColorBrush(&raw const current_color, None)
            .ok()?
    };
    apply_progress_effects(
        &overlay_layout,
        text,
        color_progress,
        base_brush,
        &current_brush,
        highlight_brush,
    );
    if let Some(clip_x) = progress_clip_end_x(&overlay_layout, text, clip_progress) {
        let clip = D2D_RECT_F {
            left: draw_rect.left as f32,
            top: draw_rect.top as f32,
            right: (draw_rect.left as f32 + clip_x).min(draw_rect.right as f32),
            bottom: draw_rect.bottom as f32,
        };
        // SAFETY: clip 是栈上局部变量；PushAxisAlignedClip 与 PopAxisAlignedClip
        // 必须配对，此处成对调用符合 Direct2D 剪裁栈契约
        unsafe {
            state
                .dc_render_target
                .PushAxisAlignedClip(&raw const clip, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            state.dc_render_target.DrawTextLayout(
                origin,
                &overlay_layout,
                base_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            state.dc_render_target.PopAxisAlignedClip();
        }
    } else {
        // SAFETY: overlay_layout/base_brush 均为有效 COM 对象
        unsafe {
            state.dc_render_target.DrawTextLayout(
                origin,
                &overlay_layout,
                base_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
    }

    Some(should_animate)
}

fn render_lyrics_d2d_frame(
    _hwnd: HWND,
    hdc_mem: windows::Win32::Graphics::Gdi::HDC,
    client_rect: &RECT,
    scale: i32,
    is_hovered: bool,
    is_locked: bool,
    current_line: &str,
    sub_line: &str,
    font_size: i32,
    font_family: &str,
    translation_font_family: &str,
    preset: ColorPreset,
    fade_alpha: f32,
    marquee_start_ms: i64,
    _lyric_progress: f32,
    smooth_lyric_progress: f32,
    line_duration_ms: Option<i64>,
) -> Option<bool> {
    let mut should_animate = false;
    D2D_STATE.with(|cell| {
        let mut state_ref = cell.borrow_mut();
        if state_ref.is_none() {
            // SAFETY: 该闭包在 desktop-lyrics 消息循环线程执行，该线程已 CoInitializeEx
            let state = unsafe { Direct2DState::new().ok()? };
            *state_ref = Some(state);
        }
        let state = state_ref.as_mut()?;
        // SAFETY: 所有 COM 调用均在已初始化 COM 的 desktop-lyrics 线程上执行；
        // &raw const 的局部变量仅在对应 COM 调用期间被读取，无别名冲突；
        // BeginDraw/EndDraw 配对调用符合 Direct2D 渲染契约
        unsafe {
            state.dc_render_target.BindDC(hdc_mem, client_rect).ok()?;
            state.dc_render_target.BeginDraw();
            state.dc_render_target.Clear(None);

            let font_size_scaled = font_size * scale / 96;
            let layout = build_lyrics_layout(client_rect, scale);
            let corner_radius = (16 * scale / 96) as f32;
            let strip_radius = (10 * scale / 96) as f32;

            if is_hovered {
                // 简单黑色遮罩卡片
                // 外层 unsafe 块保证 COM 已初始化
                draw_hover_card(state, client_rect, corner_radius);

                let btn_text_format = state
                    .dwrite_factory
                    .CreateTextFormat(
                        w!("Segoe MDL2 Assets"),
                        None::<&IDWriteFontCollection>,
                        DWRITE_FONT_WEIGHT_NORMAL,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        (18 * scale / 96).max(1) as f32,
                        w!("zh-cn"),
                    )
                    .ok()?;
                let _ = btn_text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = btn_text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = btn_text_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);

                let close_rect = layout.close_rect;
                let lock_rect = layout.lock_rect;
                if !is_locked {
                    let close_bg = d2d_color(0xD3_2F_2F, 0.30);
                    let close_brush = state
                        .dc_render_target
                        .CreateSolidColorBrush(&raw const close_bg, None)
                        .ok()?;
                    let close_round = rounded_rect_to_d2d(&close_rect, strip_radius);
                    state
                        .dc_render_target
                        .FillRoundedRectangle(&raw const close_round, &close_brush);
                }
                let lock_bg = if is_locked {
                    d2d_color(0x19_76_D2, 0.30)
                } else {
                    d2d_color(0xFF_FFFF, 0.12)
                };
                let lock_brush = state
                    .dc_render_target
                    .CreateSolidColorBrush(&raw const lock_bg, None)
                    .ok()?;
                let lock_round = rounded_rect_to_d2d(&lock_rect, strip_radius);
                state
                    .dc_render_target
                    .FillRoundedRectangle(&raw const lock_round, &lock_brush);

                // 深色卡片上用亮色图标：锁定=亮蓝，未锁定/关闭=亮红
                let icon_color = if is_locked {
                    d2d_color(0x90_CA_F9, 1.0)
                } else {
                    d2d_color(0xFF_8A_80, 1.0)
                };
                let icon_brush = state
                    .dc_render_target
                    .CreateSolidColorBrush(&raw const icon_color, None)
                    .ok()?;
                if !is_locked {
                    state.dc_render_target.DrawText(
                        &[0xE711],
                        &btn_text_format,
                        &rect_to_d2d(&close_rect),
                        &icon_brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
                state.dc_render_target.DrawText(
                    &[if is_locked { 0xE72E } else { 0xE785 }],
                    &btn_text_format,
                    &rect_to_d2d(&lock_rect),
                    &icon_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            if !current_line.is_empty() {
                let text_color = d2d_color(preset.text_color, fade_alpha);
                let highlight_color = d2d_color(preset.highlight_color, fade_alpha);
                let outline_color =
                    d2d_color(preset.outline_color, preset.outline_alpha * fade_alpha);
                let base_brush = state
                    .dc_render_target
                    .CreateSolidColorBrush(&raw const text_color, None)
                    .ok()?;
                let highlight_brush = state
                    .dc_render_target
                    .CreateSolidColorBrush(&raw const highlight_color, None)
                    .ok()?;
                let current_vec: Vec<u16> = current_line.encode_utf16().collect();
                let sub_vec: Vec<u16> = sub_line.encode_utf16().collect();
                let current = trim_utf16_nul(&current_vec);
                let sub = trim_utf16_nul(&sub_vec);
                if sub.is_empty() {
                    should_animate |= draw_d2d_lyric_line(
                        state,
                        current,
                        layout.text_rect,
                        font_size_scaled,
                        font_family,
                        scale,
                        marquee_start_ms,
                        text_color,
                        highlight_color,
                        outline_color,
                        &base_brush,
                        &highlight_brush,
                        smooth_lyric_progress,
                        smooth_lyric_progress,
                        line_duration_ms,
                    )?;
                } else {
                    // 译文行独立跟随译文字体设置（空 = 跟随原文）
                    let sub_family = if translation_font_family.is_empty() {
                        font_family
                    } else {
                        translation_font_family
                    };
                    // 按两行各自的字体度量高度紧凑排版并整体垂直居中
                    let fallback_h = (font_size_scaled * 5 / 4).max(1);
                    let upper_h = measure_text_height_dwrite_with_state(
                        state,
                        current,
                        font_size_scaled,
                        font_family,
                    )
                    .unwrap_or(fallback_h)
                    .max(1);
                    let lower_h =
                        measure_text_height_dwrite_with_state(state, sub, font_size_scaled, sub_family)
                            .unwrap_or(fallback_h)
                            .max(1);
                    let gap = (6 * scale / 96).max(2);
                    let avail = layout.text_rect.bottom - layout.text_rect.top;
                    let group_top =
                        layout.text_rect.top + (avail - (upper_h + lower_h + gap)).max(0) / 2;
                    let lower_top = group_top + upper_h + gap;
                    let upper_rect = RECT {
                        top: group_top,
                        bottom: group_top + upper_h,
                        ..layout.text_rect
                    };
                    let lower_rect = RECT {
                        top: lower_top,
                        bottom: lower_top + lower_h,
                        ..layout.text_rect
                    };
                    for (text, rect, family) in [
                        (current, upper_rect, font_family),
                        (sub, lower_rect, sub_family),
                    ] {
                        should_animate |= draw_d2d_lyric_line(
                            state,
                            text,
                            rect,
                            font_size_scaled,
                            family,
                            scale,
                            marquee_start_ms,
                            text_color,
                            highlight_color,
                            outline_color,
                            &base_brush,
                            &highlight_brush,
                            smooth_lyric_progress,
                            smooth_lyric_progress,
                            line_duration_ms,
                        )?;
                    }
                }
            }

            state.dc_render_target.EndDraw(None, None).ok()?;
        }
        Some(should_animate)
    })
}

/// 悬浮卡片底板：简单的半透明黑色遮罩圆角卡片 + 细高光描边
///
/// # Safety
/// 必须在已初始化 COM 的渲染线程（D2D_STATE 上下文）调用，且此时
/// 渲染目标已 BindDC
unsafe fn draw_hover_card(
    state: &mut Direct2DState,
    client_rect: &RECT,
    corner_radius: f32,
) {
    let w = client_rect.right - client_rect.left;
    let h = client_rect.bottom - client_rect.top;
    if w <= 0 || h <= 0 {
        return;
    }
    let card = D2D1_ROUNDED_RECT {
        rect: rect_to_d2d(client_rect),
        radiusX: corner_radius,
        radiusY: corner_radius,
    };
    // SAFETY: card/tint/border 为栈上局部变量；画刷在本次绘制内使用
    unsafe {
        let tint = d2d_color(0x00_00_00, 0.5);
        if let Ok(tint_brush) =
            state.dc_render_target.CreateSolidColorBrush(&raw const tint, None)
        {
            state
                .dc_render_target
                .FillRoundedRectangle(&raw const card, &tint_brush);
        }
        let border = d2d_color(0xFF_FFFF, 0.08);
        if let Ok(border_brush) = state
            .dc_render_target
            .CreateSolidColorBrush(&raw const border, None)
        {
            state
                .dc_render_target
                .DrawRoundedRectangle(&raw const card, &border_brush, 1.0, None);
        }
    }
}

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

pub struct DesktopLyricsManager {
    initialized: bool,
}

// SAFETY: DesktopLyricsManager 仅含一个 bool 字段，本身无内部可变性。
// show/hide 等操作通过 PostMessageW 将请求转发到桌面歌词消息循环线程执行；
// set_locked 虽直接调用 GetWindowLongPtrW/SetWindowLongPtrW/InvalidateRect，
// 但这些 Win32 API 本身支持跨线程调用（仅读写窗口扩展样式，不涉及窗口过程指针），
// 因此可安全跨线程共享。
unsafe impl Send for DesktopLyricsManager {}
// SAFETY: 同上，所有方法要么读 bool，要么通过消息循环间接操作窗口，
// 要么调用线程安全的 Win32 窗口 API，无数据竞争
unsafe impl Sync for DesktopLyricsManager {}

impl Default for DesktopLyricsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopLyricsManager {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    pub fn initialize(&mut self, app_handle: tauri::AppHandle) -> Result<(), String> {
        if self.initialized {
            return Ok(());
        }

        let _ = APP_HANDLE.set(app_handle);

        let shared_state = SHARED_STATE.get_or_init(|| {
            Arc::new(Mutex::new(SharedLyricState {
                current_line: String::new(),
                sub_line: String::new(),
                prev_line: String::new(),
                prev_sub_line: String::new(),
                current_words: Vec::new(),
                font_size: 28,
                font_family: "Microsoft YaHei".to_string(),
                translation_font_family: String::new(),
                font_generation: 0,
                is_locked: true,
                is_hovered: false,
                is_playing: false,
                color_preset: PRESET_DARK,
                preset_auto: false,
                auto_light_text: true,
                last_bg_sample_ms: 0,
                fade_alpha: 1.0,
                fade_pending: false,
                marquee_active: false,
                marquee_start_ms: 0,
                visual_time: 0.0,
                target_time: 0.0,
                visual_time_last_ms: now_ms(),
                lyric_progress: 0.0,
                smooth_lyric_progress: 0.0,
                render_pending: false,
            }))
        });

        let state_for_thread = Arc::clone(shared_state);

        std::thread::Builder::new()
            .name("desktop-lyrics".to_string())
            .spawn(move || {
                run_message_loop(state_for_thread);
            })
            .map_err(|e| format!("Failed to spawn desktop lyrics thread: {e}"))?;

        for _ in 0..50 {
            if LYRICS_HWND.get().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        self.initialized = true;
        log::info!("Desktop lyrics window thread started");
        Ok(())
    }

    pub fn update_lyric(
        &self,
        current_line: &str,
        sub_line: &str,
        progress: f32,
        words: Vec<DesktopLyricWord>,
        current_time: f32,
        is_playing: bool,
    ) -> Result<(), String> {
        let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
        let progress = progress.clamp(0.0, 1.0);
        let current_time = current_time.max(0.0);
        {
            let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
            let text_changed = guard.current_line != current_line || guard.sub_line != sub_line;
            let words_changed = guard.current_words != words;
            if !text_changed && !words_changed && guard.is_playing == is_playing {
                let progress_delta = (guard.lyric_progress - progress).abs();
                let time_delta = (guard.target_time - current_time).abs();
                if progress_delta < 0.0008 && time_delta < 0.02 {
                    return Ok(());
                }
            }
            if text_changed {
                // 注意:不能用 clone_from,因为 guard.prev_line 和 guard.current_line 都借用了 guard
                // 会触发 mutable + immutable borrow 冲突
                let prev = guard.current_line.clone();
                let prev_sub = guard.sub_line.clone();
                guard.prev_line = prev;
                guard.prev_sub_line = prev_sub;
                guard.current_line = current_line.to_string();
                guard.sub_line = sub_line.to_string();
                guard.marquee_start_ms = now_ms();
                if !guard.prev_line.is_empty() || !guard.prev_sub_line.is_empty() {
                    guard.fade_alpha = 0.0;
                    guard.fade_pending = true;
                }
            }
            if words_changed {
                guard.current_words = words;
            }
            guard.is_playing = is_playing;
            guard.target_time = current_time;
            guard.visual_time = if (guard.visual_time - current_time).abs() > 0.5 {
                current_time
            } else {
                guard.visual_time
            };
            guard.visual_time_last_ms = now_ms();
            guard.lyric_progress = progress;
            guard.render_pending = true;
        }
        post_update();
        Ok(())
    }

    pub fn show(&self) -> Result<(), String> {
        let hwnd = get_hwnd();
        if hwnd != 0 {
            // SAFETY: hwnd 经 LYRICS_HWND 校验非 0，由消息循环线程创建并有效
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut _), SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    HWND(hwnd as *mut _),
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SET_WINDOW_POS_FLAGS(0x0001 | 0x0002),
                );
            }
        }
        Ok(())
    }

    pub fn hide(&self) -> Result<(), String> {
        let hwnd = get_hwnd();
        if hwnd != 0 {
            // SAFETY: 同 show()，hwnd 经校验有效
            unsafe {
                let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
            }
        }
        Ok(())
    }

    pub fn set_locked(&self, locked: bool) -> Result<(), String> {
        let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
        {
            let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
            guard.is_locked = locked;
            guard.render_pending = true;
        }

        let hwnd = get_hwnd();
        if hwnd != 0 {
            // SAFETY: hwnd 经校验有效；GetWindowLongPtrW/SetWindowLongPtrW
            // 仅读写 GWL_EXSTYLE（窗口扩展样式），不涉及窗口过程指针
            unsafe {
                let style = GetWindowLongPtrW(HWND(hwnd as *mut _), GWL_EXSTYLE);
                let transparent_bit = WS_EX_TRANSPARENT.0 as isize;
                let new_style = if locked {
                    style | transparent_bit
                } else {
                    style & !transparent_bit
                };
                SetWindowLongPtrW(HWND(hwnd as *mut _), GWL_EXSTYLE, new_style);
                let _ = InvalidateRect(Some(HWND(hwnd as *mut _)), None, true);
            }
        }
        Ok(())
    }

    pub fn set_font_size(&self, size: i32) -> Result<(), String> {
        let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
        {
            let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
            guard.font_size = size;
            guard.render_pending = true;
        }

        let hwnd = get_hwnd();
        if hwnd != 0 {
            resize_window_for_font(HWND(hwnd as *mut _), size);
        }

        post_update();
        Ok(())
    }

    /// 设置原文/译文歌词字体族。译文族传空字符串表示跟随原文。
    /// 同时提升字体代数，渲染线程会据此重建外部字体内存字体集
    /// （fonts/ 目录的文件可能在会话期间新增）
    pub fn set_font_family(
        &self,
        font_family: &str,
        translation_font_family: &str,
    ) -> Result<(), String> {
        let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
        {
            let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
            guard.font_family = font_family.to_string();
            guard.translation_font_family = translation_font_family.to_string();
            guard.font_generation = guard.font_generation.wrapping_add(1);
            guard.render_pending = true;
        }
        post_update();
        Ok(())
    }

    pub fn set_color_preset(&self, preset_name: &str) -> Result<(), String> {
        let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
        {
            let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
            // auto 模式保留当前 color_preset 作为首帧占位，渲染线程采样后动态决定
            guard.preset_auto = preset_name == "auto";
            guard.color_preset = match preset_name {
                "light" => PRESET_LIGHT,
                "blue" => PRESET_BLUE,
                "pink" => PRESET_PINK,
                "orange" => PRESET_ORANGE,
                "green" => PRESET_GREEN,
                "auto" => guard.color_preset,
                _ => PRESET_DARK,
            };
            guard.render_pending = true;
        }
        post_update();
        Ok(())
    }

    pub fn is_visible(&self) -> bool {
        let hwnd = get_hwnd();
        if hwnd != 0 {
            // SAFETY: hwnd 经校验有效，IsWindowVisible 仅查询窗口可见性，无副作用
            unsafe { IsWindowVisible(HWND(hwnd as *mut _)).as_bool() }
        } else {
            false
        }
    }
}

fn get_hwnd() -> isize {
    LYRICS_HWND.get().copied().unwrap_or(0)
}

fn post_update() {
    let hwnd = get_hwnd();
    if hwnd != 0 {
        // SAFETY: hwnd 经校验有效；PostMessageW 异步投递消息到目标线程消息队列，
        // WPARAM/LPARAM 均为 0，不传递所有权，无生命周期问题
        unsafe {
            let _ = PostMessageW(
                Some(HWND(hwnd as *mut _)),
                WM_DL_UPDATE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

fn get_close_btn_rect(window_rect: &RECT, scale: i32) -> RECT {
    let btn = CLOSE_BTN_SIZE * scale / 96;
    let margin = CLOSE_BTN_MARGIN * scale / 96;
    let spacing = 8 * scale / 96;
    let w = window_rect.right - window_rect.left;
    // Two buttons side by side, centered horizontally at top
    let total_w = btn * 2 + spacing;
    let start_x = (w - total_w) / 2;
    RECT {
        left: start_x + btn + spacing,
        top: margin,
        right: start_x + total_w,
        bottom: margin + btn,
    }
}

fn get_lock_btn_rect(window_rect: &RECT, scale: i32) -> RECT {
    let btn = CLOSE_BTN_SIZE * scale / 96;
    let margin = CLOSE_BTN_MARGIN * scale / 96;
    let spacing = 8 * scale / 96;
    let w = window_rect.right - window_rect.left;
    let total_w = btn * 2 + spacing;
    let start_x = (w - total_w) / 2;
    RECT {
        left: start_x,
        top: margin,
        right: start_x + btn,
        bottom: margin + btn,
    }
}

#[derive(Clone, Copy)]
struct LyricsLayout {
    close_rect: RECT,
    lock_rect: RECT,
    text_rect: RECT,
}

fn desired_window_height(scale: i32, font_size: i32) -> i32 {
    let btn = CLOSE_BTN_SIZE * scale / 96;
    let margin = CLOSE_BTN_MARGIN * scale / 96;
    let padding = 8 * scale / 96;
    let top_offset = btn + margin + padding;
    let bottom_padding = 14 * scale / 96;
    let line_spacing = 8 * scale / 96;
    let font_size_scaled = (font_size * scale / 96).max(18 * scale / 96);

    // 始终预留双行歌词高度，避免歌词翻译切换时窗口抖动，也避免大字体被裁剪。
    // 每行按 1.5em 预留：字体实际行高（ascent+descent）随字体族在
    // 1.2~1.6em 间浮动，只按字号预留会裁剪高行高字体
    let content_h = font_size_scaled * 3 + line_spacing;
    (top_offset + content_h + bottom_padding).max(150 * scale / 96)
}

fn resize_window_for_font(hwnd: HWND, font_size: i32) {
    // SAFETY: hwnd 由调用方保证有效；rect 是栈上局部变量，&raw mut 仅在
    // GetWindowRect 调用期间被写入，随后在本函数内读取，无别名冲突
    unsafe {
        let dpi = get_dpi_for_window(hwnd);
        let scale = dpi as i32;
        let new_h = desired_window_height(scale, font_size);

        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &raw mut rect);
        let current_w = rect.right - rect.left;
        let current_h = rect.bottom - rect.top;
        if current_w <= 0 || (current_h - new_h).abs() <= 1 {
            return;
        }

        // 保持底部位置不变，只根据字体大小调整高度
        let new_y = rect.bottom - new_h;
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            new_y,
            current_w,
            new_h,
            SET_WINDOW_POS_FLAGS(0),
        );
    }
}

fn build_lyrics_layout(client_rect: &RECT, scale: i32) -> LyricsLayout {
    let close_rect = get_close_btn_rect(client_rect, scale);
    let lock_rect = get_lock_btn_rect(client_rect, scale);
    let padding = 8 * scale / 96;
    let btn_bottom = close_rect.bottom;
    let mut text_rect = *client_rect;
    text_rect.left += padding;
    text_rect.right -= padding;
    text_rect.top = btn_bottom + padding;
    text_rect.bottom -= padding;

    // 双行的具体行高在渲染时按两行字体度量动态计算（见
    // measure_text_height_dwrite_with_state），这里只给出文本总区域
    LyricsLayout {
        close_rect,
        lock_rect,
        text_rect,
    }
}

fn apply_progress_effects(
    layout: &IDWriteTextLayout,
    text: &[u16],
    progress: f32,
    base_brush: &ID2D1SolidColorBrush,
    current_brush: &ID2D1SolidColorBrush,
    highlight_brush: &ID2D1SolidColorBrush,
) {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return;
    }
    let progress = progress.clamp(0.0, 1.0);
    let decoded = String::from_utf16_lossy(text);
    let graphemes: Vec<&str> = UnicodeSegmentation::graphemes(decoded.as_str(), true).collect();
    if graphemes.is_empty() {
        return;
    }

    let exact = (graphemes.len() as f32) * progress;
    let highlighted = exact.floor() as usize;
    let current_t = exact - highlighted as f32;
    let mut utf16_idx = 0usize;
    for (cluster_idx, grapheme) in graphemes.iter().enumerate() {
        let len = grapheme.encode_utf16().count();
        let range = DWRITE_TEXT_RANGE {
            startPosition: utf16_idx as u32,
            length: len as u32,
        };
        let brush = if cluster_idx < highlighted {
            highlight_brush
        } else if cluster_idx == highlighted && current_t > 0.0 {
            current_brush
        } else {
            base_brush
        };
        // SAFETY: layout 是有效 COM 对象；brush 引用有效画刷；range 在文本范围内
        let _ = unsafe { layout.SetDrawingEffect(brush, range) };
        utf16_idx += len;
    }
}

fn point_in_rect(x: i32, y: i32, rect: &RECT) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

/// 描边绘制的 8 方向偏移（像素，随 DPI 缩放）。
/// 返回相对文本原点的 (dx, dy) 偏移，用于把描边层铺在文字下方
fn outline_offsets(scale: i32) -> [(f32, f32); 8] {
    let r = ((scale as f32 / 96.0).round() as i32).max(1) as f32;
    [
        (-r, 0.0),
        (r, 0.0),
        (0.0, -r),
        (0.0, r),
        (-r, -r),
        (-r, r),
        (r, -r),
        (r, r),
    ]
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 捕获屏幕区域（不含分层窗口自身）到 out_w×out_h 的 BGRA 缓冲，
/// alpha 恒为 255。StretchBlt 不带 CAPTUREBLT 标志时不会捕获分层窗口，
/// 拿到的降采样图就是本窗口身后被遮挡的内容。
/// 返回 (像素数据, 宽, 高)；区域非法或 GDI 失败返回 None
///
/// # Safety
/// `window_rect` 为屏幕坐标。GDI 对象（DIB/兼容 DC）在函数内成对创建与释放，
/// 屏幕 DC 配对 ReleaseDC
unsafe fn capture_region_bgra(
    window_rect: &RECT,
    out_w: i32,
    out_h: i32,
) -> Option<(Vec<u8>, i32, i32)> {
    let region_w = window_rect.right - window_rect.left;
    let region_h = window_rect.bottom - window_rect.top;
    if region_w <= 0 || region_h <= 0 || out_w <= 0 || out_h <= 0 {
        return None;
    }
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: out_w,
            biHeight: -out_h, // 自上而下，行序与屏幕方向一致
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    };

    // SAFETY: GetDC(None) 返回整屏 DC，函数结束前配对 ReleaseDC
    let hdc_screen = unsafe { GetDC(None) };
    if hdc_screen.is_invalid() {
        return None;
    }
    // SAFETY: 由有效屏幕 DC 派生兼容 DC
    let hdc_mem = unsafe { CreateCompatibleDC(Some(hdc_screen)) };
    let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: bmi 为栈上局部变量；p_bits 指向 DIB 数据，DeleteObject 前有效
    let Ok(h_bitmap) = (unsafe {
        CreateDIBSection(None, &raw const bmi, DIB_RGB_COLORS, &raw mut p_bits, None, 0)
    }) else {
        // SAFETY: 配对释放本函数创建的 GDI 资源
        unsafe {
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);
        }
        return None;
    };
    if p_bits.is_null() {
        // SAFETY: 同上
        unsafe {
            let _ = DeleteObject(HGDIOBJ(h_bitmap.0));
            let _ = DeleteDC(hdc_mem);
            let _ = ReleaseDC(None, hdc_screen);
        }
        return None;
    }
    // SAFETY: 选入 DIB 后原对象保存在 old，退出前选回
    let old_bitmap = unsafe { SelectObject(hdc_mem, HGDIOBJ(h_bitmap.0)) };
    // SAFETY: 目标 DC 已选入 DIB；源为屏幕 DC；SRCCOPY 不含 CAPTUREBLT
    let blitted = unsafe {
        StretchBlt(
            hdc_mem,
            0,
            0,
            out_w,
            out_h,
            Some(hdc_screen),
            window_rect.left,
            window_rect.top,
            region_w,
            region_h,
            SRCCOPY,
        )
    };

    let captured = if blitted.as_bool() {
        // SAFETY: p_bits 指向 out_h * (out_w * 4) 字节的 DIB 数据（32bpp 行天然 4 对齐）
        let len = out_w as usize * out_h as usize * 4;
        let mut data = vec![0u8; len];
        unsafe {
            std::ptr::copy_nonoverlapping(p_bits.cast::<u8>(), data.as_mut_ptr(), len);
        }
        // GDI 位图的 alpha 通道不可靠（常为 0），强制不透明
        for px in data.chunks_exact_mut(4) {
            px[3] = 255;
        }
        Some((data, out_w, out_h))
    } else {
        None
    };

    // SAFETY: 恢复 DC 原选入对象并释放本函数创建的全部 GDI 资源
    unsafe {
        let _ = SelectObject(hdc_mem, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(h_bitmap.0));
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);
    }
    captured
}

/// 采样窗口背后的屏幕区域平均亮度（0=全黑 1=全白），供 auto 配色
/// 决定使用深色还是浅色文字
///
/// # Safety
/// `window_rect` 为屏幕坐标
unsafe fn sample_background_luma(window_rect: &RECT) -> Option<f32> {
    const SAMPLE_W: i32 = 48;
    let region_w = window_rect.right - window_rect.left;
    let region_h = window_rect.bottom - window_rect.top;
    if region_w <= 0 || region_h <= 0 {
        return None;
    }
    let sample_h = ((region_h as f32 / region_w as f32) * SAMPLE_W as f32)
        .max(1.0)
        .round() as i32;
    // SAFETY: GDI 资源在函数内配对管理
    let (data, ..) = unsafe { capture_region_bgra(window_rect, SAMPLE_W, sample_h)? };

    let mut sum = 0f64;
    let mut count = 0usize;
    for px in data.chunks_exact(4) {
        sum += 0.299 * px[2] as f64 + 0.587 * px[1] as f64 + 0.114 * px[0] as f64;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    Some((sum / count as f64 / 255.0) as f32)
}

fn measure_text_width_dwrite_with_state(
    state: &mut Direct2DState,
    text: &[u16],
    height: i32,
    font_size_scaled: i32,
    family: &str,
) -> Option<i32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return Some(0);
    }
    // SAFETY: create_layout 要求 COM 已初始化，调用方在 D2D_STATE 线程上下文
    let layout = unsafe {
        state
            .create_layout(text, font_size_scaled, 100_000.0, height.max(1) as f32, family)
            .ok()?
    };
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: layout 是有效 COM 对象；metrics 是栈上局部变量，&raw mut 仅在
    // GetMetrics 调用期间被写入，随后读取，无别名冲突
    unsafe { layout.GetMetrics(&raw mut metrics).ok()? };
    Some(metrics.widthIncludingTrailingWhitespace.ceil().max(0.0) as i32)
}

/// 测量单行文本的字体度量高度（随字体族的实际 ascent/descent 变化），
/// 供双行歌词按各自实际高度紧凑排版
fn measure_text_height_dwrite_with_state(
    state: &mut Direct2DState,
    text: &[u16],
    font_size_scaled: i32,
    family: &str,
) -> Option<i32> {
    let text = trim_utf16_nul(text);
    if text.is_empty() {
        return Some(0);
    }
    // SAFETY: create_layout 要求 COM 已初始化，调用方在 D2D_STATE 线程上下文；
    // NO_WRAP 下单行高度与给定矩形高度无关
    let layout = unsafe {
        state
            .create_layout(
                text,
                font_size_scaled,
                100_000.0,
                font_size_scaled.max(1) as f32,
                family,
            )
            .ok()?
    };
    let mut metrics = DWRITE_TEXT_METRICS::default();
    // SAFETY: 同上，metrics 为栈上局部变量
    unsafe { layout.GetMetrics(&raw mut metrics).ok()? };
    Some(metrics.height.ceil().max(0.0) as i32)
}

fn get_cached_font(font_size_scaled: i32) -> HFONT {
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

/// # Safety
/// 完整的桌面歌词渲染函数。调用者必须保证：
/// - `hwnd` 是由本模块消息循环线程创建的有效窗口句柄
/// - 当前线程已初始化 COM（`CoInitializeEx`）
/// - 仅在 desktop-lyrics 消息循环线程上调用（D2D/DWrite 单线程访问）
unsafe fn render_lyrics(hwnd: HWND) {
    // SAFETY: 见函数级 Safety 文档；内部所有 &raw mut/const 均指向栈上局部变量，
    // 在对应 Win32/COM 调用返回后才被读取，无别名冲突。GDI 对象（h_bitmap/hdc_mem）
    // 在使用后通过 DeleteObject/DeleteDC 释放，遵循 GDI 对象生命周期管理契约。
    unsafe {
        let mut window_rect = RECT::default();
        let _ = GetWindowRect(hwnd, &raw mut window_rect);
        let mut client_rect = RECT::default();
        let _ = GetClientRect(hwnd, &raw mut client_rect);
        let w = client_rect.right - client_rect.left;
        let h = client_rect.bottom - client_rect.top;
        if w <= 0 || h <= 0 {
            return;
        }

        let dpi = get_dpi_for_window(hwnd);
        let scale = dpi as i32;

        let (
            is_hovered,
            is_locked,
            current_line,
            sub_line,
            current_words,
            is_playing,
            font_size,
            font_family,
            translation_font_family,
            preset,
            preset_auto,
            mut fade_alpha,
            fade_pending,
            render_pending,
            marquee_start_ms,
            lyric_progress,
            mut smooth_lyric_progress,
            mut visual_time,
            target_time,
            visual_time_last_ms,
        ) = {
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(guard) = state.lock() {
                    (
                        guard.is_hovered,
                        guard.is_locked,
                        guard.current_line.clone(),
                        guard.sub_line.clone(),
                        guard.current_words.clone(),
                        guard.is_playing,
                        guard.font_size,
                        guard.font_family.clone(),
                        guard.translation_font_family.clone(),
                        guard.color_preset,
                        guard.preset_auto,
                        guard.fade_alpha,
                        guard.fade_pending,
                        guard.render_pending,
                        guard.marquee_start_ms,
                        guard.lyric_progress,
                        guard.smooth_lyric_progress,
                        guard.visual_time,
                        guard.target_time,
                        guard.visual_time_last_ms,
                    )
                } else {
                    return;
                }
            } else {
                return;
            }
        };

        let now = now_ms();

        // auto 配色：限时采样窗口背后背景亮度，深/浅文字滞回切换
        let preset = if preset_auto {
            let mut light_text = true;
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(mut guard) = state.try_lock() {
                    light_text = guard.auto_light_text;
                    if now - guard.last_bg_sample_ms > 600 {
                        // 采样不涉及自身分层窗口（无 CAPTUREBLT）
                        if let Some(luma) = sample_background_luma(&window_rect) {
                            // 滞回区间避免临界亮度下来回跳变
                            if luma > 0.62 {
                                light_text = false;
                            } else if luma < 0.38 {
                                light_text = true;
                            }
                            guard.auto_light_text = light_text;
                        }
                        guard.last_bg_sample_ms = now;
                    }
                }
            }
            if light_text {
                PRESET_LIGHT
            } else {
                PRESET_DARK
            }
        } else {
            preset
        };

        let delta = ((now - visual_time_last_ms).max(0) as f32 / 1000.0).min(0.1);
        if is_playing {
            let diff = visual_time - target_time;
            if diff.abs() > 0.25 {
                visual_time = target_time;
            } else if diff.abs() > 0.15 {
                let speed = (1.0 - diff * 2.0).clamp(0.5, 1.5);
                visual_time += delta * speed;
            } else {
                visual_time += delta;
            }
        } else {
            visual_time = target_time;
        }
        if let Some(state) = SHARED_STATE.get() {
            if let Ok(mut guard) = state.try_lock() {
                guard.visual_time = visual_time;
                guard.visual_time_last_ms = now;
            }
        }

        let display_progress =
            karaoke_progress_from_words(&current_words, visual_time, lyric_progress);
        if current_words.is_empty() {
            let progress_delta = (display_progress - smooth_lyric_progress).abs();
            if progress_delta > 0.0001 {
                smooth_lyric_progress += (display_progress - smooth_lyric_progress) * 0.42;
            } else {
                smooth_lyric_progress = display_progress;
            }
        } else {
            smooth_lyric_progress = display_progress;
        }
        if let Some(state) = SHARED_STATE.get() {
            if let Ok(mut guard) = state.try_lock() {
                guard.smooth_lyric_progress = smooth_lyric_progress;
            }
        }

        if fade_pending {
            fade_alpha = (fade_alpha + 0.06).min(1.0);
            let done = fade_alpha >= 1.0;
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(mut guard) = state.lock() {
                    guard.fade_alpha = fade_alpha;
                    if done {
                        guard.fade_pending = false;
                        guard.prev_line.clear();
                        guard.prev_sub_line.clear();
                    }
                }
            }
            if !done {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }

        if !render_pending
            && !fade_pending
            && !is_hovered
            && !current_line.is_empty()
            && !sub_line.is_empty()
        {
            // 仍允许后续歌词/翻译变化触发渲染，但避免空闲时重复走整帧合成
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };

        let mut p_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let h_bitmap = match CreateDIBSection(
            None,
            &raw const bmi,
            DIB_RGB_COLORS,
            &raw mut p_bits,
            None,
            0,
        ) {
            Ok(h) => h,
            Err(e) => {
                log::error!("Desktop lyrics: CreateDIBSection failed: {e}");
                return;
            }
        };

        if p_bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(h_bitmap.0));
            return;
        }

        let hdc_mem = CreateCompatibleDC(None);
        let old_bitmap = SelectObject(hdc_mem, HGDIOBJ(h_bitmap.0));

        SetBkMode(hdc_mem, TRANSPARENT);

        let font_size_scaled = font_size * scale / 96;
        // 调用 get_cached_font 是为了预热字体缓存(有副作用),返回值未使用
        let _ = get_cached_font(font_size_scaled);

        let current_words_animating = is_playing && !current_words.is_empty();
        let display_progress =
            karaoke_progress_from_words(&current_words, visual_time, lyric_progress);
        let line_duration_ms = current_words
            .iter()
            .filter(|w| !w.text.is_empty() && w.end > w.start)
            .map(|w| w.end)
            .reduce(f32::max)
            .map(|end| {
                let start = current_words
                    .iter()
                    .filter(|w| !w.text.is_empty() && w.end > w.start)
                    .map(|w| w.start)
                    .reduce(f32::min)
                    .unwrap_or(end);
                ((end - start).max(0.0) * 1000.0) as i64
            });
        let d2d_should_animate_scroll = render_lyrics_d2d_frame(
            hwnd,
            hdc_mem,
            &client_rect,
            scale,
            is_hovered,
            is_locked,
            &current_line,
            &sub_line,
            font_size,
            &font_family,
            &translation_font_family,
            preset,
            fade_alpha,
            marquee_start_ms,
            display_progress,
            smooth_lyric_progress,
            line_duration_ms,
        )
        .unwrap_or(false);
        let should_animate_scroll = current_words_animating || d2d_should_animate_scroll;

        let pt_dst = POINT {
            x: window_rect.left,
            y: window_rect.top,
        };
        let pt_src = POINT { x: 0, y: 0 };
        let sz = SIZE { cx: w, cy: h };
        let blend = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&raw const pt_dst),
            Some(&raw const sz),
            Some(hdc_mem),
            Some(&raw const pt_src),
            COLORREF(0),
            Some(&raw const blend),
            ULW_ALPHA,
        );

        if let Some(state) = SHARED_STATE.get() {
            if let Ok(mut guard) = state.try_lock() {
                guard.marquee_active = should_animate_scroll;
                guard.render_pending = false;
            }
        }

        let _ = SelectObject(hdc_mem, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(h_bitmap.0));
        let _ = DeleteDC(hdc_mem);
    }
}

fn run_message_loop(_state: Arc<Mutex<SharedLyricState>>) {
    // SAFETY: 本函数在专属的 desktop-lyrics 线程上运行，所有 Win32 调用均在该线程：
    // - CoInitializeEx 在线程入口调用，CoUninitialize 在退出时配对
    // - RegisterClassW/CreateWindowExW 注册的窗口过程由本线程消息泵分发
    // - &raw const/mut 均指向栈上局部变量，在 Win32 调用返回后才读取，无别名
    // - SetTimer 回调由本线程消息泵处理（回调指针为 None，走 WM_TIMER）
    unsafe {
        let _ = set_thread_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();

        let instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                log::error!("Desktop lyrics: Failed to get module handle: {e}");
                if com_initialized {
                    CoUninitialize();
                }
                return;
            }
        };

        let class_name = w!("DesktopLyricsWnd");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name,
        };

        if RegisterClassW(&raw const wc) == 0 {
            log::error!("Desktop lyrics: Failed to register window class");
            return;
        }

        let screen_w = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
        );

        let mut work_area = RECT {
            left: 0,
            top: 0,
            right: screen_w,
            bottom: 0,
        };

        let _ = windows::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
            0,
            Some((&raw mut work_area).cast()),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );

        let window_w = (screen_w as f32 * 0.55).clamp(600.0, 1200.0) as i32;
        let initial_font_size = SHARED_STATE
            .get()
            .and_then(|state| state.try_lock().ok().map(|guard| guard.font_size))
            .unwrap_or(28);
        let dpi_scale = 96;
        let window_h = desired_window_height(dpi_scale, initial_font_size);
        let x = ((work_area.right - work_area.left) - window_w) / 2 + work_area.left;
        let y = work_area.bottom - window_h - 10;

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            class_name,
            w!("Desktop Lyrics"),
            WS_POPUP,
            x,
            y,
            window_w,
            window_h,
            None,
            None,
            Some(instance.into()),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                log::error!("Desktop lyrics: Failed to create window: {e}");
                return;
            }
        };

        let _ = LYRICS_HWND.set(hwnd.0 as isize);

        // 启动定时器：16ms 检测一次，但只有滚动/淡入/悬浮变化时才重绘
        let _ = SetTimer(Some(hwnd), HOVER_TIMER_ID, 16, None);

        let mut msg = MSG::default();
        while GetMessageW(&raw mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&raw const msg);
            DispatchMessageW(&raw const msg);
        }

        if com_initialized {
            CoUninitialize();
        }
        log::info!("Desktop lyrics message loop exited");
    }
}

/// # Safety
/// Win32 窗口过程回调。由系统在 desktop-lyrics 消息循环线程上调用，
/// `hwnd` 是本模块创建的窗口句柄。各消息处理分支内的 unsafe 块均仅调用
/// 线程安全的 Win32 API 或操作栈上局部变量。
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => unsafe {
            // SAFETY: ps 是栈上局部变量，BeginPaint/EndPaint 配对调用
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &raw mut ps);
            render_lyrics(hwnd);
            let _ = EndPaint(hwnd, &raw const ps);
            LRESULT(0)
        },
        WM_DL_UPDATE => unsafe {
            // SAFETY: hwnd 有效，InvalidateRect 仅标记重绘区域
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        },
        WM_SIZE => unsafe {
            // SAFETY: 同上
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        },
        WM_TIMER => unsafe {
            // SAFETY: pt/rect/client_rect 均为栈上局部变量，&raw mut 仅在对应
            // Win32 调用期间被写入；GetWindowLongPtrW/SetWindowLongPtrW 仅操作
            // GWL_EXSTYLE，不涉及窗口过程指针
            if wparam.0 == HOVER_TIMER_ID {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&raw mut pt);
                let mut rect = RECT::default();
                let _ = GetWindowRect(hwnd, &raw mut rect);

                let is_inside = pt.x >= rect.left
                    && pt.x < rect.right
                    && pt.y >= rect.top
                    && pt.y < rect.bottom;

                if let Some(state) = SHARED_STATE.get() {
                    if let Ok(mut guard) = state.try_lock() {
                        let progress_animating =
                            (guard.lyric_progress - guard.smooth_lyric_progress).abs() > 0.0001;
                        let word_animating = guard.is_playing && !guard.current_words.is_empty();
                        let should_redraw = guard.is_hovered != is_inside
                            || guard.marquee_active
                            || guard.fade_pending
                            || progress_animating
                            || word_animating;
                        if guard.is_hovered != is_inside {
                            guard.is_hovered = is_inside;
                        }
                        if should_redraw {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }

                        // 锁定状态下：光标悬停在解锁按钮上时临时移除穿透样式使其可点击，
                        // 移开后恢复穿透。WS_EX_TRANSPARENT 对整个窗口生效，无法只让按钮
                        // 区域接收点击，因此通过定时器轮询光标位置按区域动态切换。
                        if guard.is_locked {
                            let dpi = get_dpi_for_window(hwnd);
                            let mut client_rect = RECT::default();
                            let _ = GetClientRect(hwnd, &raw mut client_rect);
                            let lock_rect = get_lock_btn_rect(&client_rect, dpi as i32);
                            // WS_POPUP 窗口客户区原点即窗口原点，换算为屏幕坐标
                            let on_lock_btn = pt.x >= rect.left + lock_rect.left
                                && pt.x < rect.left + lock_rect.right
                                && pt.y >= rect.top + lock_rect.top
                                && pt.y < rect.top + lock_rect.bottom;

                            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                            let transparent_bit = WS_EX_TRANSPARENT.0 as isize;
                            let has_transparent = style & transparent_bit != 0;
                            if on_lock_btn && has_transparent {
                                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !transparent_bit);
                            } else if !on_lock_btn && !has_transparent {
                                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | transparent_bit);
                            }
                        }
                    }
                }
            }
            LRESULT(0)
        },
        WM_DPICHANGED if lparam.0 != 0 => unsafe {
            // SAFETY: lparam 非空且指向系统提供的 RECT，在窗口过程调用期间有效
            let rect = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SET_WINDOW_POS_FLAGS(0),
            );
            LRESULT(0)
        },
        WM_MOUSEMOVE => unsafe {
            // 非锁定模式下 WM_MOUSEMOVE 能正常工作
            // SAFETY: hwnd 有效，InvalidateRect 仅标记重绘区域
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(mut guard) = state.try_lock() {
                    if !guard.is_hovered {
                        guard.is_hovered = true;
                        let _ = InvalidateRect(Some(hwnd), None, true);
                    }
                }
            }

            DefWindowProcW(hwnd, msg, wparam, lparam)
        },
        WM_LBUTTONUP => unsafe {
            // SAFETY: rect 是栈上局部变量，&raw mut 仅在 GetClientRect 期间写入；
            // GetWindowLongPtrW/SetWindowLongPtrW 仅操作 GWL_EXSTYLE
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &raw mut rect);
            let dpi = get_dpi_for_window(hwnd);
            let close_rect = get_close_btn_rect(&rect, dpi as i32);
            let lock_rect = get_lock_btn_rect(&rect, dpi as i32);

            let is_locked = SHARED_STATE
                .get()
                .and_then(|state| state.try_lock().ok().map(|guard| guard.is_locked))
                .unwrap_or(false);

            if !is_locked && point_in_rect(x, y, &close_rect) {
                let _ = ShowWindow(hwnd, SW_HIDE);
                if let Some(state) = SHARED_STATE.get() {
                    if let Ok(mut guard) = state.try_lock() {
                        guard.is_hovered = false;
                    }
                }
                if let Some(app) = APP_HANDLE.get() {
                    let _ = app.emit("desktop-lyrics-closed", ());
                }
            } else if point_in_rect(x, y, &lock_rect) {
                if let Some(state) = SHARED_STATE.get() {
                    if let Ok(mut guard) = state.try_lock() {
                        let new_locked = !guard.is_locked;
                        guard.is_locked = new_locked;

                        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                        let transparent_bit = WS_EX_TRANSPARENT.0 as isize;
                        let new_style = if new_locked {
                            style | transparent_bit
                        } else {
                            style & !transparent_bit
                        };
                        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);
                        let _ = InvalidateRect(Some(hwnd), None, true);

                        if let Some(app) = APP_HANDLE.get() {
                            let _ = app.emit("desktop-lyrics-lock-changed", new_locked);
                        }
                    }
                }
            }
            LRESULT(0)
        },
        WM_MOUSELEAVE => unsafe {
            // SAFETY: hwnd 有效，InvalidateRect 仅标记重绘区域
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(mut guard) = state.try_lock() {
                    guard.is_hovered = false;
                }
            }
            let _ = InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        },
        WM_NCHITTEST => unsafe {
            // SAFETY: rect/screen_pt 均为栈上局部变量，&raw mut 仅在对应 Win32
            // 调用期间被写入；ScreenToClient 将屏幕坐标转为客户区坐标
            if let Some(state) = SHARED_STATE.get() {
                if let Ok(guard) = state.try_lock() {
                    let x = (lparam.0 & 0xFFFF) as i16 as i32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &raw mut rect);
                    let dpi = get_dpi_for_window(hwnd);

                    let mut screen_pt = POINT { x, y };
                    let _ = ScreenToClient(hwnd, &raw mut screen_pt);

                    let close_rect = get_close_btn_rect(&rect, dpi as i32);
                    let lock_rect = get_lock_btn_rect(&rect, dpi as i32);

                    if guard.is_locked {
                        if point_in_rect(screen_pt.x, screen_pt.y, &lock_rect) {
                            return LRESULT(HTCLIENT as isize);
                        }
                        return LRESULT(HTTRANSPARENT as isize);
                    }

                    if point_in_rect(screen_pt.x, screen_pt.y, &close_rect)
                        || point_in_rect(screen_pt.x, screen_pt.y, &lock_rect)
                    {
                        return LRESULT(HTCLIENT as isize);
                    }

                    let edge = (RESIZE_EDGE_WIDTH * dpi as i32 / 96).max(4);
                    if screen_pt.x < edge {
                        return LRESULT(HTLEFT as isize);
                    }
                    if screen_pt.x >= rect.right - edge {
                        return LRESULT(HTRIGHT as isize);
                    }

                    return LRESULT(HTCAPTION as isize);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        },
        WM_CLOSE => unsafe {
            // SAFETY: hwnd 有效，ShowWindow 仅修改窗口可见性
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        },
        WM_DESTROY => unsafe {
            // SAFETY: hwnd 有效；KillTimer 注销本窗口创建的定时器；
            // PostQuitMessage 向本线程消息队列投递 WM_QUIT 以退出消息循环
            let _ = KillTimer(Some(hwnd), HOVER_TIMER_ID);
            PostQuitMessage(0);
            LRESULT(0)
        },
        _ => unsafe {
            // SAFETY: 转发未处理的消息给默认窗口过程
            DefWindowProcW(hwnd, msg, wparam, lparam)
        },
    }
}

static DESKTOP_LYRICS_MANAGER: OnceLock<Arc<Mutex<DesktopLyricsManager>>> = OnceLock::new();

/// 使桌面歌词的字体内存缓存全部失效：渲染线程在下一帧重建外部字体
/// 名字索引、丢弃已按需加载的字体集合与文本格式缓存。
/// fonts/ 目录文件变化或用户主动清理字体缓存后调用
pub fn invalidate_font_caches() -> Result<(), String> {
    let state = SHARED_STATE.get().ok_or("Desktop lyrics not initialized")?;
    {
        let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
        guard.font_generation = guard.font_generation.wrapping_add(1);
        guard.render_pending = true;
    }
    post_update();
    Ok(())
}

pub fn get_desktop_lyrics_manager() -> Arc<Mutex<DesktopLyricsManager>> {
    Arc::clone(
        DESKTOP_LYRICS_MANAGER.get_or_init(|| Arc::new(Mutex::new(DesktopLyricsManager::new()))),
    )
}

#[command]
pub fn show_desktop_lyrics(app: tauri::AppHandle) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.initialize(app)?;
    guard.show()
}

#[command]
pub fn hide_desktop_lyrics() -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.hide()
}

#[command]
pub fn update_desktop_lyric(
    current_line: String,
    sub_line: String,
    progress: f32,
    words: Vec<DesktopLyricWord>,
    current_time: f32,
    is_playing: bool,
) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.update_lyric(
        &current_line,
        &sub_line,
        progress,
        words,
        current_time,
        is_playing,
    )
}

#[command]
pub fn set_desktop_lyrics_locked(locked: bool) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_locked(locked)
}

#[command]
pub fn set_desktop_lyrics_font_size(size: i32) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_font_size(size)
}

#[command]
pub fn set_desktop_lyrics_font_family(
    font_family: String,
    translation_font_family: String,
) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_font_family(&font_family, &translation_font_family)
}

#[command]
pub fn set_desktop_lyrics_color_preset(preset: String) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_color_preset(&preset)
}

#[command]
pub fn is_desktop_lyrics_visible() -> Result<bool, String> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    Ok(guard.is_visible())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用 dev 运行目录（target/debug/fonts）中的真实字体文件验证
    /// DirectWrite 内存字体集加载链路：
    /// Factory5 → InMemoryFontFileLoader → FontSetBuilder → 集合 → FindFamilyName。
    /// 目录不存在或没有可用字体时跳过
    #[test]
    fn dwrite_external_font_load_pipeline() {
        let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("fonts");
        let entries: Vec<std::path::PathBuf> = match std::fs::read_dir(&fonts_dir) {
            Ok(it) => it
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .map(|x| {
                            let ext = x.to_string_lossy().to_lowercase();
                            matches!(ext.as_str(), "ttf" | "otf")
                        })
                        .unwrap_or(false)
                })
                .collect(),
            Err(_) => return,
        };
        if entries.is_empty() {
            return;
        }

        // SAFETY: 测试线程入口即初始化 COM，退出前配对 CoUninitialize
        let mut failures: Vec<(String, String)> = Vec::new();
        unsafe {
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let dwrite: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DWriteCreateFactory 失败");
            // 走生产构建函数（含 RegisterFontFileLoader）
            let loader = build_memory_loader(&dwrite).expect("build_memory_loader 失败");

            for path in &entries {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let family =
                    crate::system::fonts::frontend_family_from_file_name(&file_name);
                // 走生产加载函数；外层 unsafe 块保证 COM 已初始化、loader 已注册
                let report = match try_load_external_font(
                    &dwrite,
                    &loader,
                    &path.to_string_lossy(),
                    &family,
                ) {
                    None => "try_load_external_font 失败".to_string(),
                    Some(font) => {
                        let wide: Vec<u16> = font
                            .internal_family
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        let mut index = 0u32;
                        let mut exists = windows::core::BOOL::default();
                        font.collection
                            .FindFamilyName(
                                PCWSTR::from_raw(wide.as_ptr()),
                                &raw mut index,
                                &raw mut exists,
                            )
                            .expect("FindFamilyName 失败");
                        if exists.as_bool() {
                            let format = dwrite.CreateTextFormat(
                                PCWSTR::from_raw(wide.as_ptr()),
                                Some(&font.collection),
                                DWRITE_FONT_WEIGHT_NORMAL,
                                DWRITE_FONT_STYLE_NORMAL,
                                DWRITE_FONT_STRETCH_NORMAL,
                                28.0,
                                w!("zh-cn"),
                            );
                            match format {
                                Ok(_) => "OK".to_string(),
                                Err(e) => format!("CreateTextFormat: {e}"),
                            }
                        } else {
                            format!("FindFamilyName 未命中内部族名 {}", font.internal_family)
                        }
                    }
                };
                println!("  {file_name}: {report}");
                failures.push((file_name, report));
            }
            let failures: Vec<(String, String)> = failures
                .into_iter()
                .filter(|(_, report)| report != "OK")
                .collect();
            assert!(failures.is_empty(), "失败: {failures:?}");

            if com_initialized {
                CoUninitialize();
            }
        }
    }
}
