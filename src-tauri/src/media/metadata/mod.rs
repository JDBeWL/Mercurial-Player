//! 音频元数据：`extractor` 解析标签、`cache` 管缓存、`cover` 管封面，各自的职责见其模块文档。

pub mod cache;
pub mod cover;
pub mod extractor;

pub use cache::*;
pub use cover::*;
pub use extractor::*;
