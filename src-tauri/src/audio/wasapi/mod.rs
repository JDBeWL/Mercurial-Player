//! WASAPI 音频模块
//!
//! Windows 独占模式支持；仍走 Windows 音频栈，未实现 ASIO。

mod exclusive;
mod player;

pub use exclusive::{AudioCommand, AudioResponse, PlaybackState, WasapiExclusivePlayback};
pub use player::check_device_exclusive_support;
