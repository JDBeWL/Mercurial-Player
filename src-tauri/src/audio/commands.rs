//! 音频相关的 Tauri 命令
//!
//! 包含播放控制、设备管理等命令。

use super::device::{get_all_audio_devices, AudioDeviceInfo};
use super::playback::{
    check_track_finished, get_status, play_track_exclusive, play_track_shared, seek_track_shared,
    PlaybackStatus,
};

#[cfg(windows)]
use super::wasapi::WasapiExclusivePlayback;

use crate::AppState;
use cpal::traits::{DeviceTrait, HostTrait};
use tauri::{command, AppHandle, State};

// ============================================================================
// 播放控制命令
// ============================================================================

#[command]
pub fn get_waveform_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    // 使用 try_lock 避免阻塞主线程
    match state.player.waveform_data.try_lock() {
        Ok(data) => Ok(data.clone()),
        Err(_) => Ok(Vec::new()) // 锁被占用时返回空数据，避免阻塞
    }
}

#[command]
pub fn get_spectrum_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    // 使用 try_lock 避免阻塞主线程
    match state.player.spectrum_data.try_lock() {
        Ok(data) => Ok(data.clone()),
        Err(_) => Ok(vec![0.0; 128]) // 锁被占用时返回默认数据，避免阻塞
    }
}

#[command]
pub fn play_track(app: AppHandle, state: State<AppState>, path: String, position: Option<f32>) -> Result<(), String> {
    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, position)
    } else {
        play_track_shared(&app, &state, &path, position)
    }
}

#[command]
pub fn pause_track(state: State<AppState>) -> Result<(), String> {
    // 使用 try_lock 避免阻塞
    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .unwrap_or(false);
    
    if exclusive_mode {
        #[cfg(windows)]
        {
            if let Ok(guard) = state.player.wasapi_player.try_lock() {
                if let Some(ref wasapi) = *guard {
                    wasapi.pause()?;
                }
            }
        }
    } else {
        if let Ok(player) = state.player.sink.try_lock() {
            player.pause();
        }
    }
    Ok(())
}

#[command]
pub fn resume_track(state: State<AppState>) -> Result<(), String> {
    // 使用 try_lock 避免阻塞
    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .unwrap_or(false);
    
    if exclusive_mode {
        #[cfg(windows)]
        {
            if let Ok(guard) = state.player.wasapi_player.try_lock() {
                if let Some(ref wasapi) = *guard {
                    wasapi.resume()?;
                }
            }
        }
    } else {
        if let Ok(player) = state.player.sink.try_lock() {
            player.play();
        }
    }
    Ok(())
}

#[command]
pub fn set_volume(state: State<AppState>, volume: f32) -> Result<(), String> {
    if !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between 0.0 and 1.0".to_string());
    }
    
    // 使用 try_lock 避免阻塞
    if let Ok(mut target_vol) = state.player.target_volume.try_lock() {
        *target_vol = volume;
    }
    
    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .unwrap_or(false);
    
    if exclusive_mode {
        #[cfg(windows)]
        {
            if let Ok(guard) = state.player.wasapi_player.try_lock() {
                if let Some(ref wasapi) = *guard {
                    wasapi.set_volume(volume)?;
                }
            }
        }
    } else {
        if let Ok(player) = state.player.sink.try_lock() {
            player.set_volume(volume);
        }
    }
    Ok(())
}

#[command]
pub fn get_playback_status(state: State<AppState>) -> Result<PlaybackStatus, String> {
    get_status(&state)
}

#[command]
pub fn is_track_finished(state: State<AppState>) -> Result<bool, String> {
    check_track_finished(&state)
}

#[command]
pub fn seek_track(app: AppHandle, state: State<AppState>, time: f32) -> Result<(), String> {
    let path = state.player.current_path.try_lock()
        .map_err(|_| "Failed to acquire current path lock".to_string())?
        .clone()
        .ok_or("No track currently loaded")?;

    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, Some(time))
    } else {
        seek_track_shared(&app, &state, &path, time)
    }
}

// ============================================================================
// 设备管理命令
// ============================================================================

#[command]
pub fn get_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    get_all_audio_devices()
}

#[command]
pub fn set_audio_device(
    app: AppHandle,
    state: State<AppState>,
    device_name: String,
    current_time: Option<f32>,
) -> Result<(), String> {
    println!("Attempting to switch to audio device: {device_name}");

    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    let result = if exclusive_mode {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time)
    } else {
        switch_to_shared_mode(&app, &state, &device_name, current_time)
    };

    // 如果切换成功，更新设备监听器
    if result.is_ok() {
        if let Ok(monitor) = state.player.device_monitor.try_lock() {
            monitor.update_current_device(device_name);
        }
    }

    result
}

#[cfg(windows)]
fn switch_to_wasapi_exclusive(
    _app: &AppHandle,
    state: &State<AppState>,
    device_name: &str,
    _current_time: Option<f32>,
) -> Result<(), String> {
    println!("Switching to WASAPI exclusive mode for device: {device_name}");

    {
        if let Ok(player) = state.player.sink.try_lock() {
            player.stop();
            player.clear();
        }
    }

    // 确保旧的 WASAPI 播放器被正确清理
    {
        if let Ok(mut old_wasapi) = state.player.wasapi_player.try_lock() {
            // take() 会获取所有权，drop 会自动清理线程和资源
            let _ = old_wasapi.take();
        }
    }

    let wasapi_playback = WasapiExclusivePlayback::new();

    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_device_name)) => {
            println!("WASAPI Exclusive initialized: {actual_device_name} @ {sample_rate}Hz, {channels} channels");

            if let Ok(mut wasapi_guard) = state.player.wasapi_player.try_lock() {
                *wasapi_guard = Some(wasapi_playback);
            } else {
                return Err("Failed to acquire WASAPI player lock".to_string());
            }

            if let Ok(mut device_name_guard) = state.player.current_device_name.try_lock() {
                *device_name_guard = device_name.to_string();
            } else {
                return Err("Failed to acquire current device name lock".to_string());
            }

            println!("Successfully switched to WASAPI exclusive mode");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to initialize WASAPI exclusive mode: {e}");
            if let Ok(mut exclusive_mode_guard) = state.player.exclusive_mode.try_lock() {
                *exclusive_mode_guard = false;
            }
            Err(format!("Failed to initialize WASAPI exclusive mode: {e}. The device may be in use by another application."))
        }
    }
}

#[cfg(not(windows))]
fn switch_to_wasapi_exclusive(
    _app: &AppHandle,
    _state: &State<AppState>,
    _device_name: &str,
    _current_time: Option<f32>,
) -> Result<(), String> {
    Err("Exclusive mode is only supported on Windows".to_string())
}

fn switch_to_shared_mode(
    app: &AppHandle,
    state: &State<AppState>,
    device_name: &str,
    current_time: Option<f32>,
) -> Result<(), String> {
    println!("Switching to shared mode for device: {device_name}");

    let host = cpal::default_host();
    let device = host
        .output_devices()
        .map_err(|e| format!("Failed to get output devices: {e}"))?
        .find(|d| d.description().ok().map(|desc| desc.name() == device_name).unwrap_or(false))
        .ok_or(format!("Audio device not found: {device_name}"))?;

    let new_mixer_sink = rodio::stream::DeviceSinkBuilder::from_device(device)
        .map_err(|e| format!("Failed to create device sink builder: {e}"))?
        .open_stream()
        .map_err(|e| format!("Failed to create mixer sink: {e}"))?;

    let new_player = rodio::Player::connect_new(new_mixer_sink.mixer());

    let (is_playing, volume, current_path) = {
        let old_player = state.player.sink.try_lock()
            .map_err(|_| "Failed to acquire player lock".to_string())?;
        let playing = !old_player.is_paused();
        let vol = old_player.volume();
        old_player.stop();
        let current_path = state.player.current_path.try_lock()
            .map_err(|_| "Failed to acquire current path lock".to_string())?
            .clone();
        (playing, vol, current_path)
    };

    {
        let mut wasapi_guard = state.player.wasapi_player.try_lock()
            .map_err(|_| "Failed to acquire WASAPI player lock".to_string())?;
        *wasapi_guard = None;
    }

    {
        let mut player_guard = state.player.sink.try_lock()
            .map_err(|_| "Failed to acquire player lock".to_string())?;
        *player_guard = new_player;
        player_guard.set_volume(volume);
        if is_playing {
            player_guard.play();
        } else {
            player_guard.pause();
        }
    }

    {
        let mut output_stream_guard = state.player.output_stream.try_lock()
            .map_err(|_| "Failed to acquire output stream lock".to_string())?;
        *output_stream_guard = Some(new_mixer_sink);
    }

    {
        let mut device_name_guard = state.player.current_device_name.try_lock()
            .map_err(|_| "Failed to acquire current device name lock".to_string())?;
        *device_name_guard = device_name.to_string();
    }

    if let Some(path) = current_path {
        play_track(app.clone(), state.clone(), path, current_time)?;
    }

    println!("Successfully switched to shared mode");
    Ok(())
}

#[command]
pub fn toggle_exclusive_mode(
    _app: AppHandle,
    state: State<AppState>,
    enabled: bool,
    _current_time: Option<f32>,
) -> Result<(), String> {
    println!("Toggling exclusive mode: {enabled} (requires restart)");

    let prev_exclusive = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;
    if prev_exclusive == enabled {
        println!("Exclusive mode already set to {enabled}, no action needed");
        return Ok(());
    }

    if let Ok(mut config) = state.config_manager.load_config() {
        config.audio.exclusive_mode = enabled;
        state.config_manager.save_config(&config)?;
    }

    Err("RESTART_REQUIRED".to_string())
}

#[command]
pub fn get_exclusive_mode(state: State<AppState>) -> Result<bool, String> {
    state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())
}

#[command]
pub fn get_current_audio_device(state: State<AppState>) -> Result<AudioDeviceInfo, String> {
    let current_device_name = state.player.current_device_name.try_lock()
        .map_err(|_| "Failed to acquire current device name lock".to_string())?
        .clone();

    let host = cpal::default_host();
    let default_device_name = host.default_output_device().and_then(|d| d.description().ok().map(|desc| desc.name().to_string()));

    let is_default = default_device_name.is_some_and(|d_name| d_name == current_device_name);
    let supports_exclusive_mode = {
        #[cfg(windows)]
        {
            super::wasapi::check_device_exclusive_support(Some(&current_device_name)).unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    let is_exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    let audio_mode_status = {
        if is_exclusive_mode {
            #[cfg(windows)]
            {
                let wasapi_active = state.player.wasapi_player.try_lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false);
                if wasapi_active { "exclusive" } else { "standard" }
            }
            #[cfg(not(windows))]
            {
                "standard"
            }
        } else {
            "standard"
        }
    }
    .to_string();

    Ok(AudioDeviceInfo {
        name: current_device_name,
        is_default,
        supports_exclusive_mode,
        is_exclusive_mode,
        audio_mode_status,
    })
}

#[command]
pub fn set_target_fps(state: State<AppState>, fps: u32) -> Result<(), String> {
    if fps == 0 {
        return Err("FPS cannot be zero".to_string());
    }
    // 限制最大刷新率为 240fps，防止过高频率
    let clamped_fps = fps.min(240);
    state.player.target_fps.store(clamped_fps as u64, std::sync::atomic::Ordering::Relaxed);
    println!("Target FPS set to {}", clamped_fps);
    Ok(())
}

#[command]
pub fn set_vertical_sync(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.player.enable_vertical_sync.store(enabled, std::sync::atomic::Ordering::Relaxed);
    println!("Vertical sync {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}
