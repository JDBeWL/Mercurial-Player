//! 桌面歌词显示模块
//!
//! 在屏幕底部显示一个置顶窗口，展示当前播放歌词。
//! 使用 Direct2D 渲染，确保文字边缘平滑无毛刺。
//! 支持点击穿透（锁定模式）和顶部拖拽（解锁模式）。
//! 悬浮时显示锁定按钮和关闭按钮。
//! 支持双行歌词。
//! 字体族跟随前端歌词字体设置（原文/译文可分别指定）：
//! 优先使用系统已安装字体，未安装时尝试把 fonts/ 目录的外部字体
//! 加载为 DirectWrite 内存字体集后使用。

#![allow(unsafe_code)]

mod commands;
mod fonts;
mod manager;
mod renderer;
mod window;

pub use commands::*;
pub use manager::DesktopLyricsManager;

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 配色方案。outline 为文字描边色：取与文字色同色系的深色
/// （深/浅预设用灰阶），比纯黑描边更柔和，同时保证任意背景可读
#[derive(Clone, Copy, Debug)]
struct ColorPreset {
    text_color: u32,
    highlight_color: u32,
    outline_color: u32,
    outline_alpha: f32,
}

const PRESET_DARK: ColorPreset = ColorPreset {
    text_color: 0x00_21_21_21,
    highlight_color: 0x00_02_88_D1,
    outline_color: 0x00_EC_EF_F1,
    outline_alpha: 0.9,
};

const PRESET_LIGHT: ColorPreset = ColorPreset {
    text_color: 0x00_FF_FF_FF,
    highlight_color: 0x00_FF_C5_3D,
    outline_color: 0x00_37_47_4F,
    outline_alpha: 0.8,
};

const PRESET_BLUE: ColorPreset = ColorPreset {
    text_color: 0x00_03_A9_F4,
    highlight_color: 0x00_81_D4_FA,
    outline_color: 0x00_01_57_9B,
    outline_alpha: 0.7,
};

const PRESET_PINK: ColorPreset = ColorPreset {
    text_color: 0x00_E9_1E_63,
    highlight_color: 0x00_FF_80_AB,
    outline_color: 0x00_88_0E_4F,
    outline_alpha: 0.7,
};

const PRESET_ORANGE: ColorPreset = ColorPreset {
    text_color: 0x00_FF_98_00,
    highlight_color: 0x00_FF_CC_80,
    outline_color: 0x00_BF_36_0C,
    outline_alpha: 0.7,
};

const PRESET_GREEN: ColorPreset = ColorPreset {
    text_color: 0x00_4C_AF_50,
    highlight_color: 0x00_A5_D6_A7,
    outline_color: 0x00_1B_5E_20,
    outline_alpha: 0.7,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLyricWord {
    text: String,
    start: f32,
    end: f32,
}

struct SharedLyricState {
    current_line: String,
    sub_line: String,
    prev_line: String,
    prev_sub_line: String,
    current_words: Vec<DesktopLyricWord>,
    font_size: i32,
    /// 原文歌词字体族（来自前端歌词字体设置，渲染前由前端同步）
    font_family: String,
    /// 译文字体族（空字符串 = 跟随原文）
    translation_font_family: String,
    /// 字体设置代数：变化时渲染线程重建外部字体内存字体集
    font_generation: u32,
    is_locked: bool,
    is_hovered: bool,
    is_playing: bool,
    color_preset: ColorPreset,
    /// 配色为 auto：按窗口背后背景亮度在深/浅文字间自动切换
    preset_auto: bool,
    /// auto 模式下当前使用浅色文字（背景偏暗时为 true）
    auto_light_text: bool,
    /// 上次背景采样时间（毫秒），限流采样频率
    last_bg_sample_ms: i64,
    fade_alpha: f32,
    fade_pending: bool,
    marquee_active: bool,
    marquee_start_ms: i64,
    visual_time: f32,
    target_time: f32,
    visual_time_last_ms: i64,
    lyric_progress: f32,
    smooth_lyric_progress: f32,
    render_pending: bool,
}

static FONT_CACHE: OnceLock<Mutex<HashMap<i32, usize>>> = OnceLock::new();

static SHARED_STATE: OnceLock<Arc<Mutex<SharedLyricState>>> = OnceLock::new();
static LYRICS_HWND: OnceLock<isize> = OnceLock::new();

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::fonts::{build_memory_loader, try_load_external_font};
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_WEIGHT_NORMAL, DWriteCreateFactory, IDWriteFactory,
    };
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::core::{PCWSTR, w};

    /// 用 dev 运行目录（target/debug/fonts）中的真实字体文件验证
    /// DirectWrite 内存字体集加载链路：
    /// Factory5 → InMemoryFontFileLoader → FontSetBuilder → 集合 → FindFamilyName。
    /// 目录不存在或没有可用字体时跳过
    #[test]
    fn dwrite_external_font_load_pipeline() {
        let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("debug")
            .join("fonts");
        let entries: Vec<std::path::PathBuf> = match std::fs::read_dir(&fonts_dir) {
            Ok(it) => it
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .map(|x| {
                            let ext = x.to_string_lossy().to_lowercase();
                            matches!(ext.as_str(), "ttf" | "otf")
                        })
                        .unwrap_or(false)
                })
                .collect(),
            Err(_) => return,
        };
        if entries.is_empty() {
            return;
        }

        // SAFETY: 测试线程入口即初始化 COM，退出前配对 CoUninitialize
        let mut failures: Vec<(String, String)> = Vec::new();
        unsafe {
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let dwrite: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DWriteCreateFactory 失败");
            // 走生产构建函数（含 RegisterFontFileLoader）
            let loader = build_memory_loader(&dwrite).expect("build_memory_loader 失败");

            for path in &entries {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let family = crate::system::fonts::frontend_family_from_file_name(&file_name);
                // 走生产加载函数；外层 unsafe 块保证 COM 已初始化、loader 已注册
                let report = match try_load_external_font(
                    &dwrite,
                    &loader,
                    &path.to_string_lossy(),
                    &family,
                ) {
                    None => "try_load_external_font 失败".to_string(),
                    Some(font) => {
                        let wide: Vec<u16> = font
                            .internal_family
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        let mut index = 0u32;
                        let mut exists = windows::core::BOOL::default();
                        font.collection
                            .FindFamilyName(
                                PCWSTR::from_raw(wide.as_ptr()),
                                &raw mut index,
                                &raw mut exists,
                            )
                            .expect("FindFamilyName 失败");
                        if exists.as_bool() {
                            let format = dwrite.CreateTextFormat(
                                PCWSTR::from_raw(wide.as_ptr()),
                                Some(&font.collection),
                                DWRITE_FONT_WEIGHT_NORMAL,
                                DWRITE_FONT_STYLE_NORMAL,
                                DWRITE_FONT_STRETCH_NORMAL,
                                28.0,
                                w!("zh-cn"),
                            );
                            match format {
                                Ok(_) => "OK".to_string(),
                                Err(e) => format!("CreateTextFormat: {e}"),
                            }
                        } else {
                            format!("FindFamilyName 未命中内部族名 {}", font.internal_family)
                        }
                    }
                };
                println!("  {file_name}: {report}");
                failures.push((file_name, report));
            }
            let failures: Vec<(String, String)> = failures
                .into_iter()
                .filter(|(_, report)| report != "OK")
                .collect();
            assert!(failures.is_empty(), "失败: {failures:?}");

            if com_initialized {
                CoUninitialize();
            }
        }
    }
}
