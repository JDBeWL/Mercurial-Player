//! 整帧渲染。
//!
//! [`render_lyrics_d2d_frame`] 在 D2D_STATE 渲染线程上完成一帧的全部
//! Direct2D 绘制：悬浮卡片与关闭/锁定按钮、单行或双行歌词（含译文
//! 行的独立字体与垂直居中排版），并汇总是否需要继续滚动动画。
//! 颜色/几何换算辅助函数也放在此处供本模块内复用。

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, IDWriteFontCollection,
};
use windows::core::w;

use super::super::ColorPreset;
use super::d2d_resources::{D2D_STATE, Direct2DState, trim_utf16_nul};
use super::draw_line::draw_d2d_lyric_line;
use super::layout::{build_lyrics_layout, measure_text_height_dwrite_with_state};

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

pub(super) fn render_lyrics_d2d_frame(
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
                    let lower_h = measure_text_height_dwrite_with_state(
                        state,
                        sub,
                        font_size_scaled,
                        sub_family,
                    )
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
unsafe fn draw_hover_card(state: &mut Direct2DState, client_rect: &RECT, corner_radius: f32) {
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
        if let Ok(tint_brush) = state
            .dc_render_target
            .CreateSolidColorBrush(&raw const tint, None)
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
