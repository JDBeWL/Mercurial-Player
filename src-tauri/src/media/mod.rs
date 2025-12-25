//! 媒体模块
//!
//! 提供文件系统操作和音频元数据处理功能。

pub mod commands;
pub mod filesystem;
pub mod http_client;
pub mod metadata;
pub mod netease;

// 重新导出常用类型
pub use filesystem::{get_audio_files_from_dir, read_dir, AUDIO_EXTENSIONS};
pub use metadata::{Playlist, TrackMetadata};
