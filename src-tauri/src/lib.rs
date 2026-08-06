//! Mercurial Player - 库模块
//!
//! 导出所有公共模块和类型。

/// 获取锁, poison 错误时记录日志并返回内部数据(而非 panic)
///
/// 支持 `Mutex::lock()`、`RwLock::read()`、`RwLock::write()`，
/// 三者均返回 `Result<T, PoisonError<T>>`。
macro_rules! lock_or_log {
    ($lock:expr) => {
        match $lock {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("锁中毒, 自动恢复: {}", poisoned);
                poisoned.into_inner()
            }
        }
    };
}
pub(crate) use lock_or_log;

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

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

/// 非 Windows 平台的占位类型
#[cfg(not(windows))]
#[derive(Debug)]
pub struct Placeholder;

#[cfg(not(windows))]
impl Placeholder {
    /// 占位方法,非 Windows 平台调用 WASAPI 方法时返回错误
    pub fn stop(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn clear_buffer(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause_no_fade(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume_no_fade(&self) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn stop_with_fade_out(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn pause_with_fade_out(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }

    pub fn resume_with_fade_in(&self, _duration_ms: u32) -> Result<(), String> {
        Err("WASAPI not available on non-Windows".to_string())
    }
}

/// 音频输出相关状态
///
/// 包含音频 sink、输出流、音量、设备、独占模式等与音频输出直接相关的字段
pub struct AudioOutputState {
    /// 音频输出 sink
    pub sink: Arc<Mutex<rodio::Player>>,
    /// 音频输出流（必须长期持有，否则会静音）
    pub output_stream: Arc<Mutex<Option<rodio::MixerDeviceSink>>>,
    /// 目标音量
    pub target_volume: Arc<Mutex<f32>>,
    /// 当前音频设备名称
    pub current_device_name: Arc<Mutex<String>>,
    /// 是否启用独占模式
    pub exclusive_mode: Arc<Mutex<bool>>,
    /// WASAPI 独占模式播放器（仅 Windows）
    #[cfg(windows)]
    pub wasapi_player: Arc<Mutex<Option<WasapiExclusivePlayback>>>,
    /// 非 Windows 平台的占位字段
    #[cfg(not(windows))]
    pub wasapi_player: Arc<Mutex<Option<Placeholder>>>,
}

/// 当前播放曲目状态
pub struct TrackState {
    /// 当前音频源
    pub current_source: Arc<Mutex<Option<SymphoniaSource>>>,
    /// 当前播放文件路径
    pub current_path: Arc<Mutex<Option<String>>>,
}

/// 可视化相关状态
///
/// 波形/频谱数据 + FFT 计算参数
pub struct VisualizationState {
    /// 波形数据（用于可视化）
    pub waveform_data: Arc<Mutex<Vec<f32>>>,
    /// 频谱数据（用于可视化）
    pub spectrum_data: Arc<Mutex<Vec<f32>>>,
    /// 目标刷新率（用于可视化FFT计算，默认60fps）
    pub target_fps: Arc<AtomicU64>,
    /// 是否启用垂直同步（启用后FFT频率与屏幕刷新率同步）
    pub enable_vertical_sync: Arc<AtomicBool>,
}

/// 解码线程管理
pub struct DecodeThreadState {
    /// 解码线程代际计数器(每次切歌递增,旧线程检测到变化即退出)
    pub generation: Arc<AtomicU64>,
    /// 当前解码线程 ID（用于区分不同的播放会话）
    pub id: Arc<AtomicU64>,
}

/// 淡入淡出控制
///
/// generation 用于取消陈旧的 fade 线程;
/// enabled 控制是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
pub struct FadeControl {
    /// 共享模式淡入淡出代际计数器(每次新的 fade 操作递增,用于取消陈旧的 fade 线程)
    pub generation: Arc<AtomicU32>,
    /// 是否启用淡入淡出(运行时读取,避免每次访问配置文件)
    pub enabled: Arc<AtomicBool>,
}

/// 播放器状态
///
/// 按职责域拆分为子结构体,每个子结构体负责一组内聚的字段
pub struct PlayerState {
    /// 音频输出 (sink/流/音量/设备/独占模式/WASAPI)
    pub output: AudioOutputState,
    /// 当前曲目 (source/path)
    pub track: TrackState,
    /// 可视化 (波形/频谱/FFT 参数)
    pub visualization: VisualizationState,
    /// 解码线程管理
    pub decode: DecodeThreadState,
    /// EQ 均衡器
    pub equalizer: Arc<Mutex<Equalizer>>,
    /// 设备监听器
    pub device_monitor: Arc<Mutex<DeviceMonitor>>,
    /// 淡入淡出控制
    pub fade: FadeControl,
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
#[cfg(windows)]
pub use audio::PlaybackState;
pub use audio::{AudioDeviceInfo, PlaybackStatus, SymphoniaDecoder};
pub use config::AppConfig;
pub use equalizer::EqSettings;
pub use media::{Playlist, TrackMetadata};
