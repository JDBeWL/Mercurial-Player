//! 系统模块
//!
//! 提供系统信息获取和窗口管理功能。

pub mod commands;

// 重新导出命令
pub use commands::{
    get_system_fonts, get_system_info, set_mini_mode,
};
