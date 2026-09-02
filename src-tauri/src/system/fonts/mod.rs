//! 系统字体查询模块
//!
//! - [`scan`]: 外部字体扫描(fonts/ 目录)与系统字体枚举
//! - [`parse`]: TTC/OTC 集合二进制解析与前端命名约定解析

mod parse;
pub mod scan;

pub use parse::{frontend_family_from_file_name, internal_font_families};
pub use scan::*;

#[cfg(test)]
mod tests;
