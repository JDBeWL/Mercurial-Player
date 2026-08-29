//! 应用状态组装
//!
//! 独立于 Tauri Builder 的 [`AppState`] 构建逻辑,保持 main() 精简。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

use mercurial_player::config::ConfigManager;
use mercurial_player::equalizer::{Equalizer, GlobalEqualizer};
use mercurial_player::{
    AppState, AudioOutputState, DecodeThreadState, FadeControl, PlayerState, TrackState,
    VisualizationState,
};

#[cfg(windows)]
use mercurial_player::audio::{DeviceMonitor, WasapiExclusivePlayback};

#[cfg(not(windows))]
use mercurial_player::{Placeholder, audio::DeviceMonitor};

/// 跨平台的播放器类型别名
#[cfg(windows)]
pub type PlatformPlayer = WasapiExclusivePlayback;
#[cfg(not(windows))]
pub type PlatformPlayer = Placeholder;

/// 启动期创建的音频输出三件套(sink 由流派生,流需保活)
pub struct AudioOutput {
    pub sink: rodio::Player,
    pub mixer_sink: rodio::stream::MixerDeviceSink,
    pub wasapi_player: Option<PlatformPlayer>,
}

/// 组装应用全局状态
pub fn build_app_state(
    output: AudioOutput,
    device_name: String,
    exclusive_mode_enabled: bool,
    fade_enabled: bool,
    config_manager: ConfigManager,
) -> AppState {
    let AudioOutput {
        sink,
        mixer_sink,
        wasapi_player,
    } = output;

    // wasapi_player 在非 Windows 平台恒为 None(两个构造函数都不产生独占播放器),
    // 因此直接传值即可: is_some() 在 Linux 上为 false,与旧的条件编译分支等价
    AppState {
        player: PlayerState {
            output: AudioOutputState {
                sink: Arc::new(Mutex::new(sink)),
                output_stream: Arc::new(Mutex::new(Some(mixer_sink))),
                target_volume: Arc::new(Mutex::new(1.0)),
                current_device_name: Arc::new(Mutex::new(device_name.clone())),
                exclusive_mode: Arc::new(Mutex::new(
                    exclusive_mode_enabled && wasapi_player.is_some(),
                )),
                wasapi_player: Arc::new(Mutex::new(wasapi_player)),
            },
            track: TrackState {
                current_source: Arc::new(Mutex::new(None)),
                current_path: Arc::new(Mutex::new(None)),
            },
            visualization: VisualizationState {
                waveform_data: Arc::new(Mutex::new(Vec::with_capacity(1024))),
                spectrum_data: Arc::new(Mutex::new(vec![0.0; 128])),
                target_fps: Arc::new(AtomicU64::new(60)), // 默认60fps
            },
            decode: DecodeThreadState {
                generation: Arc::new(AtomicU64::new(0)),
                id: Arc::new(AtomicU64::new(0)),
            },
            equalizer: Arc::new(Mutex::new(Equalizer::new(48000, 2))),
            device_monitor: Arc::new(Mutex::new(DeviceMonitor::new(device_name))),
            fade: FadeControl {
                generation: Arc::new(AtomicU32::new(0)),
                enabled: Arc::new(AtomicBool::new(fade_enabled)),
            },
        },
        config_manager,
        equalizer: GlobalEqualizer::new(),
    }
}
