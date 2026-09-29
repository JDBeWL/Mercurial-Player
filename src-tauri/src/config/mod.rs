//! 配置模块：`AppConfig` 定义与读写落盘（[`manager::ConfigManager`]）。

use std::path::PathBuf;
use std::sync::OnceLock;

pub mod commands;
pub mod manager;

// 重新导出常用类型
pub use manager::{AppConfig, ConfigManager};

/// 全局数据目录覆盖（Android 专用）
///
/// 桌面端配置文件/数据文件落主程序同级 `data/`（便携化）；Android 的
/// `current_exe()` 位于只读的 APK 内，必须改写到应用数据目录。
/// 由 `lib.rs::run`（首个 ConfigManager 之前）与 `app_setup::init` 设置，未设置时保持原逻辑。
static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 设置全局数据目录（仅应在启动装配时调用一次）
pub fn set_data_dir_override(dir: PathBuf) {
    let _ = DATA_DIR_OVERRIDE.set(dir);
}

/// 获取全局数据目录（如有）
pub fn data_dir_override() -> Option<&'static PathBuf> {
    DATA_DIR_OVERRIDE.get()
}
