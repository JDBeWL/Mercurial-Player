//! 应用状态组装：独立于 Tauri Builder 的 [`AppState`] 构建逻辑。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

use crate::config::ConfigManager;
use crate::equalizer::GlobalEqualizer;
use crate::{
    AppState, AudioOutputState, DecodeThreadState, FadeControl, PlayerState, TrackState,
    VisualizationState,
};

#[cfg(windows)]
use crate::audio::{DeviceMonitor, PlaybackQueue, WasapiExclusivePlayback};

#[cfg(target_os = "android")]
use crate::audio::{AaudioExclusivePlayer, DeviceMonitor, PlaybackQueue};

#[cfg(not(any(windows, target_os = "android")))]
use crate::{
    Placeholder,
    audio::{DeviceMonitor, PlaybackQueue},
};

/// 跨平台的"独占/直出"播放器类型别名：Windows = WASAPI 独占，Android = AAudio 独占
/// （USB DAC 位完美），其它 = 占位实现。三者共用同一套方法签名，命令层与解码推送线程因此
/// 不必按平台分支。
#[cfg(windows)]
pub type PlatformPlayer = WasapiExclusivePlayback;
#[cfg(target_os = "android")]
pub type PlatformPlayer = AaudioExclusivePlayer;
#[cfg(not(any(windows, target_os = "android")))]
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

    // 均衡器设置独立落盘到 data/eq.json(与 config.json 分开,高频低频写互不干扰)
    let equalizer = GlobalEqualizer::with_persistence(config_manager.get_config_directory());

    // 其它平台根本建不出独占播放器，is_some() 恒为 false，直接传值即可
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
                current_path: Arc::new(Mutex::new(None)),
            },
            visualization: VisualizationState {
                spectrum_data: Arc::new(Mutex::new(vec![0.0; 128])),
                target_fps: Arc::new(AtomicU64::new(60)), // 默认60fps
            },
            decode: DecodeThreadState {
                generation: Arc::new(AtomicU64::new(0)),
                id: Arc::new(AtomicU64::new(0)),
            },
            device_monitor: Arc::new(Mutex::new(DeviceMonitor::new(device_name))),
            queue: Arc::new(Mutex::new(PlaybackQueue::new())),
            fade: FadeControl {
                generation: Arc::new(AtomicU32::new(0)),
                enabled: Arc::new(AtomicBool::new(fade_enabled)),
            },
        },
        config_manager,
        equalizer,
    }
}
