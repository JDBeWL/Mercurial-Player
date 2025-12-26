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
use rodio::{OutputStream, Sink};
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
    if *state.player.exclusive_mode.lock().unwrap() {
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
        if let Ok(sink) = state.player.sink.try_lock() {
            sink.pause();
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
        if let Ok(sink) = state.player.sink.try_lock() {
            sink.play();
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
        if let Ok(sink) = state.player.sink.try_lock() {
            sink.set_volume(volume);
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
    let path = state.player.current_path.lock().unwrap().clone().ok_or("No track currently loaded")?;
    if *state.player.exclusive_mode.lock().unwrap() {
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

    let exclusive_mode = *state.player.exclusive_mode.lock().unwrap();

    if exclusive_mode {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time)
    } else {
        switch_to_shared_mode(&app, &state, &device_name, current_time)
    }
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
        let sink = state.player.sink.lock().unwrap();
        sink.stop();
        sink.clear();
    }

    // 确保旧的 WASAPI 播放器被正确清理
    {
        let mut old_wasapi = state.player.wasapi_player.lock().unwrap();
        // take() 会获取所有权，drop 会自动清理线程和资源
        let _ = old_wasapi.take();
    }

    let wasapi_playback = WasapiExclusivePlayback::new();

    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_device_name)) => {
            println!("WASAPI Exclusive initialized: {actual_device_name} @ {sample_rate}Hz, {channels} channels");

            *state.player.wasapi_player.lock().unwrap() = Some(wasapi_playback);
            *state.player.current_device_name.lock().unwrap() = device_name.to_string();

            println!("Successfully switched to WASAPI exclusive mode");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to initialize WASAPI exclusive mode: {e}");
            *state.player.exclusive_mode.lock().unwrap() = false;
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
        .find(|d| d.name().is_ok_and(|name| name == device_name))
        .ok_or(format!("Audio device not found: {device_name}"))?;

    let (_stream, stream_handle) =
        OutputStream::try_from_device(&device).map_err(|e| format!("Failed to create output stream: {e}"))?;

    Box::leak(Box::new(_stream));

    let new_sink = Sink::try_new(&stream_handle).map_err(|e| format!("Failed to create new sink: {e}"))?;

    let (is_playing, volume, current_path) = {
        let old_sink = state.player.sink.lock().unwrap();
        let playing = !old_sink.is_paused();
        let vol = old_sink.volume();
        old_sink.stop();
        (playing, vol, state.player.current_path.lock().unwrap().clone())
    };

    *state.player.wasapi_player.lock().unwrap() = None;
    *state.player.sink.lock().unwrap() = new_sink;

    {
        let sink_guard = state.player.sink.lock().unwrap();
        sink_guard.set_volume(volume);
        if is_playing {
            sink_guard.play();
        } else {
            sink_guard.pause();
        }
    }

    *state.player.current_device_name.lock().unwrap() = device_name.to_string();

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

    let prev_exclusive = *state.player.exclusive_mode.lock().unwrap();
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
    Ok(*state.player.exclusive_mode.lock().unwrap())
}

#[command]
pub fn get_current_audio_device(state: State<AppState>) -> Result<AudioDeviceInfo, String> {
    let current_device_name = state.player.current_device_name.lock().unwrap().clone();

    let host = cpal::default_host();
    let default_device_name = host.default_output_device().and_then(|d| d.name().ok());

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
    let is_exclusive_mode = *state.player.exclusive_mode.lock().unwrap();

    let audio_mode_status = {
        if is_exclusive_mode {
            #[cfg(windows)]
            {
                if state.player.wasapi_player.lock().unwrap().is_some() {
                    "exclusive"
                } else {
                    "standard"
                }
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
