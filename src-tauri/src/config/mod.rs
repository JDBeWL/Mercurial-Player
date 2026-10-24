//! 配置模块
//!
//! `AppConfig` 定义与 `config.json` 的读写落盘。

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod commands;
pub mod manager;

pub use manager::{AppConfig, ConfigManager};

/// 全局数据目录覆盖（Android 专用）：`current_exe()` 在只读 APK 内，配置必须改写到应用数据目录。
static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 设置全局数据目录，先设者生效，后设者被忽略并记 warn。
///
/// JNI 侧早于 Tauri `setup` 取得目录、总是赢过 `app_setup::init` 的兜底值，中途换路径会让已有配置看起来"设置丢失"。
#[must_use = "返回是否采用了本次目录，调用方需据此决定是否记录回退"]
pub fn set_data_dir_override(dir: PathBuf) -> bool {
    if DATA_DIR_OVERRIDE.set(dir.clone()).is_err() {
        if let Some(existing) = DATA_DIR_OVERRIDE.get() {
            log::warn!(
                "数据目录已定为 {}，忽略 {}",
                existing.display(),
                dir.display()
            );
        }
        return false;
    }
    true
}

pub fn data_dir_override() -> Option<&'static PathBuf> {
    DATA_DIR_OVERRIDE.get()
}
