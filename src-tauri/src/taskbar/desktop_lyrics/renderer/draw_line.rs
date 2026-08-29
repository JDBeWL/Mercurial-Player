//! 单行歌词绘制。
//!
//! [`draw_d2d_lyric_line`] 负责把一行歌词（含可选译文行）绘制到
//! Direct2D 渲染目标：计算跑马灯滚动偏移、单字素时的颜色插值过渡、
//! 多字素时的描边 + 基础层 + 卡拉OK叠加层与按进度剪裁的绘制。
//! 仅做绘制编排，颜色/画刷由调用方（整帧渲染）传入。

use unicode_segmentation::UnicodeSegmentation;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DRAW_TEXT_OPTIONS_NONE, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextLayout;
use windows_numerics::Vector2;

use super::super::now_ms;
use super::d2d_resources::{Direct2DState, trim_utf16_nul};
use super::karaoke::{progress_clip_end_x, progress_cluster_phase};
use super::layout::measure_text_width_dwrite_with_state;

const MARQUEE_SPEED_PX_PER_SEC: i32 = 42;

fn lerp_d2d_color(from: D2D1_COLOR_F, to: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    let t = t.clamp(0.0, 1.0);
    D2D1_COLOR_F {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
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
        let range = windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_RANGE {
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

pub(super) fn draw_d2d_lyric_line(
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
