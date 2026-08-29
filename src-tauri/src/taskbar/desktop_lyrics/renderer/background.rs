//! 背景亮度采样（auto 配色）。
//!
//! 通过 GDI 抓取窗口身后的屏幕区域（不带 CAPTUREBLT，不会捕获分层
//! 窗口自身）并计算平均亮度，供 auto 配色在深色/浅色文字间切换。

use std::mem::size_of;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, HGDIOBJ, RGBQUAD, ReleaseDC, SRCCOPY, SelectObject, StretchBlt,
};

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
        CreateDIBSection(
            None,
            &raw const bmi,
            DIB_RGB_COLORS,
            &raw mut p_bits,
            None,
            0,
        )
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
pub(super) unsafe fn sample_background_luma(window_rect: &RECT) -> Option<f32> {
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
