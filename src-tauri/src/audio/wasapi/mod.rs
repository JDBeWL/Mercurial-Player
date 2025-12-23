//! WASAPI 音频模块
//!
//! 提供 Windows Audio Session API (WASAPI) 独占模式支持。

mod exclusive;
mod player;

pub use exclusive::{
    AudioCommand, AudioResponse, PlaybackState, WasapiExclusivePlayback,
};
pub use player::{
    check_device_exclusive_support, get_exclusive_capable_devices, WasapiExclusivePlayer,
};
