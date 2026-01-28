//! Windows任务栏缩略图工具栏
//!
//! 使用ITaskbarList3

#![allow(unsafe_code)] // Windows API交互需要unsafe

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::UI::Shell::{
    ITaskbarList3, TaskbarList, THUMBBUTTON, THUMBBUTTONMASK, THB_BITMAP,
    THB_FLAGS, THB_TOOLTIP, THBF_DISMISSONCLICK, THBF_ENABLED,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetSystemMetrics, HICON, SM_CXSMICON};

use super::PlaybackState;

/// 缩略图按钮ID
pub const BTN_PREVIOUS: u32 = 0;
pub const BTN_PLAY_PAUSE: u32 = 1;
pub const BTN_NEXT: u32 = 2;

/// 按钮消息ID(WM_COMMAND偏移)
pub const THBN_CLICKED: u32 = 0x1800;

/// 全局任务栏管理器实例
static TASKBAR_MANAGER: OnceLock<Arc<Mutex<TaskbarManager>>> = OnceLock::new();

/// 任务栏缩略图工具栏管理器
pub struct TaskbarManager {
    taskbar_list: Option<ITaskbarList3>,
    hwnd: HWND,
    initialized: AtomicBool,
    current_state: PlaybackState,
    icons: TaskbarIcons,
}

/// 任务栏按钮图标
struct TaskbarIcons {
    prev_icon: Option<HICON>,
    play_icon: Option<HICON>,
    pause_icon: Option<HICON>,
    next_icon: Option<HICON>,
}

impl Default for TaskbarIcons {
    fn default() -> Self {
        Self {
            prev_icon: None,
            play_icon: None,
            pause_icon: None,
            next_icon: None,
        }
    }
}

impl Drop for TaskbarIcons {
    fn drop(&mut self) {
        unsafe {
            if let Some(icon) = self.prev_icon.take() {
                let _ = DestroyIcon(icon);
            }
            if let Some(icon) = self.play_icon.take() {
                let _ = DestroyIcon(icon);
            }
            if let Some(icon) = self.pause_icon.take() {
                let _ = DestroyIcon(icon);
            }
            if let Some(icon) = self.next_icon.take() {
                let _ = DestroyIcon(icon);
            }
        }
    }
}

// SAFETY: TaskbarManager 只在主线程创建和使用，且通过 Mutex 保护
unsafe impl Send for TaskbarManager {}
unsafe impl Sync for TaskbarManager {}

impl TaskbarManager {
    /// 创建新的任务栏管理器
    pub fn new() -> Self {
        Self {
            taskbar_list: None,
            hwnd: HWND::default(),
            initialized: AtomicBool::new(false),
            current_state: PlaybackState::Stopped,
            icons: TaskbarIcons::default(),
        }
    }

    /// 初始化任务栏按钮
    ///
    /// # 也许它是安全的（？）
    /// 调用Windows COM API，需要有效的窗口句柄
    pub fn initialize(&mut self, hwnd: isize) -> Result<(), String> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }

        self.hwnd = HWND(hwnd as *mut std::ffi::c_void);

        // 创建ITaskbarList3实例
        let taskbar_list: ITaskbarList3 = unsafe {
            CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create TaskbarList: {e}"))?
        };

        // 初始化TaskbarList
        unsafe {
            taskbar_list
                .HrInit()
                .map_err(|e| format!("Failed to initialize TaskbarList: {e}"))?;
        }

        // 创建图标
        self.create_icons()?;

        // 添加缩略图按钮
        self.add_thumb_buttons(&taskbar_list)?;

        self.taskbar_list = Some(taskbar_list);
        self.initialized.store(true, Ordering::SeqCst);

        println!("Taskbar thumbnail toolbar initialized");
        Ok(())
    }

    /// 创建按钮图标
    fn create_icons(&mut self) -> Result<(), String> {
        // 使用简单的形状创建图标
        self.icons.prev_icon = Some(create_prev_icon()?);
        self.icons.play_icon = Some(create_play_icon()?);
        self.icons.pause_icon = Some(create_pause_icon()?);
        self.icons.next_icon = Some(create_next_icon()?);
        Ok(())
    }

    /// 添加缩略图按钮
    fn add_thumb_buttons(&self, taskbar_list: &ITaskbarList3) -> Result<(), String> {
        let prev_tooltip: Vec<u16> = "上一首\0".encode_utf16().collect();
        let play_tooltip: Vec<u16> = "播放\0".encode_utf16().collect();
        let next_tooltip: Vec<u16> = "下一首\0".encode_utf16().collect();

        let mut buttons = [
            THUMBBUTTON {
                dwMask: THB_BITMAP | THB_TOOLTIP | THB_FLAGS,
                iId: BTN_PREVIOUS,
                iBitmap: 0,
                hIcon: self.icons.prev_icon.unwrap_or_default(),
                szTip: [0; 260],
                dwFlags: THBF_ENABLED | THBF_DISMISSONCLICK,
            },
            THUMBBUTTON {
                dwMask: THB_BITMAP | THB_TOOLTIP | THB_FLAGS,
                iId: BTN_PLAY_PAUSE,
                iBitmap: 0,
                hIcon: self.icons.play_icon.unwrap_or_default(),
                szTip: [0; 260],
                dwFlags: THBF_ENABLED | THBF_DISMISSONCLICK,
            },
            THUMBBUTTON {
                dwMask: THB_BITMAP | THB_TOOLTIP | THB_FLAGS,
                iId: BTN_NEXT,
                iBitmap: 0,
                hIcon: self.icons.next_icon.unwrap_or_default(),
                szTip: [0; 260],
                dwFlags: THBF_ENABLED | THBF_DISMISSONCLICK,
            },
        ];

        // 复制tooltip文本
        copy_tooltip(&mut buttons[0].szTip, &prev_tooltip);
        copy_tooltip(&mut buttons[1].szTip, &play_tooltip);
        copy_tooltip(&mut buttons[2].szTip, &next_tooltip);

        // 修改dwMask，使用hIcon而非iBitmap
        for button in &mut buttons {
            button.dwMask = THUMBBUTTONMASK(0x2) | THB_TOOLTIP | THB_FLAGS; // THB_ICON = 0x2
        }

        unsafe {
            taskbar_list
                .ThumbBarAddButtons(self.hwnd, &buttons)
                .map_err(|e| format!("Failed to add thumb buttons: {e}"))?;
        }

        Ok(())
    }

    /// 更新播放/暂停按钮状态
    pub fn update_playback_state(&mut self, state: PlaybackState) -> Result<(), String> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err("Taskbar not initialized".to_string());
        }

        if self.current_state == state {
            return Ok(());
        }

        self.current_state = state;

        let taskbar_list = self
            .taskbar_list
            .as_ref()
            .ok_or("TaskbarList not available")?;

        let (icon, tooltip) = match state {
            PlaybackState::Playing => (self.icons.pause_icon, "暂停\0"),
            PlaybackState::Paused | PlaybackState::Stopped => (self.icons.play_icon, "播放\0"),
        };

        let tooltip_utf16: Vec<u16> = tooltip.encode_utf16().collect();

        let mut button = THUMBBUTTON {
            dwMask: THUMBBUTTONMASK(0x2) | THB_TOOLTIP | THB_FLAGS, // THB_ICON = 0x2
            iId: BTN_PLAY_PAUSE,
            iBitmap: 0,
            hIcon: icon.unwrap_or_default(),
            szTip: [0; 260],
            dwFlags: THBF_ENABLED | THBF_DISMISSONCLICK,
        };

        copy_tooltip(&mut button.szTip, &tooltip_utf16);

        unsafe {
            taskbar_list
                .ThumbBarUpdateButtons(self.hwnd, &[button])
                .map_err(|e| format!("Failed to update thumb button: {e}"))?;
        }

        Ok(())
    }

    /// 获取当前播放状态
    pub fn get_state(&self) -> PlaybackState {
        self.current_state
    }
}

impl Default for TaskbarManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 复制tooltip到固定大小数组
fn copy_tooltip(dest: &mut [u16; 260], src: &[u16]) {
    let len = src.len().min(259);
    dest[..len].copy_from_slice(&src[..len]);
    dest[len] = 0;
}

/// 获取系统小图标尺寸
fn get_icon_size() -> i32 {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    if size > 0 { size } else { 16 } // 如果获取失败，返回默认值16
}

/// 创建上一首图标 (|◀)
fn create_prev_icon() -> Result<HICON, String> {
    let size = get_icon_size();
    
    create_icon_from_pixels(size, size, |x, y, w, h| {
        let scale = w as f32 / 16.0;
        let cy = h as f32 / 2.0;
        let dy = (y as f32 - cy).abs();

        // 左边竖线
        let bar_left = (2.0 * scale) as i32;
        let bar_right = (3.0 * scale) as i32;
        let bar_height = 5.0 * scale;
        let in_bar = x >= bar_left && x <= bar_right && dy < bar_height;

        // 向左的三角形 ◀
        let tri_right = 11.0 * scale;
        let tri_width = 6.0 * scale;
        let dx = tri_right - x as f32;
        let in_triangle = dx > 0.0 && dx < tri_width && dy < (tri_width - dx) * 0.85;

        in_bar || in_triangle
    })
}

/// 创建播放图标 (▶)
fn create_play_icon() -> Result<HICON, String> {
    let size = get_icon_size();
    
    create_icon_from_pixels(size, size, |x, y, w, h| {
        let scale = w as f32 / 16.0;
        let cx = 5.0 * scale;
        let cy = h as f32 / 2.0;
        let dx = x as f32 - cx;
        let dy = (y as f32 - cy).abs();

        // 向右的三角形
        let tri_width = 8.0 * scale;
        dx >= 0.0 && dx < tri_width && dy < (tri_width - dx) * 0.7
    })
}

/// 创建暂停图标 (❚❚)
fn create_pause_icon() -> Result<HICON, String> {
    let size = get_icon_size();
    
    create_icon_from_pixels(size, size, |x, y, w, h| {
        let scale = w as f32 / 16.0;
        let cy = h as f32 / 2.0;
        let dy = (y as f32 - cy).abs();

        // 两个竖条
        let bar1_left = (4.0 * scale) as i32;
        let bar1_right = (5.0 * scale) as i32;
        let bar2_left = (10.0 * scale) as i32;
        let bar2_right = (11.0 * scale) as i32;
        let bar_height = 5.0 * scale;
        
        let in_bar1 = x >= bar1_left && x <= bar1_right && dy < bar_height;
        let in_bar2 = x >= bar2_left && x <= bar2_right && dy < bar_height;

        in_bar1 || in_bar2
    })
}

/// 创建下一首图标 (▶|)
fn create_next_icon() -> Result<HICON, String> {
    let size = get_icon_size();
    
    create_icon_from_pixels(size, size, |x, y, w, h| {
        let scale = w as f32 / 16.0;
        let cy = h as f32 / 2.0;
        let dy = (y as f32 - cy).abs();

        // 向右的三角形 ▶
        let tri_left = 4.0 * scale;
        let tri_width = 6.0 * scale;
        let dx = x as f32 - tri_left;
        let in_triangle = dx > 0.0 && dx < tri_width && dy < (tri_width - dx) * 0.85;

        // 右边竖线
        let bar_left = (12.0 * scale) as i32;
        let bar_right = (13.0 * scale) as i32;
        let bar_height = 5.0 * scale;
        let in_bar = x >= bar_left && x <= bar_right && dy < bar_height;

        in_triangle || in_bar
    })
}

/// 从像素回调创建图标
fn create_icon_from_pixels<F>(width: i32, height: i32, pixel_fn: F) -> Result<HICON, String>
where
    F: Fn(i32, i32, i32, i32) -> bool,
{
    unsafe {
        let hdc = CreateCompatibleDC(None);
        if hdc.is_invalid() {
            return Err("Failed to create compatible DC".to_string());
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // 自上而下
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default()],
        };

        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .map_err(|e| format!("Failed to create DIB section: {e}"))?;

        if bits.is_null() {
            let _ = DeleteDC(hdc);
            return Err("DIB bits is null".to_string());
        }

        // 填充像素数据
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (width * height) as usize);

        // 先绘制内容
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                if pixel_fn(x, y, width, height) {
                    // 白色，完全不透明
                    pixels[idx] = 0xFFFF_FFFF;
                } else {
                    // 透明
                    pixels[idx] = 0x0000_0000;
                }
            }
        }

        // 添加灰色边框
        let gray_border: u32 = 0xFF80_8080; // 灰色
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                // 如果当前像素是透明的，检查是否相邻有白色
                if pixels[idx] == 0x0000_0000 {
                    let mut has_neighbor = false;
                    for dy in -1..=1_i32 {
                        for dx in -1..=1_i32 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let nx = x + dx;
                            let ny = y + dy;
                            if nx >= 0 && nx < width && ny >= 0 && ny < height {
                                let nidx = (ny * width + nx) as usize;
                                if pixels[nidx] == 0xFFFF_FFFF {
                                    has_neighbor = true;
                                    break;
                                }
                            }
                        }
                        if has_neighbor {
                            break;
                        }
                    }
                    if has_neighbor {
                        pixels[idx] = gray_border;
                    }
                }
            }
        }

        // 创建掩码位图
        let mask_bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default()],
        };

        let mut mask_bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let mask_hbm = CreateDIBSection(Some(hdc), &mask_bmi, DIB_RGB_COLORS, &mut mask_bits, None, 0)
            .map_err(|e| format!("Failed to create mask DIB section: {e}"))?;

        if !mask_bits.is_null() {
            let mask_pixels =
                std::slice::from_raw_parts_mut(mask_bits as *mut u32, (width * height) as usize);
            for y in 0..height {
                for x in 0..width {
                    let idx = (y * width + x) as usize;
                    // 如果主位图有内容（非透明），掩码为0（显示）
                    if pixels[idx] != 0x0000_0000 {
                        mask_pixels[idx] = 0x0000_0000; // 不透明部分的掩码
                    } else {
                        mask_pixels[idx] = 0xFFFF_FFFF; // 透明部分的掩码
                    }
                }
            }
        }

        // 使用 ICONINFO 创建图标
        let iconinfo = windows::Win32::UI::WindowsAndMessaging::ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_hbm,
            hbmColor: hbm,
        };

        let hicon = windows::Win32::UI::WindowsAndMessaging::CreateIconIndirect(&iconinfo)
            .map_err(|e| format!("Failed to create icon: {e}"))?;

        // 清理
        let _ = DeleteObject(hbm.into());
        let _ = DeleteObject(mask_hbm.into());
        let _ = DeleteDC(hdc);

        Ok(hicon)
    }
}

/// 获取全局任务栏管理器
pub fn get_taskbar_manager() -> Arc<Mutex<TaskbarManager>> {
    TASKBAR_MANAGER
        .get_or_init(|| Arc::new(Mutex::new(TaskbarManager::new())))
        .clone()
}

/// 初始化任务栏（在主窗口创建后调用）
pub fn init_taskbar(hwnd: isize) -> Result<(), String> {
    let manager = get_taskbar_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock taskbar manager: {e}"))?;
    guard.initialize(hwnd)
}

/// 更新播放状态
pub fn update_playback_state(state: PlaybackState) -> Result<(), String> {
    let manager = get_taskbar_manager();
    let mut guard = manager
        .lock()
        .map_err(|e| format!("Failed to lock taskbar manager: {e}"))?;
    guard.update_playback_state(state)
}

