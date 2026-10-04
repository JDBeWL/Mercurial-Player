//! WASAPI 音频模块
//!
//! 提供 Windows Audio Session API (WASAPI) 独占模式支持。
//! 独占模式仍走 Windows 音频栈，项目未实现更纯净的 ASIO 后端。

mod exclusive;
mod player;

pub use exclusive::{AudioCommand, AudioResponse, PlaybackState, WasapiExclusivePlayback};
pub use player::check_device_exclusive_support;
