//! 桌面歌词显示模块
//!
//! 在Windows任务栏左下角显示当前播放的歌词

#![allow(unsafe_code)]

use std::sync::{Arc, Mutex, OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM, COLORREF};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FW_NORMAL, HBRUSH, HFONT,
    PAINTSTRUCT, SetBkMode, SetTextColor, TRANSPARENT, HGDIOBJ, FONT_CHARSET, FONT_OUTPUT_PRECISION,
    FONT_CLIP_PRECISION, FONT_QUALITY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, LWA_ALPHA,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
    WM_DESTROY, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

/// 全局桌面歌词管理器
static DESKTOP_LYRICS_MANAGER: OnceLock<Arc<Mutex<DesktopLyricsManager>>> = OnceLock::new();

/// 桌面歌词管理器
pub struct DesktopLyricsManager {
    hwnd: isize, // 使用 isize 而不是 HWND 以实现 Send
    current_lyric: String,
    is_visible: bool,
    font: isize,
    bg_brush: isize,
}

// 手动实现 Send 和 Sync
unsafe impl Send for DesktopLyricsManager {}
unsafe impl Sync for DesktopLyricsManager {}

impl DesktopLyricsManager {
    /// 创建新的桌面歌词管理器
    pub fn new() -> Self {
        Self {
            hwnd: 0,
            current_lyric: String::new(),
            is_visible: false,
            font: 0,
            bg_brush: 0,
        }
    }

    /// 初始化桌面歌词窗口
    pub fn initialize(&mut self) -> Result<(), String> {
        if self.hwnd != 0 {
            return Ok(());
        }

        unsafe {
            let instance = GetModuleHandleW(None)
                .map_err(|e| format!("Failed to get module handle: {e}"))?;

            // 注册窗口类
            let class_name = w!("DesktopLyricsWindow");
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance.into(),
                hIcon: Default::default(),
                hCursor: Default::default(),
                hbrBackground: HBRUSH::default(),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: class_name,
            };

            if RegisterClassW(&wc) == 0 {
                return Err("Failed to register window class".to_string());
            }

            // 创建窗口
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name,
                w!("Desktop Lyrics"),
                WS_POPUP,
                10,  // 左下角 x
                0,   // 临时 y，稍后调整
                800, // 宽度
                60,  // 高度
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("Failed to create window: {e}"))?;

            // 设置窗口透明度
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA)
                .map_err(|e| format!("Failed to set window attributes: {e}"))?;

            // 创建字体
            let font = CreateFontW(
                24,                // 高度
                0,                 // 宽度
                0,                 // 倾斜角度
                0,                 // 基线角度
                FW_NORMAL.0 as i32, // 粗细
                0,                 // 斜体
                0,                 // 下划线
                0,                 // 删除线
                FONT_CHARSET(1),   // 字符集 (DEFAULT_CHARSET)
                FONT_OUTPUT_PRECISION(0), // 输出精度
                FONT_CLIP_PRECISION(0),   // 裁剪精度
                FONT_QUALITY(5),   // 质量 (CLEARTYPE_QUALITY)
                0,                 // 间距和字体族
                w!("Microsoft YaHei"), // 字体名称
            );

            // 创建背景画刷（半透明黑色）
            let bg_brush = CreateSolidBrush(COLORREF(0x00000000));

            self.hwnd = hwnd.0 as isize;
            self.font = font.0 as isize;
            self.bg_brush = bg_brush.0 as isize;

            // 调整窗口位置到屏幕左下角
            self.position_window()?;

            println!("Desktop lyrics window initialized");
            Ok(())
        }
    }

    /// 调整窗口位置到屏幕左下角
    fn position_window(&self) -> Result<(), String> {
        unsafe {
            // 获取任务栏高度（假设40像素）
            let taskbar_height = 40;
            let screen_height = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
            );

            let y = screen_height - 60 - taskbar_height;

            SetWindowPos(
                HWND(self.hwnd as *mut _),
                Some(HWND_TOPMOST),
                10,
                y,
                800,
                60,
                SWP_NOACTIVATE,
            )
            .map_err(|e| format!("Failed to position window: {e}"))?;

            Ok(())
        }
    }

    /// 更新歌词文本
    pub fn update_lyric(&mut self, lyric: &str) -> Result<(), String> {
        self.current_lyric = lyric.to_string();
        // 窗口会在下次 WM_PAINT 消息时自动重绘
        Ok(())
    }

    /// 显示歌词窗口
    pub fn show(&mut self) -> Result<(), String> {
        if self.hwnd == 0 {
            self.initialize()?;
        }

        unsafe {
            ShowWindow(HWND(self.hwnd as *mut _), SW_SHOWNOACTIVATE);
        }

        self.is_visible = true;
        Ok(())
    }

    /// 隐藏歌词窗口
    pub fn hide(&mut self) -> Result<(), String> {
        if self.hwnd != 0 {
            unsafe {
                ShowWindow(HWND(self.hwnd as *mut _), SW_HIDE);
            }
        }

        self.is_visible = false;
        Ok(())
    }

    /// 获取当前歌词
    pub fn get_current_lyric(&self) -> &str {
        &self.current_lyric
    }

    /// 获取窗口句柄
    pub fn get_hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    /// 获取字体句柄
    pub fn get_font(&self) -> HFONT {
        HFONT(self.font as *mut _)
    }

    /// 获取背景画刷
    pub fn get_bg_brush(&self) -> HBRUSH {
        HBRUSH(self.bg_brush as *mut _)
    }

    /// 是否可见
    pub fn is_visible(&self) -> bool {
        self.is_visible
    }
}

impl Drop for DesktopLyricsManager {
    fn drop(&mut self) {
        unsafe {
            if self.hwnd != 0 {
                let _ = DestroyWindow(HWND(self.hwnd as *mut _));
            }
            if self.font != 0 {
                let _ = DeleteObject(HGDIOBJ(self.font as *mut _));
            }
            if self.bg_brush != 0 {
                let _ = DeleteObject(HGDIOBJ(self.bg_brush as *mut _));
            }
        }
    }
}

/// 窗口过程
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            if let Some(manager) = DESKTOP_LYRICS_MANAGER.get() {
                // 在窗口回调中尽量避免阻塞：尝试获取锁，失败则跳过绘制
                if let Ok(guard) = manager.try_lock() {
                    // 设置背景透明
                    SetBkMode(hdc, TRANSPARENT);

                    // 设置文字颜色（白色）
                    SetTextColor(hdc, COLORREF(0x00FFFFFF));

                    // 选择字体（仅当字体句柄有效时）
                    let mut old_font = HGDIOBJ::default();
                    if guard.get_font().0 != std::ptr::null_mut() {
                        old_font = windows::Win32::Graphics::Gdi::SelectObject(
                            hdc,
                            HGDIOBJ(guard.get_font().0 as *mut _),
                        );
                    }

                    // 获取客户区
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);

                    // 绘制文字 - 必须保证以 NUL 结尾
                    let mut text: Vec<u16> = guard.get_current_lyric().encode_utf16().collect();
                    text.push(0);

                    // 使用 DrawTextW 绘制 UTF-16 字符串（windows-rs 提供接收 &mut [u16] 的重载）
                    let _ = windows::Win32::Graphics::Gdi::DrawTextW(
                        hdc,
                        &mut text,
                        &mut rect,
                        windows::Win32::Graphics::Gdi::DT_CENTER
                            | windows::Win32::Graphics::Gdi::DT_VCENTER
                            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
                    );

                    // 恢复旧字体（如果存在）
                    if old_font.0 != std::ptr::null_mut() {
                        windows::Win32::Graphics::Gdi::SelectObject(hdc, old_font);
                    }
                }
            }

            EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        WM_DESTROY => unsafe {
            PostQuitMessage(0);
            LRESULT(0)
        },
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 获取全局桌面歌词管理器
pub fn get_desktop_lyrics_manager() -> Arc<Mutex<DesktopLyricsManager>> {
    DESKTOP_LYRICS_MANAGER
        .get_or_init(|| Arc::new(Mutex::new(DesktopLyricsManager::new())))
        .clone()
}

/// 初始化桌面歌词
pub fn init_desktop_lyrics() -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock desktop lyrics manager: {e}"))?;
    guard.initialize()
}

/// 更新桌面歌词
pub fn update_desktop_lyric(lyric: &str) -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock desktop lyrics manager: {e}"))?;
    guard.update_lyric(lyric)
}

/// 显示桌面歌词
pub fn show_desktop_lyrics() -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock desktop lyrics manager: {e}"))?;
    guard.show()
}

/// 隐藏桌面歌词
pub fn hide_desktop_lyrics() -> Result<(), String> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock desktop lyrics manager: {e}"))?;
    guard.hide()
}
