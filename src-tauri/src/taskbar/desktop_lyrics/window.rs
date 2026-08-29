#![allow(unsafe_code)]

use std::sync::{Arc, Mutex};

use tauri::Emitter;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, HBRUSH, InvalidateRect, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GWL_EXSTYLE,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect, HCURSOR, HICON,
    HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTRANSPARENT, KillTimer, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, SET_WINDOW_POS_FLAGS, SW_HIDE, SetTimer, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCHITTEST, WM_PAINT, WM_SIZE, WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::renderer::{
    CLOSE_BTN_MARGIN, CLOSE_BTN_SIZE, get_close_btn_rect, get_lock_btn_rect, render_lyrics,
};
use super::{APP_HANDLE, LYRICS_HWND, SHARED_STATE, SharedLyricState};

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
pub(super) unsafe fn get_dpi_for_window(hwnd: HWND) -> u32 {
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

const RESIZE_EDGE_WIDTH: i32 = 18;

pub(super) fn get_hwnd() -> isize {
    LYRICS_HWND.get().copied().unwrap_or(0)
}

pub(super) fn post_update() {
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

pub(super) fn resize_window_for_font(hwnd: HWND, font_size: i32) {
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

fn point_in_rect(x: i32, y: i32, rect: &RECT) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

pub(super) fn run_message_loop(_state: Arc<Mutex<SharedLyricState>>) {
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
