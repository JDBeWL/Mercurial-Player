//! 媒体模块
//!
//! 音频文件遍历与安全检查、元数据与封面、BPM 与节拍分析。

pub mod beat_cache;
pub mod beat_model;
pub mod bpm;
pub mod commands;
pub mod filesystem;
pub mod metadata;
// `ort` 的 directml feature 在非 Windows 平台会拉一套用不上的绑定，故仅 Windows 编译。
#[cfg(windows)]
pub mod ort_model;
