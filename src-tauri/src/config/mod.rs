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
/// 未设置时保持原逻辑。
static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// 设置全局数据目录：先设者生效，后设者被忽略并记一条 warn
///
/// Android 上 `lib.rs::run()` 的 JNI 解析（`/data/data/<pkg>`）早于 Tauri `setup`，
/// 因此总是赢过 `app_setup::init` 的 `<app_data>/files`，后者只在 JNI 失败时兜底。
/// 沿用先设者而非改判：设备上现有 config.json/eq.json 都落在先设目录，
/// 改路径会让用户升级后看起来"设置丢失"。
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

/// 获取全局数据目录（如有）
pub fn data_dir_override() -> Option<&'static PathBuf> {
    DATA_DIR_OVERRIDE.get()
}
