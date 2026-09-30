//! 媒体模块：文件遍历与安全检查、音频元数据与封面。
//!
//! HTTP 客户端在 `crate::http_client`（media 与 lyrics 共用），歌词来源在 `crate::lyrics`。

pub mod commands;
pub mod filesystem;
pub mod metadata;
