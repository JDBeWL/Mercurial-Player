//! 配置模块：`AppConfig` 定义与读写落盘（[`manager::ConfigManager`]）。

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod commands;
pub mod manager;

pub use manager::{AppConfig, ConfigManager};

/// 全局数据目录覆盖（Android 专用）：桌面端配置落主程序同级 `data/`（便携化），Android 的
/// `current_exe()` 在只读 APK 内，必须改写到应用数据目录。
static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 设置全局数据目录：先设者生效，后设者被忽略并记一条 warn
///
/// 不改判：Android 上 JNI 解析的目录早于 Tauri `setup`，总是赢过 `app_setup::init` 的兜底值，
/// 而已有配置都在先设目录下，中途换路径会让升级看起来"设置丢失"。
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
