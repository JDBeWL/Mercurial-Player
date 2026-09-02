//! 音频元数据模块
//!
//! 提供音轨元数据结构和处理函数。按职责拆分为:
//! - `cache` — 元数据缓存(memory + disk JSON)与封面缓存目录管理
//! - `cover` — 内嵌封面提取、封面缓存写入与导出
//! - `extractor` — 标签解析、TrackMetadata/Playlist 类型与元数据读取

pub mod cache;
pub mod cover;
pub mod extractor;

pub use cache::*;
pub use cover::*;
pub use extractor::*;
