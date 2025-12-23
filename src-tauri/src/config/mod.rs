//! 配置模块
//!
//! 提供应用程序配置的管理功能。

pub mod commands;
pub mod manager;

// 重新导出常用类型
pub use manager::{
    AppConfig, AudioConfig, ConfigManager, DirectoryScanConfig, GeneralConfig, PlaylistConfig,
    TitleExtractionConfig,
};
