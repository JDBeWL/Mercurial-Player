#![allow(unsafe_code)]

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, IsWindowVisible, SET_WINDOW_POS_FLAGS, SW_HIDE,
    SW_SHOWNOACTIVATE, SetWindowLongPtrW, SetWindowPos, ShowWindow, WS_EX_TRANSPARENT,
};

use super::window::{get_hwnd, post_update, resize_window_for_font, run_message_loop};
use super::{
    APP_HANDLE, DesktopLyricWord, LYRICS_HWND, PRESET_BLUE, PRESET_DARK, PRESET_GREEN,
    PRESET_LIGHT, PRESET_ORANGE, PRESET_PINK, SHARED_STATE, SharedLyricState, now_ms,
};

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
