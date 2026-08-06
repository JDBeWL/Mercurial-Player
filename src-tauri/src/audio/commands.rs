//! 音频相关的 Tauri 命令
//!
//! 包含播放控制、设备管理等命令。

use super::device::{AudioDeviceInfo, get_all_audio_devices};
use super::playback::{
    PlaybackStatus, check_track_finished, get_status, play_track_exclusive, play_track_shared,
    seek_track_shared,
};

#[cfg(windows)]
use super::wasapi::WasapiExclusivePlayback;

use crate::AppState;
use cpal::traits::HostTrait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tauri::{AppHandle, State, command};

// ============================================================================
// 共享模式淡入淡出辅助
// ============================================================================

/// 共享模式淡入淡出步数(30ms 总时长 / 3ms 每步 = 10 步)
const FADE_STEPS: u32 = 10;
const FADE_STEP_MS: u64 = 3;

/// 启动共享模式 fade 线程(后台执行,立即返回)
/// 递增代际计数器以取消之前未完成的 fade 线程
/// `direction`: 正数 = 淡入(0→target), 负数 = 淡出(target→0)
/// `on_complete`: fade 完成后执行的闭包(在 fade 线程中调用)
/// 注:on_complete 在持锁状态下执行,且执行前会再次检查代际,
/// 防止 fade-out 的 pause() 在 resume 的 play() 之后执行的竞态
fn spawn_shared_fade(
    sink: Arc<std::sync::Mutex<rodio::Player>>,
    target_volume: f32,
    direction: i32,
    fade_generation: Arc<AtomicU32>,
    on_complete: Box<dyn FnOnce(&rodio::Player) + Send>,
) {
    use std::thread;
    let fade_gen = fade_generation.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        for i in 1..=FADE_STEPS {
            // 检查是否被新的 fade 操作取消
            if fade_generation.load(Ordering::SeqCst) != fade_gen {
                return;
            }
            let progress = i as f32 / FADE_STEPS as f32;
            let vol = if direction >= 0 {
                target_volume * progress
            } else {
                target_volume * (1.0 - progress)
            };
            if let Ok(p) = sink.lock() {
                p.set_volume(vol);
            } else {
                return;
            }
            thread::sleep(Duration::from_millis(FADE_STEP_MS));
        }
        // 持锁 + 代际双重检查,确保 on_complete 不会被取消后的操作覆盖
        if let Ok(p) = sink.lock() {
            if fade_generation.load(Ordering::SeqCst) != fade_gen {
                return;
            }
            on_complete(&p);
        }
    });
}

// ============================================================================
// 播放控制命令
// ============================================================================

#[command]
pub fn get_waveform_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    // 使用 try_lock 避免阻塞主线程
    match state.player.visualization.waveform_data.try_lock() {
        Ok(data) => Ok(data.clone()),
        Err(_) => Ok(Vec::new()), // 锁被占用时返回空数据，避免阻塞
    }
}

#[command]
pub fn get_spectrum_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    // 使用 try_lock 避免阻塞主线程
    match state.player.visualization.spectrum_data.try_lock() {
        Ok(data) => Ok(data.clone()),
        Err(_) => Ok(vec![0.0; 128]), // 锁被占用时返回默认数据，避免阻塞
    }
}

#[command]
pub async fn play_track(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    position: Option<f32>,
) -> Result<(), String> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, position).await
    } else {
        play_track_shared(&app, &state, &path, position)
    }
}

#[command]
pub fn pause_track(state: State<AppState>) -> Result<(), String> {
    // 用 lock() 阻塞等待,避免热切换期间用户操作失败
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
            if let Some(ref wasapi) = *guard {
                // 独占模式:wasapi.pause()/resume() 内部已实现淡入淡出
                // 若 fade 禁用,则使用不带 fade 的方法立即暂停/恢复
                if state.player.fade.enabled.load(Ordering::SeqCst) {
                    wasapi.pause()?;
                } else {
                    wasapi.pause_no_fade()?;
                }
            } else {
                return Err("WASAPI player not initialized".to_string());
            }
        }
        #[cfg(not(windows))]
        {
            return Err("Exclusive mode is only supported on Windows".to_string());
        }
    } else {
        // 共享模式:fade 启用时启动淡出线程,否则直接 pause
        if state.player.fade.enabled.load(Ordering::SeqCst) {
            let target_vol = *state
                .player
                .output
                .target_volume
                .lock()
                .map_err(|e| format!("Failed to acquire target volume lock: {e}"))?;
            spawn_shared_fade(
                Arc::clone(&state.player.output.sink),
                target_vol,
                -1,
                Arc::clone(&state.player.fade.generation),
                Box::new(|p| p.pause()),
            );
        } else {
            // fade 禁用:取消任何残留的 fade 线程,直接 pause
            state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
            let player = state
                .player
                .output
                .sink
                .lock()
                .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
            player.pause();
        }
    }

    Ok(())
}

#[command]
pub fn resume_track(state: State<AppState>) -> Result<(), String> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
            if let Some(ref wasapi) = *guard {
                if state.player.fade.enabled.load(Ordering::SeqCst) {
                    wasapi.resume()?;
                } else {
                    wasapi.resume_no_fade()?;
                }
            } else {
                return Err("WASAPI player not initialized".to_string());
            }
        }
        #[cfg(not(windows))]
        {
            return Err("Exclusive mode is only supported on Windows".to_string());
        }
    } else {
        // 共享模式:fade 启用时先取消正在进行的 fade,再将音量设为 0,立即 play(),然后启动淡入线程
        // fade 禁用时直接 play()(取消残留 fade 线程以防其 pause() 把新播放暂停)
        state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
        let player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        if state.player.fade.enabled.load(Ordering::SeqCst) {
            player.set_volume(0.0);
            player.play();
            drop(player);
            let target_vol = *state
                .player
                .output
                .target_volume
                .lock()
                .map_err(|e| format!("Failed to acquire target volume lock: {e}"))?;
            spawn_shared_fade(
                Arc::clone(&state.player.output.sink),
                target_vol,
                1,
                Arc::clone(&state.player.fade.generation),
                Box::new(|_| {}),
            );
        } else {
            // fade 禁用:恢复目标音量并直接 play
            let target_vol = *state
                .player
                .output
                .target_volume
                .lock()
                .map_err(|e| format!("Failed to acquire target volume lock: {e}"))?;
            player.set_volume(target_vol);
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

    {
        let mut target_vol = state
            .player
            .output
            .target_volume
            .lock()
            .map_err(|e| format!("Failed to acquire target volume lock: {e}"))?;
        *target_vol = volume;
    }
    // 取消任何正在进行的淡入淡出,避免 fade 线程覆盖用户新设置的音量
    state.player.fade.generation.fetch_add(1, Ordering::SeqCst);

    // 用 lock() 阻塞等待:用户拖动音量滑块时不应失败,即使热切换期间也只需等几十毫秒
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
            if let Some(ref wasapi) = *guard {
                wasapi.set_volume(volume)?;
            } else {
                return Err("WASAPI player not initialized".to_string());
            }
        }
        #[cfg(not(windows))]
        {
            return Err("Exclusive mode is only supported on Windows".to_string());
        }
    } else {
        let player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        player.set_volume(volume);
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
pub async fn seek_track(
    app: AppHandle,
    state: State<'_, AppState>,
    time: f32,
) -> Result<(), String> {
    // 用 lock() 阻塞等待:用户拖动进度条时不应失败
    let path = state
        .player
        .track
        .current_path
        .lock()
        .map_err(|e| format!("Failed to acquire current path lock: {e}"))?
        .clone()
        .ok_or("No track currently loaded")?;

    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, Some(time)).await
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
pub async fn set_audio_device(
    app: AppHandle,
    state: State<'_, AppState>,
    device_name: String,
    current_time: Option<f32>,
) -> Result<(), String> {
    log::info!("Attempting to switch to audio device: {device_name}");

    // 用 lock() 阻塞等待:播放期间切换设备不应失败
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;

    let result = if exclusive_mode {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time).await
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
async fn switch_to_wasapi_exclusive(
    app: &AppHandle,
    state: &State<'_, AppState>,
    device_name: &str,
    current_time: Option<f32>,
) -> Result<(), String> {
    log::info!("Switching to WASAPI exclusive mode for device: {device_name}");

    // 1. 先记录当前播放路径 (在停止旧播放器前)
    // 用 lock() 阻塞等待:播放期间解码线程会周期性持有这些锁
    let current_path = {
        let old_player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        old_player.stop();

        state
            .player
            .track
            .current_path
            .lock()
            .map_err(|e| format!("Failed to acquire current path lock: {e}"))?
            .clone()
    };

    // 2. 停止旧的 cpal sink (从共享模式切换过来的场景)
    {
        let player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        player.stop();
        player.clear();
    }

    // 3. 确保旧的 WASAPI 播放器被正确清理
    // 用 lock() 阻塞等待:切换期间解码线程会周期性持有此锁
    {
        let mut old_wasapi = state
            .player
            .output
            .wasapi_player
            .lock()
            .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
        // take() 会获取所有权，drop 会自动清理线程和资源
        let _ = old_wasapi.take();
    }

    let wasapi_playback = WasapiExclusivePlayback::new();

    match wasapi_playback.initialize(Some(device_name)) {
        Ok((sample_rate, channels, actual_device_name)) => {
            log::info!(
                "WASAPI Exclusive initialized: {actual_device_name} @ {sample_rate}Hz, {channels} channels"
            );

            {
                let mut wasapi_guard = state
                    .player
                    .output
                    .wasapi_player
                    .lock()
                    .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
                *wasapi_guard = Some(wasapi_playback);
            }

            {
                let mut device_name_guard = state
                    .player
                    .output
                    .current_device_name
                    .lock()
                    .map_err(|e| format!("Failed to acquire current device name lock: {e}"))?;
                *device_name_guard = device_name.to_string();
            }

            // 4. 恢复播放 (如果有正在播放的曲目)
            // 直接调用 play_track_exclusive,不通过 play_track 派发
            // 因为此时 exclusive_mode 标志尚未更新 (仍为旧值 false),
            // 若用 play_track 会错误路由到 play_track_shared
            if let Some(path) = current_path {
                play_track_exclusive(app, state, &path, current_time).await?;
                // play_track_exclusive 不会读 target_volume,需要手动同步音量
                let vol = *state
                    .player
                    .output
                    .target_volume
                    .lock()
                    .map_err(|e| format!("Failed to acquire target volume lock: {e}"))?;
                let wasapi_guard = state
                    .player
                    .output
                    .wasapi_player
                    .lock()
                    .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
                if let Some(ref wasapi) = *wasapi_guard {
                    let _ = wasapi.set_volume(vol);
                }
            }

            log::info!("Successfully switched to WASAPI exclusive mode");
            Ok(())
        }
        Err(e) => {
            log::error!("Failed to initialize WASAPI exclusive mode: {e}");
            if let Ok(mut exclusive_mode_guard) = state.player.output.exclusive_mode.lock() {
                *exclusive_mode_guard = false;
            }
            Err(format!(
                "Failed to initialize WASAPI exclusive mode: {e}. The device may be in use by another application."
            ))
        }
    }
}

#[cfg(not(windows))]
async fn switch_to_wasapi_exclusive(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
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
    log::info!("Switching to shared mode for device: {device_name}");

    // 1. 先记录当前播放状态和路径 (在停止旧播放器前)
    // 用 lock() 阻塞等待:播放期间解码线程会周期性持有这些锁,try_lock 会失败
    let (is_playing, volume, current_path) = {
        let old_player = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        let playing = !old_player.is_paused();
        let vol = old_player.volume();
        old_player.stop();
        let current_path = state
            .player
            .track
            .current_path
            .lock()
            .map_err(|e| format!("Failed to acquire current path lock: {e}"))?
            .clone();
        (playing, vol, current_path)
    };

    // 2. 先停止并 drop WASAPI 独占模式播放器,释放设备
    // 必须在打开新的 cpal stream 之前完成,否则设备仍被独占模式占用
    // 用 lock() 阻塞等待:解码线程会周期性持有此锁调用 push_samples 等
    {
        let mut wasapi_guard = state
            .player
            .output
            .wasapi_player
            .lock()
            .map_err(|e| format!("Failed to acquire WASAPI player lock: {e}"))?;
        if let Some(wasapi) = wasapi_guard.as_ref() {
            let _ = wasapi.stop();
            let _ = wasapi.clear_buffer();
        }
        // take() 获取所有权,drop 时会 join 音频线程并释放独占设备
        let _ = wasapi_guard.take();
    }

    // 3. 等待设备释放 (WASAPI 独占模式释放有延迟)
    // 短暂 sleep + 重试机制,避免设备未完全释放就尝试打开
    // 注意:output_devices() 返回迭代器,设备只能消费一次,因此每次重试都要重新获取
    let get_device = || -> Result<cpal::Device, String> {
        let host = cpal::default_host();
        host.output_devices()
            .map_err(|e| format!("Failed to get output devices: {e}"))?
            .find(|d| super::device::get_device_friendly_name(d).is_some_and(|n| n == device_name))
            .ok_or_else(|| format!("Audio device not found: {device_name}"))
    };

    // 4. 尝试打开新的 cpal/rodio stream (带重试)
    // WASAPI 独占模式释放后,OS 可能需要短暂时间才让其他程序打开设备
    let new_mixer_sink = {
        let mut last_err: Option<String> = None;
        let mut sink: Option<rodio::MixerDeviceSink> = None;
        for attempt in 1..=5 {
            let dev = match get_device() {
                Ok(d) => d,
                Err(e) => {
                    last_err = Some(e);
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            match rodio::stream::DeviceSinkBuilder::from_device(dev) {
                Ok(builder) => match builder.open_stream() {
                    Ok(s) => {
                        if attempt > 1 {
                            log::info!("cpal stream opened after {attempt} attempts");
                        }
                        sink = Some(s);
                        break;
                    }
                    Err(e) => {
                        last_err = Some(format!(
                            "Failed to create mixer sink: {e}. The device may be in use by another application."
                        ));
                        log::warn!("cpal open_stream attempt {attempt}/5 failed: {e}");
                        std::thread::sleep(Duration::from_millis(50));
                    }
                },
                Err(e) => {
                    last_err = Some(format!("Failed to create device sink builder: {e}"));
                    log::warn!("cpal DeviceSinkBuilder attempt {attempt}/5 failed: {e}");
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        match sink {
            Some(s) => s,
            None => {
                return Err(last_err
                    .unwrap_or_else(|| "Failed to create mixer sink after retries".to_string()));
            }
        }
    };

    let new_player = rodio::Player::connect_new(new_mixer_sink.mixer());

    // 5. 替换播放器
    // 用 lock() 阻塞等待:确保热切换期间能成功替换播放器
    {
        let mut player_guard = state
            .player
            .output
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire player lock: {e}"))?;
        *player_guard = new_player;
        player_guard.set_volume(volume);
        if is_playing {
            player_guard.play();
        } else {
            player_guard.pause();
        }
    }

    {
        let mut output_stream_guard = state
            .player
            .output
            .output_stream
            .lock()
            .map_err(|e| format!("Failed to acquire output stream lock: {e}"))?;
        *output_stream_guard = Some(new_mixer_sink);
    }

    {
        let mut device_name_guard = state
            .player
            .output
            .current_device_name
            .lock()
            .map_err(|e| format!("Failed to acquire current device name lock: {e}"))?;
        *device_name_guard = device_name.to_string();
    }

    if let Some(path) = current_path {
        // 直接调用 play_track_shared,不通过 play_track 派发
        // 因为此时 exclusive_mode 标志尚未更新 (仍为旧值 true),
        // 若用 play_track 会错误路由到 play_track_exclusive,
        // 而 WASAPI 播放器已被 take() 走,导致 "WASAPI player not initialized"
        play_track_shared(app, state, &path, current_time)?;
    }

    log::info!("Successfully switched to shared mode");
    Ok(())
}

#[command]
pub async fn toggle_exclusive_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
    current_time: Option<f32>,
) -> Result<(), String> {
    log::info!("Toggling exclusive mode: {enabled}");

    // 用 lock() 阻塞等待:用户切换独占模式时不应失败,即使播放期间
    let prev_exclusive = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;
    if prev_exclusive == enabled {
        log::info!("Exclusive mode already set to {enabled}, no action needed");
        return Ok(());
    }

    // 获取当前设备名 (用于热切换)
    let device_name = state
        .player
        .output
        .current_device_name
        .lock()
        .map_err(|e| format!("Failed to acquire current device name lock: {e}"))?
        .clone();
    if device_name.is_empty() {
        // 没有当前设备,只能保存配置并要求重启 (首次启动场景)
        if let Ok(mut config) = state.config_manager.load_config() {
            config.audio.exclusive_mode = enabled;
            state.config_manager.save_config(&config)?;
        }
        return Err("RESTART_REQUIRED".to_string());
    }

    // 先保存配置,无论切换成功与否都持久化用户选择
    if let Ok(mut config) = state.config_manager.load_config() {
        config.audio.exclusive_mode = enabled;
        if let Err(e) = state.config_manager.save_config(&config) {
            log::warn!("Failed to save config after toggling exclusive mode: {e}");
        }
    }

    // 热切换到目标模式
    let result = if enabled {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time).await
    } else {
        switch_to_shared_mode(&app, &state, &device_name, current_time)
    };

    match result {
        Ok(()) => {
            // 更新 exclusive_mode 标志 (必须成功,否则状态不一致)
            {
                let mut guard = state
                    .player
                    .output
                    .exclusive_mode
                    .lock()
                    .map_err(|e| format!("Failed to acquire exclusive mode lock: {e}"))?;
                *guard = enabled;
            }
            // 更新设备监听器 (best-effort,失败不影响切换结果)
            if let Ok(monitor) = state.player.device_monitor.try_lock() {
                monitor.update_current_device(device_name);
            }
            log::info!("Successfully hot-switched exclusive mode to {enabled}");
            Ok(())
        }
        Err(e) => {
            // 切换失败,回滚配置
            log::error!("Failed to hot-switch exclusive mode to {enabled}: {e}");
            if let Ok(mut config) = state.config_manager.load_config() {
                config.audio.exclusive_mode = prev_exclusive;
                let _ = state.config_manager.save_config(&config);
            }
            Err(format!(
                "Failed to switch exclusive mode: {e}. The device may be in use by another application."
            ))
        }
    }
}

#[command]
pub fn get_exclusive_mode(state: State<AppState>) -> Result<bool, String> {
    state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())
}

#[command]
pub fn get_current_audio_device(state: State<AppState>) -> Result<AudioDeviceInfo, String> {
    let current_device_name = state
        .player
        .output
        .current_device_name
        .try_lock()
        .map_err(|_| "Failed to acquire current device name lock".to_string())?
        .clone();

    let host = cpal::default_host();
    let default_device_name = host
        .default_output_device()
        .and_then(|d| super::device::get_device_friendly_name(&d));

    let is_default = default_device_name.is_some_and(|d_name| d_name == current_device_name);
    let supports_exclusive_mode = {
        #[cfg(windows)]
        {
            super::wasapi::check_device_exclusive_support(Some(&current_device_name))
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };
    let is_exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    let audio_mode_status = {
        if is_exclusive_mode {
            #[cfg(windows)]
            {
                let wasapi_active = state
                    .player
                    .output
                    .wasapi_player
                    .try_lock()
                    .map(|g| g.is_some())
                    .unwrap_or(false);
                if wasapi_active {
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

// ============================================================================
// 上次播放会话恢复
// ============================================================================

/// 启动时调用,根据配置中的 last_session 校验并恢复播放
#[command]
pub async fn resume_last_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<super::session::ResumeResult, String> {
    super::session::try_resume_last_session(&app, &state).await
}

/// 保存上次播放会话 (前端节流写入调用)
#[command]
pub fn save_last_session(
    state: State<AppState>,
    track_path: String,
    track_title: String,
    track_artist: String,
    duration_secs: f32,
    position_secs: f32,
    playlist_name: Option<String>,
    track_index_in_playlist: Option<usize>,
    playlist_tracks: Vec<crate::config::manager::TrackSnapshot>,
) -> Result<(), String> {
    super::session::save_last_session(
        &state,
        track_path,
        track_title,
        track_artist,
        duration_secs,
        position_secs,
        playlist_name,
        track_index_in_playlist,
        playlist_tracks,
    )
}

/// 清除上次播放会话记录 (用于文件失效场景)
#[command]
pub fn clear_last_session(state: State<AppState>) -> Result<(), String> {
    super::session::clear_last_session(&state)
}

#[command]
pub fn set_target_fps(state: State<AppState>, fps: u32) -> Result<(), String> {
    if fps == 0 {
        return Err("FPS cannot be zero".to_string());
    }
    // 限制最大刷新率为 240fps，防止过高频率
    let clamped_fps = fps.min(240);
    state
        .player
        .visualization
        .target_fps
        .store(clamped_fps as u64, Ordering::Relaxed);
    log::info!("Target FPS set to {clamped_fps}");
    Ok(())
}

#[command]
pub fn set_vertical_sync(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state
        .player
        .visualization
        .enable_vertical_sync
        .store(enabled, Ordering::Relaxed);
    log::info!(
        "Vertical sync {}",
        if enabled { "enabled" } else { "disabled" }
    );
    Ok(())
}

/// 设置是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
/// 立即生效,无需重启
#[command]
pub fn set_fade_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.player.fade.enabled.store(enabled, Ordering::SeqCst);
    // 取消任何正在进行的 fade 线程,防止禁用 fade 后残留线程执行 on_complete
    state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
    log::info!("Fade {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

/// 获取当前是否启用淡入淡出
#[command]
pub fn get_fade_enabled(state: State<AppState>) -> Result<bool, String> {
    Ok(state.player.fade.enabled.load(Ordering::SeqCst))
}
