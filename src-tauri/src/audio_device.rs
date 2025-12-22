//! 音频设备管理相关的 Tauri 命令
//!
//! 这个模块包含所有与音频设备管理相关的功能，包括获取可用设备列表和切换输出设备

use super::AppState;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::StreamConfig;
use rodio::{OutputStream, Sink};
use tauri::{command, State, AppHandle};

/// 表示音频设备信息
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    /// 设备名称
    pub name: String,
    /// 是否为默认设备
    pub is_default: bool,
    /// 是否支持独占模式
    pub supports_exclusive_mode: bool,
    /// 当前使用独占模式
    pub is_exclusive_mode: bool,
    /// 当前音频模式状态
    pub audio_mode_status: String,
}

/// 获取所有可用的音频输出设备
#[command]
pub fn get_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    let devices = host.output_devices().map_err(|e| e.to_string())?;
    let mut device_infos: Vec<AudioDeviceInfo> = Vec::new();

    for device in devices {
        if let Ok(name) = device.name() {
            let is_default = default_device_name.as_ref().map_or(false, |d_name| *d_name == name);
            
            // 使用 WASAPI 检测设备是否支持独占模式
            let supports_exclusive_mode = check_wasapi_exclusive_support(&name);
            
            device_infos.push(AudioDeviceInfo { 
                name, 
                is_default,
                supports_exclusive_mode,
                is_exclusive_mode: false,
                audio_mode_status: "standard".to_string(),
            });
        }
    }

    Ok(device_infos)
}

/// 使用 WASAPI 检查设备是否支持独占模式
fn check_wasapi_exclusive_support(device_name: &str) -> bool {
    match crate::wasapi_player::check_device_exclusive_support(Some(device_name)) {
        Ok(supported) => supported,
        Err(e) => {
            println!("Failed to check exclusive mode support for {}: {}", device_name, e);
            false
        }
    }
}

/// 切换音频输出设备
#[command]
pub fn set_audio_device(app: AppHandle, state: State<AppState>, device_name: String, current_time: Option<f32>) -> Result<(), String> {
    println!("Attempting to switch to audio device: {}", device_name);

    // 检查是否启用独占模式
    let exclusive_mode = *state.player.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        // 使用 WASAPI 独占模式
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time)
    } else {
        // 使用标准 rodio/cpal 共享模式
        switch_to_shared_mode(&app, &state, &device_name, current_time)
    }
}

/// 切换到 WASAPI 独占模式
fn switch_to_wasapi_exclusive(
    _app: &AppHandle,
    state: &State<AppState>,
    device_name: &str,
    _current_time: Option<f32>,
) -> Result<(), String> {
    println!("Switching to WASAPI exclusive mode for device: {}", device_name);
    
    // 停止当前播放并清空 sink
    {
        let sink = state.player.sink.lock().unwrap();
        sink.stop();
        sink.clear();
    }
    
    // 给系统一点时间释放设备
    std::thread::sleep(std::time::Duration::from_millis(200));
    
    // 创建 WASAPI 独占模式播放器
    let wasapi_playback = crate::wasapi_exclusive::WasapiExclusivePlayback::new();
    
    // 初始化设备
    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_device_name)) => {
            println!(
                "WASAPI Exclusive initialized: {} @ {}Hz, {} channels",
                actual_device_name, sample_rate, channels
            );
            
            // 存储 WASAPI 播放器
            {
                let mut wasapi_guard = state.player.wasapi_player.lock().unwrap();
                *wasapi_guard = Some(wasapi_playback);
            }
            
            // 更新当前设备名称
            *state.player.current_device_name.lock().unwrap() = device_name.to_string();
            
            println!("Successfully switched to WASAPI exclusive mode");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to initialize WASAPI exclusive mode: {}", e);
            // 如果初始化失败，回滚独占模式标志
            *state.player.exclusive_mode.lock().unwrap() = false;
            Err(format!("Failed to initialize WASAPI exclusive mode: {}. The device may be in use by another application. Try closing other audio applications or restart this app.", e))
        }
    }
}

/// 切换到共享模式
fn switch_to_shared_mode(
    app: &AppHandle,
    state: &State<AppState>,
    device_name: &str,
    current_time: Option<f32>,
) -> Result<(), String> {
    println!("Switching to shared mode for device: {}", device_name);
    
    let host = cpal::default_host();
    let device = host
        .output_devices()
        .map_err(|e| format!("Failed to get output devices: {e}"))?
        .find(|d| d.name().map_or(false, |name| name == device_name))
        .ok_or(format!("Audio device not found: {}", device_name))?;

    // 创建标准的 rodio 输出流
    let (_stream, stream_handle) = OutputStream::try_from_device(&device)
        .map_err(|e| format!("Failed to create output stream: {e}"))?;

    // 泄露新的流以保持其存活
    Box::leak(Box::new(_stream));

    // 创建新的sink
    let new_sink = Sink::try_new(&stream_handle).map_err(|e| format!("Failed to create new sink: {e}"))?;

    // 从旧的sink获取播放状态
    let is_playing;
    let volume;
    let current_path;
    {
        let old_sink = state.player.sink.lock().unwrap();
        is_playing = !old_sink.is_paused();
        volume = old_sink.volume();
        old_sink.stop();
    }
    
    current_path = state.player.current_path.lock().unwrap().clone();

    // 清除 WASAPI 播放器（如果有）
    {
        let mut wasapi_guard = state.player.wasapi_player.lock().unwrap();
        *wasapi_guard = None;
    }

    // 将新sink应用到播放器状态
    {
        let mut sink_guard = state.player.sink.lock().unwrap();
        *sink_guard = new_sink;
    }

    // 恢复音量和播放状态
    {
        let sink_guard = state.player.sink.lock().unwrap();
        sink_guard.set_volume(volume);
        if is_playing {
            sink_guard.play();
        } else {
            sink_guard.pause();
        }
    }

    // 更新当前设备名称
    *state.player.current_device_name.lock().unwrap() = device_name.to_string();
    
    // 如果有当前播放的音轨，则重新播放
    if let Some(path) = current_path {
        super::playback::play_track(app.clone(), state.clone(), path, current_time)?;
    }

    println!("Successfully switched to shared mode");
    Ok(())
}

/// 切换独占模式
#[command]
pub fn toggle_exclusive_mode(_app: AppHandle, state: State<AppState>, enabled: bool, _current_time: Option<f32>) -> Result<(), String> {
    println!("Toggling exclusive mode: {} (requires restart)", enabled);
    
    // 检查之前的独占模式状态
    let prev_exclusive = *state.player.exclusive_mode.lock().unwrap();
    if prev_exclusive == enabled {
        println!("Exclusive mode already set to {}, no action needed", enabled);
        return Ok(());
    }
    
    // 由于 WASAPI 独占模式需要在应用启动时初始化，
    // 我们只更新配置，用户需要重启应用程序才能生效
    
    // 更新配置文件中的独占模式设置
    let config_result = state.config_manager.load_config();
    if let Ok(mut config) = config_result {
        config.audio.exclusive_mode = enabled;
        if let Err(e) = state.config_manager.save_config(&config) {
            return Err(format!("Failed to save config: {}", e));
        }
    }
    
    // 返回需要重启的提示
    // 注意：我们不更新 exclusive_mode 状态，因为实际模式没有改变
    // 前端应该显示一个提示，告诉用户需要重启应用程序
    Err("RESTART_REQUIRED".to_string())
}

/// 获取独占模式状态
#[command]
pub fn get_exclusive_mode(state: State<AppState>) -> Result<bool, String> {
    Ok(*state.player.exclusive_mode.lock().unwrap())
}

/// 获取当前音频设备信息
#[command]
pub fn get_current_audio_device(state: State<AppState>) -> Result<AudioDeviceInfo, String> {
    let current_device_name = state.player.current_device_name.lock().unwrap().clone();

    let host = cpal::default_host();
    let default_device_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());
    
    let is_default = default_device_name.map_or(false, |d_name| d_name == current_device_name);
    let supports_exclusive_mode = check_wasapi_exclusive_support(&current_device_name);
    let is_exclusive_mode = *state.player.exclusive_mode.lock().unwrap();
    
    // 确定当前音频模式状态
    let audio_mode_status = if is_exclusive_mode {
        // 检查是否真正在独占模式下运行
        let wasapi_guard = state.player.wasapi_player.lock().unwrap();
        if wasapi_guard.is_some() {
            "exclusive".to_string()
        } else {
            "standard".to_string()
        }
    } else {
        "standard".to_string()
    };

    Ok(AudioDeviceInfo {
        name: current_device_name,
        is_default,
        supports_exclusive_mode,
        is_exclusive_mode,
        audio_mode_status,
    })
}

/// 检测设备是否支持独占模式（使用 cpal）
#[allow(dead_code)]
pub fn check_exclusive_mode_support(device: &cpal::Device) -> bool {
    // 获取设备的默认输出配置
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(_) => return false,
    };

    // 创建输出流配置
    let mut stream_config: StreamConfig = config.into();
    stream_config.buffer_size = cpal::BufferSize::Fixed(256);

    let err_fn = |_err| {};
    let data_fn = move |_data: &mut [f32], _: &cpal::OutputCallbackInfo| {};

    match device.build_output_stream(&stream_config, data_fn, err_fn, None) {
        Ok(stream) => {
            drop(stream);
            true
        }
        Err(_) => false,
    }
}
