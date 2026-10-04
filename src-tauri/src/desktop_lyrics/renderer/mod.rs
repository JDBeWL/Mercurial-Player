#![allow(unsafe_code)]

//! 桌面歌词渲染入口（模块组织）。
//!
//! 本文件只保留 [`render_lyrics`] 主流程（读共享状态、auto 配色、平滑进度、淡入、
//! GDI 内存位图合成与 UpdateLayeredWindow 提交）；各子模块的职责见其首部注释。

mod background;
mod d2d_resources;
mod draw_line;
mod frame;
mod karaoke;
mod layout;

pub(super) use layout::{CLOSE_BTN_MARGIN, CLOSE_BTN_SIZE, get_close_btn_rect, get_lock_btn_rect};

use std::mem::size_of;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, HGDIOBJ, InvalidateRect, RGBQUAD, SelectObject,
    SetBkMode, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, ULW_ALPHA, UpdateLayeredWindow,
};

use super::fonts::get_cached_font;
use super::window::get_dpi_for_window;
use super::{PRESET_DARK, PRESET_LIGHT, SHARED_STATE, now_ms};
use background::sample_background_luma;
use frame::render_lyrics_d2d_frame;
use karaoke::karaoke_progress_from_words;

/// 完整的桌面歌词渲染函数。
///
/// # Safety
/// `hwnd` 须为本模块消息循环线程创建的窗口，当前线程已 `CoInitializeEx`；
/// D2D/DWrite 资源线程独占，只能在 desktop-lyrics 消息循环线程调用。
pub(super) unsafe fn render_lyrics(hwnd: HWND) {
    // SAFETY: 满足函数级 Safety 文档的线程与句柄约束
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
