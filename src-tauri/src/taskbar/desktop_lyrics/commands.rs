use crate::error::AppError;
use std::sync::{Arc, Mutex, OnceLock};

use tauri::command;

use super::manager::DesktopLyricsManager;
use super::window::post_update;
use super::{DesktopLyricWord, SHARED_STATE};

static DESKTOP_LYRICS_MANAGER: OnceLock<Arc<Mutex<DesktopLyricsManager>>> = OnceLock::new();

/// 使桌面歌词的字体内存缓存全部失效：渲染线程在下一帧重建外部字体
/// 名字索引、丢弃已按需加载的字体集合与文本格式缓存。
/// fonts/ 目录文件变化或用户主动清理字体缓存后调用
pub fn invalidate_font_caches() -> Result<(), AppError> {
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
pub fn show_desktop_lyrics(app: tauri::AppHandle) -> Result<(), AppError> {
    let manager = get_desktop_lyrics_manager();
    let mut guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.initialize(app)?;
    guard.show()
}

#[command]
pub fn hide_desktop_lyrics() -> Result<(), AppError> {
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
) -> Result<(), AppError> {
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
pub fn set_desktop_lyrics_locked(locked: bool) -> Result<(), AppError> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_locked(locked)
}

#[command]
pub fn set_desktop_lyrics_font_size(size: i32) -> Result<(), AppError> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_font_size(size)
}

#[command]
pub fn set_desktop_lyrics_font_family(
    font_family: String,
    translation_font_family: String,
) -> Result<(), AppError> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_font_family(&font_family, &translation_font_family)
}

#[command]
pub fn set_desktop_lyrics_color_preset(preset: String) -> Result<(), AppError> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    guard.set_color_preset(&preset)
}

#[command]
pub fn is_desktop_lyrics_visible() -> Result<bool, AppError> {
    let manager = get_desktop_lyrics_manager();
    let guard = manager.lock().map_err(|e| format!("Lock error: {e}"))?;
    Ok(guard.is_visible())
}
