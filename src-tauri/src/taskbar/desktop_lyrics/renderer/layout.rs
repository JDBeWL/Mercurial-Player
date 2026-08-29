//! 布局计算与文本测量。
//!
//! 负责按钮矩形（关闭/锁定）、歌词文本总区域的布局（[`build_lyrics_layout`]），
//! 以及基于 DirectWrite 的文本宽度/高度测量（供跑马灯滚动与双行紧凑排版）。

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS;

use super::d2d_resources::{Direct2DState, trim_utf16_nul};

// 以下按钮常量/矩形函数经 renderer 模块 re-export 供 window.rs 使用，
// 可见性需覆盖 desktop_lyrics 模块
pub(in crate::taskbar::desktop_lyrics) const CLOSE_BTN_SIZE: i32 = 28;
pub(in crate::taskbar::desktop_lyrics) const CLOSE_BTN_MARGIN: i32 = 6;

pub(in crate::taskbar::desktop_lyrics) fn get_close_btn_rect(
    window_rect: &RECT,
    scale: i32,
) -> RECT {
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

pub(in crate::taskbar::desktop_lyrics) fn get_lock_btn_rect(
    window_rect: &RECT,
    scale: i32,
) -> RECT {
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
pub(super) struct LyricsLayout {
    pub(super) close_rect: RECT,
    pub(super) lock_rect: RECT,
    pub(super) text_rect: RECT,
}

pub(super) fn build_lyrics_layout(client_rect: &RECT, scale: i32) -> LyricsLayout {
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

pub(super) fn measure_text_width_dwrite_with_state(
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
            .create_layout(
                text,
                font_size_scaled,
                100_000.0,
                height.max(1) as f32,
                family,
            )
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
pub(super) fn measure_text_height_dwrite_with_state(
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
