//! 系统模块
//!
//! 提供系统信息获取和窗口管理功能。

pub mod commands;
pub mod fonts;

// 重新导出命令
pub use commands::{
    get_app_version, get_display_refresh_rates, get_screen_refresh_rate, get_system_fonts,
    get_system_info, set_mini_mode,
};
