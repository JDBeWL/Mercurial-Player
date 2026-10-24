//! 系统字体查询模块
//!
//! `scan` 扫描 `fonts/` 与系统字体，`parse` 解析 TTC/OTC 集合与前端命名约定。

mod parse;
pub mod scan;

pub use parse::{frontend_family_from_file_name, internal_font_families};
pub use scan::*;

#[cfg(test)]
mod tests;
