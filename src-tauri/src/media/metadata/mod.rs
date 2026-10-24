//! 音频元数据
//!
//! 标签解析、元数据缓存与封面提取。

pub mod cache;
pub mod cover;
pub mod extractor;

pub use cache::*;
pub use cover::*;
pub use extractor::*;
