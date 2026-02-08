//! 自动更新模块
//!
//! 处理应用自动更新功能，包括从GitHub检查更新、下载和安装

pub mod commands;

pub use commands::{download_and_install_update, get_app_version};
