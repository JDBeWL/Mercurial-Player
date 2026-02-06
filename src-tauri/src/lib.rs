//! Mercurial Player - 库模块
//!
//! 导出所有公共模块和类型。

pub mod audio;
pub mod config;
pub mod equalizer;
pub mod error;
pub mod media;
pub mod plugins;
pub mod system;

#[cfg(windows)]
pub mod taskbar;

use audio::SymphoniaSource;

#[cfg(windows)]
use audio::WasapiExclusivePlayback;

use audio::DeviceMonitor;
use config::ConfigManager;
use equalizer::{Equalizer, GlobalEqualizer};

use rodio::Sink;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

/// 非 Windows 平台的占位类型
#[cfg(not(windows))]
pub struct Placeholder;

/// 播放器状态
///
/// 包含音频播放所需的所有状态信息
pub struct PlayerState {
    /// 音频输出 sink
    pub sink: Arc<Mutex<Sink>>,
    /// 当前音频源
    pub current_source: Arc<Mutex<Option<SymphoniaSource>>>,
    /// 当前播放文件路径
    pub current_path: Arc<Mutex<Option<String>>>,
    /// 目标音量
    pub target_volume: Arc<Mutex<f32>>,
    /// 当前音频设备名称
    pub current_device_name: Arc<Mutex<String>>,
    /// 是否启用独占模式
    pub exclusive_mode: Arc<Mutex<bool>>,
    /// 波形数据（用于可视化）
    pub waveform_data: Arc<Mutex<Vec<f32>>>,
    /// 频谱数据（用于可视化）
    pub spectrum_data: Arc<Mutex<Vec<f32>>>,
    /// WASAPI 独占模式播放器（仅 Windows）
    #[cfg(windows)]
    pub wasapi_player: Arc<Mutex<Option<WasapiExclusivePlayback>>>,
    /// 非 Windows 平台的占位字段
    #[cfg(not(windows))]
    pub wasapi_player: Arc<Mutex<Option<Placeholder>>>,
    /// 解码线程停止标志
    pub decode_thread_stop: Arc<AtomicBool>,
    /// 当前解码线程 ID（用于区分不同的播放会话）
    pub decode_thread_id: Arc<AtomicU64>,
    /// EQ 均衡器
    pub equalizer: Arc<Mutex<Equalizer>>,
    /// 设备监听器
    pub device_monitor: Arc<Mutex<DeviceMonitor>>,
}

/// 应用程序状态
///
/// 包含整个应用程序的全局状态
pub struct AppState {
    /// 播放器状态
    pub player: PlayerState,
    /// 配置管理器
    pub config_manager: ConfigManager,
    /// 全局均衡器
    pub equalizer: GlobalEqualizer,
}

// 重新导出常用类型
pub use audio::{AudioDeviceInfo, PlaybackStatus, SymphoniaDecoder};
#[cfg(windows)]
pub use audio::PlaybackState;
pub use config::AppConfig;
pub use equalizer::EqSettings;
pub use media::{Playlist, TrackMetadata};
