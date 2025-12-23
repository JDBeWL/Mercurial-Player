//! 音频模块
//!
//! 提供音频播放、解码、设备管理等功能。

pub mod commands;
pub mod decoder;
pub mod device;
pub mod playback;
pub mod wasapi;

// 重新导出常用类型
pub use decoder::{LockFreeSymphoniaSource, SymphoniaDecoder, SymphoniaSource};
pub use device::AudioDeviceInfo;
pub use playback::{PlaybackStatus, VisualizationSource};
pub use wasapi::{PlaybackState, WasapiExclusivePlayback};
