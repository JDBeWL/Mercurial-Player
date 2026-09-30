//! 音频相关的 Tauri 命令：播放控制、设备管理、独占模式切换。
//!
//! 约定：命令层一律用 `lock()` 阻塞等待而不是 `try_lock()`——播放期间解码线程会周期性
//! 持锁，try_lock 必然偶发失败，而用户操作不该因此报错。各调用点的注释只写具体理由。
use crate::error::AppError;

use super::device::{AudioDeviceInfo, get_all_audio_devices};
use super::playback::{play_track_exclusive, play_track_shared, seek_track_shared};
use super::queue::{self, RepeatMode};

#[cfg(windows)]
use super::wasapi::WasapiExclusivePlayback;

#[cfg(any(windows, target_os = "android"))]
use super::PlaybackState;

use crate::AppState;
use crate::config::manager::TrackSnapshot;

use super::LockOrErr;
use cpal::traits::HostTrait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tauri::{AppHandle, State, command};

// 共享模式淡入淡出辅助

/// 共享模式淡入淡出步数(30ms 总时长 / 3ms 每步 = 10 步)
const FADE_STEPS: u32 = 10;
const FADE_STEP_MS: u64 = 3;

/// 启动共享模式 fade 线程(后台执行,立即返回)，并递增代际以取消之前未完成的 fade 线程。
/// `direction` 正数淡入、负数淡出；`on_complete` 在持锁状态下执行且执行前再查一次代际，
/// 防止 fade-out 的 pause() 落在 resume 的 play() 之后。
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

// 播放控制命令

#[command]
pub async fn play_track(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    position: Option<f32>,
) -> Result<(), AppError> {
    // 移动端：按用户偏好对齐本首的输出模式（"下一首生效"在这里落地）
    #[cfg(target_os = "android")]
    sync_exclusive_mode_from_config(&state);

    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, position, true).await
    } else {
        play_track_shared(&app, &state, &path, position)
    }?;
    queue::note_playback_started(&app, &state, &path, position);
    Ok(())
}

/// 共享模式暂停（供 `pause_track` 命令与媒体控制入口复用，避免两处行为漂移）
pub fn pause_playback(state: &AppState) -> Result<(), AppError> {
    // 共享模式:fade 启用时启动淡出线程,否则直接 pause
    if state.player.fade.enabled.load(Ordering::SeqCst) {
        let target_vol = *state
            .player
            .output
            .target_volume
            .lock()
            .lock_or_err("target volume")?;
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
        let player = state.player.output.sink.lock().lock_or_err("player")?;
        player.pause();
    }

    Ok(())
}

/// 暂停当前播放，按 exclusive_mode 自动分流。`pause_track` 命令与启动恢复路径共用，
/// 避免两处行为漂移（曾经只有 Windows 分支，Android 独占模式暂停会失败）。
pub fn pause_any(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    // 避免热切换期间用户操作失败
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        #[cfg(any(windows, target_os = "android"))]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .lock_or_err("WASAPI player")?;
            if let Some(ref wasapi) = *guard {
                // 独占播放器的 pause()/resume() 内部已实现淡入淡出；fade 禁用时用不带 fade 的方法
                if state.player.fade.enabled.load(Ordering::SeqCst) {
                    wasapi.pause()?;
                } else {
                    wasapi.pause_no_fade()?;
                }
            } else {
                return Err(AppError::Audio("WASAPI player not initialized".to_string()));
            }
            drop(guard);
            // 独占分支也要刷新通知栏/MediaSession，否则系统侧一直以为还在播放
            queue::sync_media_session(app, state);
            return Ok(());
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            return Err(AppError::Audio(
                "Exclusive mode is only supported on Windows / Android".to_string(),
            ));
        }
    }
    pause_playback(state)?;
    queue::sync_media_session(app, state);
    Ok(())
}

#[command]
pub fn pause_track(app: AppHandle, state: State<AppState>) -> Result<(), AppError> {
    pause_any(&app, &state)
}

#[command]
pub fn resume_track(app: AppHandle, state: State<AppState>) -> Result<(), AppError> {
    resume_any(&app, &state)
}

/// 恢复播放，按 exclusive_mode 自动分流。`resume_track` 命令与媒体控制入口共用，
/// 否则通知栏/耳机按键在独占模式下只会驱动共享 sink（表现为按了没反应）。
pub fn resume_any(app: &AppHandle, state: &AppState) -> Result<(), AppError> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        #[cfg(any(windows, target_os = "android"))]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .lock_or_err("WASAPI player")?;
            if let Some(ref wasapi) = *guard {
                if state.player.fade.enabled.load(Ordering::SeqCst) {
                    wasapi.resume()?;
                } else {
                    wasapi.resume_no_fade()?;
                }
            } else {
                return Err(AppError::Audio("WASAPI player not initialized".to_string()));
            }
            drop(guard);
            queue::sync_media_session(app, state);
            return Ok(());
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            return Err(AppError::Audio(
                "Exclusive mode is only supported on Windows / Android".to_string(),
            ));
        }
    }
    resume_playback(state)?;
    queue::sync_media_session(app, state);
    Ok(())
}

/// 输出端当前是否真的在出声：独占模式看独占播放器状态，共享模式看 rodio sink 暂停位。
/// 前端与 `playback-state-sync` 需要的是"实际有没有声音"，不是共享 sink 的暂停位。
#[must_use]
pub fn is_output_playing(state: &AppState) -> bool {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .map(|g| *g)
        .unwrap_or(false);

    #[cfg(any(windows, target_os = "android"))]
    if exclusive_mode {
        return state
            .player
            .output
            .wasapi_player
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|p| p.state() == PlaybackState::Playing))
            .unwrap_or(false);
    }
    #[cfg(not(any(windows, target_os = "android")))]
    let _ = exclusive_mode;

    state
        .player
        .output
        .sink
        .lock()
        .map(|sink| !sink.is_paused())
        .unwrap_or(false)
}

/// 回收独占输出播放器：停流、清缓冲、丢弃实例（没有则什么都不做）。
///
/// 独占流会占住 USB DAC，移动端"下一首生效"把它留到下一首之前，所以走共享播放的
/// 入口必须先调它，否则旧流一直握着设备。
pub(crate) fn release_exclusive_player(state: &AppState) {
    let taken = lock_or_log!(state.player.output.wasapi_player.lock()).take();
    if let Some(player) = taken {
        let _ = player.stop();
        let _ = player.clear_buffer();
    }
}

/// 移动端"下一首生效"：在开始播放一首新曲目之前，把实际输出模式对齐到用户偏好。
///
/// `exclusive_mode` 的语义是"这一首正在用的输出"，配置里的 `usb_dac_exclusive` 才是
/// 用户想要的模式；两者只在切歌时对齐。切开关当刻不改标志，pause/resume/seek 才不会
/// 把命令发给另一条根本没在出声的链路。
#[cfg(target_os = "android")]
pub(crate) fn sync_exclusive_mode_from_config(state: &AppState) {
    let wanted = state
        .config_manager
        .load_config()
        .map(|c| c.audio.usb_dac_exclusive && super::aaudio::usb_dac_available().unwrap_or(false))
        .unwrap_or(false);
    *lock_or_log!(state.player.output.exclusive_mode.lock()) = wanted;
}

/// 共享模式恢复播放（供 `resume_track` 命令与媒体控制入口复用）
pub fn resume_playback(state: &AppState) -> Result<(), AppError> {
    // 共享模式:fade 启用时先取消正在进行的 fade,再将音量设为 0,立即 play(),然后启动淡入线程
    // fade 禁用时直接 play()(取消残留 fade 线程以防其 pause() 把新播放暂停)
    state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
    let player = state.player.output.sink.lock().lock_or_err("player")?;
    if state.player.fade.enabled.load(Ordering::SeqCst) {
        player.set_volume(0.0);
        player.play();
        drop(player);
        let target_vol = *state
            .player
            .output
            .target_volume
            .lock()
            .lock_or_err("target volume")?;
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
            .lock_or_err("target volume")?;
        player.set_volume(target_vol);
        player.play();
        drop(player);
    }

    Ok(())
}

#[command]
pub fn set_volume(state: State<AppState>, volume: f32) -> Result<(), AppError> {
    if !(0.0..=1.0).contains(&volume) {
        return Err(AppError::Audio(
            "Volume must be between 0.0 and 1.0".to_string(),
        ));
    }

    {
        let mut target_vol = state
            .player
            .output
            .target_volume
            .lock()
            .lock_or_err("target volume")?;
        *target_vol = volume;
    }
    // 取消任何正在进行的淡入淡出,避免 fade 线程覆盖用户新设置的音量
    state.player.fade.generation.fetch_add(1, Ordering::SeqCst);

    // 用户拖动音量滑块时不应失败，热切换期间也只需等几十毫秒
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        #[cfg(any(windows, target_os = "android"))]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .lock()
                .lock_or_err("WASAPI player")?;
            if let Some(ref wasapi) = *guard {
                wasapi.set_volume(volume)?;
            } else {
                return Err(AppError::Audio("WASAPI player not initialized".to_string()));
            }
            drop(guard);
            return Ok(());
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            return Err(AppError::Audio(
                "Exclusive mode is only supported on Windows / Android".to_string(),
            ));
        }
    }
    let player = state.player.output.sink.lock().lock_or_err("player")?;
    player.set_volume(volume);
    drop(player);

    Ok(())
}

#[command]
pub async fn seek_track(
    app: AppHandle,
    state: State<'_, AppState>,
    time: f32,
) -> Result<(), AppError> {
    // 用户拖动进度条时不应失败
    let path = state
        .player
        .track
        .current_path
        .lock()
        .lock_or_err("current path")?
        .clone()
        .ok_or("No track currently loaded")?;

    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, Some(time), true).await
    } else {
        seek_track_shared(&app, &state, &path, time)
    }
}

// 设备管理命令

#[command]
pub fn get_audio_devices() -> Result<Vec<AudioDeviceInfo>, AppError> {
    get_all_audio_devices()
}

/// 切换音频设备(携带当前播放进度,便于后端无缝续播)。
/// `remember` 控制是否把设备落盘到 `audio.preferredDeviceId`：只有设置页的主动选择传 true，
/// 设备拔出自动回退 / 跟随系统默认传 false，避免覆盖用户的固定选择；缺省按 true。
#[command]
pub async fn set_audio_device(
    app: AppHandle,
    state: State<'_, AppState>,
    device_name: String,
    current_time: Option<f32>,
    remember: Option<bool>,
) -> Result<(), AppError> {
    log::info!("Attempting to switch to audio device: {device_name}");

    // 播放期间切换设备不应失败
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    let result = if exclusive_mode {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time).await
    } else {
        switch_to_shared_mode(&app, &state, &device_name, current_time).await
    };

    // 切换成功后:更新设备监听器,并把设备标识落盘(仅用户主动选择;
    // 解析不到标识则跳过——不影响本次切换,只是下次启动不记忆)
    if result.is_ok() {
        if remember.unwrap_or(true) {
            // cpal 枚举会触碰平台音频 API(Windows 上还要初始化 COM),
            // 放到阻塞线程池执行,避免卡住异步命令线程
            let dev_name = device_name.clone();
            let device_id = tauri::async_runtime::spawn_blocking(move || {
                super::device::name_to_device_id(&dev_name)
            })
            .await
            .ok()
            .flatten();

            match device_id {
                Some(id) => {
                    if let Err(e) = state.config_manager.update_config(|config| {
                        config.audio.preferred_device_id = Some(id.clone());
                    }) {
                        log::warn!("保存首选输出设备失败，下次启动不会自动选中它: {e}");
                    } else {
                        log::info!("Remembered preferred audio device: {id}");
                    }
                }
                None => {
                    log::debug!(
                        "No stable device id resolvable for '{device_name}', skipping device memory"
                    );
                }
            }
        }

        // 监听线程正在枚举设备时不阻塞本次切换：跳过只让它记住的"当前设备"暂时不准，
        // 下轮枚举会纠正，但必须在日志里留痕，不能静默
        if let Ok(monitor) = state.player.device_monitor.try_lock() {
            monitor.update_current_device(device_name);
        } else {
            log::warn!("设备监听器正忙,本次未更新当前设备({device_name})");
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
) -> Result<(), AppError> {
    log::info!("Switching to WASAPI exclusive mode for device: {device_name}");

    // 停止并清理旧的 cpal sink,同时记录当前播放路径与是否在播放
    // 解码线程会周期性持有这些锁
    let (is_playing, current_path) = {
        let is_playing = {
            let old_player = state.player.output.sink.lock().lock_or_err("player")?;
            let playing = !old_player.is_paused();
            old_player.stop();
            old_player.clear();
            playing
        }; // 先释放 sink 锁,再取 current_path,避免嵌套持锁
        let current_path = state
            .player
            .track
            .current_path
            .lock()
            .lock_or_err("current path")?
            .clone();
        (is_playing, current_path)
    };

    // 确保旧的 WASAPI 播放器被正确清理
    // 切换期间解码线程会周期性持有此锁
    {
        let mut old_wasapi = state
            .player
            .output
            .wasapi_player
            .lock()
            .lock_or_err("WASAPI player")?;
        // take() 会获取所有权，drop 会自动清理线程和资源
        let _ = old_wasapi.take();
    }

    // WASAPI 独占模式初始化是阻塞系统调用(内部含重试 sleep 与 COM 操作),
    // 放到阻塞线程池执行,避免阻塞 async runtime 线程
    let dev_name = device_name.to_string();
    let (wasapi_playback, init_result) = tauri::async_runtime::spawn_blocking(move || {
        let playback = WasapiExclusivePlayback::new();
        let result = playback.initialize(Some(&dev_name));
        (playback, result)
    })
    .await
    .map_err(|e| AppError::msg(format!("WASAPI 初始化任务执行失败: {e}")))?;

    match init_result {
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
                    .lock_or_err("WASAPI player")?;
                *wasapi_guard = Some(wasapi_playback);
            }

            {
                let mut device_name_guard = state
                    .player
                    .output
                    .current_device_name
                    .lock()
                    .lock_or_err("current device name")?;
                *device_name_guard = device_name.to_string();
            }

            // 绕过 play_track 直接调用：exclusive_mode 此刻还是切换前的值（本函数由开关
            // 路径调用时仍是 false），走派发会被路由到共享模式。切换前若为暂停则传
            // start_playback=false。
            if let Some(path) = current_path {
                play_track_exclusive(app, state, &path, current_time, is_playing).await?;
                // play_track_exclusive 不会读 target_volume,需要手动同步音量
                let vol = *state
                    .player
                    .output
                    .target_volume
                    .lock()
                    .lock_or_err("target volume")?;
                let wasapi_guard = state
                    .player
                    .output
                    .wasapi_player
                    .lock()
                    .lock_or_err("WASAPI player")?;
                if let Some(ref wasapi) = *wasapi_guard {
                    if let Err(e) = wasapi.set_volume(vol) {
                        log::warn!("独占模式音量未送达渲染线程，可能仍以原增益输出: {e}");
                    }
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
            ).into())
        }
    }
}

#[cfg(not(windows))]
#[allow(clippy::unused_async)] // 必须 async 以与 Windows 版本签名一致(调用方使用 .await)
async fn switch_to_wasapi_exclusive(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _device_name: &str,
    _current_time: Option<f32>,
) -> Result<(), AppError> {
    Err(AppError::Audio(
        "Exclusive mode is only supported on Windows / Android".to_string(),
    ))
}

async fn switch_to_shared_mode(
    app: &AppHandle,
    state: &State<'_, AppState>,
    device_name: &str,
    current_time: Option<f32>,
) -> Result<(), AppError> {
    log::info!("Switching to shared mode for device: {device_name}");

    // 先记录当前播放状态和路径（在停止旧播放器前）
    // 解码线程会周期性持有这些锁，try_lock 必失败
    let (is_playing, volume, current_path) = {
        // 独占模式下真正在播的是独占播放器（Windows=WASAPI，Android=AAudio），
        // cpal sink 在切独占时已被 stop/clear，它的 is_paused() 不代表当前状态。
        // 因此独占播放器存在时以它的 state() 为准，只有纯共享模式换设备才回退 sink。
        let wasapi_playing = {
            #[cfg(not(any(windows, target_os = "android")))]
            {
                None
            }
            #[cfg(any(windows, target_os = "android"))]
            {
                let g = state
                    .player
                    .output
                    .wasapi_player
                    .lock()
                    .lock_or_err("exclusive player")?;
                g.as_ref().map(|p| p.state() == PlaybackState::Playing)
            }
        };
        let (sink_playing, vol) = {
            let old_player = state.player.output.sink.lock().lock_or_err("player")?;
            let playing = !old_player.is_paused();
            let vol = old_player.volume();
            old_player.stop();
            drop(old_player);
            (playing, vol)
        }; // 先释放 sink 锁,再取 current_path,避免嵌套持锁
        let playing = wasapi_playing.unwrap_or(sink_playing);
        let current_path = state
            .player
            .track
            .current_path
            .lock()
            .lock_or_err("current path")?
            .clone();
        (playing, vol, current_path)
    };

    // 先停止并 drop 独占播放器,释放设备：必须在打开新的 cpal stream 之前完成，否则设备仍被占用
    // 解码线程只在取写入端句柄时短暂碰这把锁（背压等待已不持锁），所以这里不会被拖住
    {
        let mut wasapi_guard = state
            .player
            .output
            .wasapi_player
            .lock()
            .lock_or_err("WASAPI player")?;
        if let Some(wasapi) = wasapi_guard.as_ref() {
            let _ = wasapi.stop();
            let _ = wasapi.clear_buffer();
        }
        // take() 获取所有权,drop 时会 join 音频线程并释放独占设备
        let _ = wasapi_guard.take();
    }

    // 打开新的 cpal/rodio stream (带重试,等待独占模式释放设备)：枚举与建流是阻塞系统调用，
    // 放阻塞线程池执行；output_devices() 的迭代器只能消费一次，故每次重试都重新获取
    let new_mixer_sink = {
        let mut last_err: Option<AppError> = None;
        let mut sink: Option<rodio::MixerDeviceSink> = None;
        for attempt in 1..=5 {
            let dev_name = device_name.to_string();
            let open_result = tauri::async_runtime::spawn_blocking(
                move || -> Result<rodio::MixerDeviceSink, AppError> {
                    let host = cpal::default_host();
                    let dev = host
                        .output_devices()
                        .map_err(|e| format!("Failed to get output devices: {e}"))?
                        .find(|d| {
                            super::device::get_device_friendly_name(d).is_some_and(|n| n == dev_name)
                        })
                        .ok_or_else(|| format!("Audio device not found: {dev_name}"))?;
                    let builder = rodio::stream::DeviceSinkBuilder::from_device(dev)
                        .map_err(|e| format!("Failed to create device sink builder: {e}"))?;
                    Ok(builder.open_stream().map_err(|e| {
                        format!(
                            "Failed to create mixer sink: {e}. The device may be in use by another application."
                        )
                    })?)
                },
            )
            .await
            .map_err(|e| AppError::msg(format!("cpal stream 创建任务执行失败: {e}")))?;

            match open_result {
                Ok(s) => {
                    if attempt > 1 {
                        log::info!("cpal stream opened after {attempt} attempts");
                    }
                    sink = Some(s);
                    break;
                }
                Err(e) => {
                    log::warn!("cpal open_stream attempt {attempt}/5 failed: {e}");
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
        match sink {
            Some(s) => s,
            None => {
                return Err(last_err.unwrap_or_else(|| {
                    AppError::msg("Failed to create mixer sink after retries")
                }));
            }
        }
    };

    let new_player = rodio::Player::connect_new(new_mixer_sink.mixer());

    // 替换播放器
    // 热切换期间必须成功替换播放器
    {
        let mut player_guard = state.player.output.sink.lock().lock_or_err("player")?;
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
            .lock_or_err("output stream")?;
        *output_stream_guard = Some(new_mixer_sink);
    }

    {
        let mut device_name_guard = state
            .player
            .output
            .current_device_name
            .lock()
            .lock_or_err("current device name")?;
        *device_name_guard = device_name.to_string();
    }

    if let Some(path) = current_path {
        // 绕过 play_track 直接调用：exclusive_mode 此刻仍是切换前的值，派发会走到已被
        // take() 走的独占播放器上
        play_track_shared(app, state, &path, current_time)?;
        // play_track_shared 末尾总是 play();若切换前为暂停状态需重新暂停,
        // 保持暂停,点击恢复时才从该位置继续播放(音源 fade_in 从 0 开始,不会爆音)
        if !is_playing {
            state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
            let player_lock = state.player.output.sink.lock().lock_or_err("player")?;
            player_lock.pause();
        }
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
) -> Result<(), AppError> {
    log::info!("Toggling exclusive mode: {enabled}");

    // 用户切换独占模式时不应失败，即使正在播放
    let prev_exclusive = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;
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
        .lock_or_err("current device name")?
        .clone();
    if device_name.is_empty() {
        // 没有当前设备,只能保存配置并要求重启 (首次启动场景)
        state.config_manager.update_config(|config| {
            config.audio.exclusive_mode = enabled;
        })?;
        return Err("RESTART_REQUIRED".to_string().into());
    }

    // 先保存配置,无论切换成功与否都持久化用户选择
    if let Err(e) = state.config_manager.update_config(|config| {
        config.audio.exclusive_mode = enabled;
    }) {
        log::warn!("Failed to save config after toggling exclusive mode: {e}");
    }

    // 热切换到目标模式
    let result = if enabled {
        switch_to_wasapi_exclusive(&app, &state, &device_name, current_time).await
    } else {
        switch_to_shared_mode(&app, &state, &device_name, current_time).await
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
                    .lock_or_err("exclusive mode")?;
                *guard = enabled;
            }
            // 更新设备监听器 (监听线程枚举中时跳过,不影响切换结果;跳过要留痕)
            if let Ok(monitor) = state.player.device_monitor.try_lock() {
                monitor.update_current_device(device_name);
            } else {
                log::warn!("设备监听器正忙,独占切换后未同步当前设备");
            }
            log::info!("Successfully hot-switched exclusive mode to {enabled}");
            Ok(())
        }
        Err(e) => {
            // 切换失败,回滚配置
            log::error!("Failed to hot-switch exclusive mode to {enabled}: {e}");
            // 回滚:独立临界区 (上方的 .await 已完成,锁不跨 await 持有)
            if let Err(rollback_err) = state.config_manager.update_config(|config| {
                config.audio.exclusive_mode = prev_exclusive;
            }) {
                log::error!(
                    "回滚独占模式配置失败，磁盘配置可能与实际输出模式不一致（下次启动仍按独占打开）: {rollback_err}"
                );
            }
            Err(format!(
                "Failed to switch exclusive mode: {e}. The device may be in use by another application."
            ).into())
        }
    }
}

#[command]
pub fn get_exclusive_mode(state: State<AppState>) -> Result<bool, AppError> {
    state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)
        .map_err(AppError::from)
}

#[command]
#[allow(clippy::branches_sharing_code)] // 非 Windows 下 if/else 均返回 "standard",但 Windows 下有不同分支
// Android 的独占生效与否不由这里上报，走 get_audio_route
pub fn get_current_audio_device(state: State<AppState>) -> Result<AudioDeviceInfo, AppError> {
    let current_device_name = state
        .player
        .output
        .current_device_name
        .lock()
        .lock_or_err("current device name")?
        .clone();

    let host = cpal::default_host();
    let default_device_name = host
        .default_output_device()
        .and_then(|d| super::device::get_device_friendly_name(&d));

    let is_default = default_device_name.is_some_and(|d_name| d_name == current_device_name);
    let supports_exclusive_mode = {
        #[cfg(windows)]
        {
            // 探测失败就如实报错，不要退化成"这台设备不支持独占"
            super::wasapi::check_device_exclusive_support(Some(&current_device_name))?
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
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    let audio_mode_status = if is_exclusive_mode {
        #[cfg(windows)]
        {
            let wasapi_active = state
                .player
                .output
                .wasapi_player
                .lock()
                .lock_or_err("WASAPI player")?
                .is_some();
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
    .to_string();

    Ok(AudioDeviceInfo {
        name: current_device_name,
        is_default,
        supports_exclusive_mode,
        is_exclusive_mode,
        audio_mode_status,
    })
}

// 上次播放会话恢复

/// 启动时调用,根据配置中的 last_session 校验并恢复播放
#[command]
pub async fn resume_last_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<super::session::ResumeResult, AppError> {
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
    playlist_tracks: Vec<TrackSnapshot>,
) -> Result<(), AppError> {
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
pub fn clear_last_session(state: State<AppState>) -> Result<(), AppError> {
    super::session::clear_last_session(&state)
}

/// 目标帧率上限（防止过高频率的 FFT 计算）
pub const MAX_TARGET_FPS: u32 = 240;

#[command]
pub fn set_target_fps(state: State<AppState>, fps: u32) -> Result<(), AppError> {
    if fps == 0 {
        return Err("FPS cannot be zero".to_string().into());
    }
    let clamped_fps = fps.min(MAX_TARGET_FPS);
    state
        .player
        .visualization
        .target_fps
        .store(clamped_fps as u64, Ordering::Relaxed);
    log::info!("Target FPS set to {clamped_fps}");
    Ok(())
}

/// 设置是否启用淡入淡出(切歌平滑过渡 + pause/resume 消除爆音)
/// 立即生效,无需重启
#[command]
pub fn set_fade_enabled(state: State<AppState>, enabled: bool) -> Result<(), AppError> {
    state.player.fade.enabled.store(enabled, Ordering::SeqCst);
    // 取消任何正在进行的 fade 线程,防止禁用 fade 后残留线程执行 on_complete
    state.player.fade.generation.fetch_add(1, Ordering::SeqCst);
    log::info!("Fade {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

/// 获取当前是否启用淡入淡出
#[command]
pub fn get_fade_enabled(state: State<AppState>) -> Result<bool, AppError> {
    Ok(state.player.fade.enabled.load(Ordering::SeqCst))
}

// 播放队列与媒体控制

/// 同步播放队列到 Rust 侧（前端在列表 / 随机序 / 循环模式变化时调用）。
/// `tracks` 必须**已按最终播放顺序排列**（随机序由前端算好），Rust 不实现 shuffle。
/// `auto_advance` 仅在 Android 传 true（曲目自然结束由 Rust 推进），桌面端保持 false。
#[command]
pub fn set_play_queue(
    state: State<AppState>,
    tracks: Vec<TrackSnapshot>,
    current_index: Option<usize>,
    repeat_mode: Option<String>,
    auto_advance: Option<bool>,
) -> Result<(), AppError> {
    let repeat = RepeatMode::parse(repeat_mode.as_deref().unwrap_or("off"));
    {
        let mut queue = state.player.queue.lock().lock_or_err("playback queue")?;
        queue.set_queue(tracks, current_index, repeat, auto_advance.unwrap_or(false));
    }
    Ok(())
}

/// 媒体控制统一入口（通知栏 / MediaSession / 耳机线控 / 蓝牙按键）。
///
/// `action`: `play` / `pause` / `toggle` / `stop` / `next` / `previous` / `seek`，`position` 仅 seek 需要
#[command]
pub fn media_control(
    app: AppHandle,
    state: State<AppState>,
    action: String,
    position: Option<f32>,
) -> Result<(), AppError> {
    queue::media_control(&app, &state, &action, position)
}

/// 后台心跳探针：前端按固定间隔调用，`adb logcat | grep background-heartbeat` 可观察
/// 进入后台后 WebView 的 JS 是否被节流/冻结。无副作用，桌面端同样可用。
#[command]
pub fn background_heartbeat(seq: u64) -> Result<(), AppError> {
    log::info!("background-heartbeat seq={seq}");
    Ok(())
}

// Android：USB DAC 独占（位完美）输出

/// 当前输出路由快照（Android）：设置页据此展示当前设备、独占是否真的生效、实际输出采样率
#[cfg(target_os = "android")]
#[command]
pub fn get_audio_route(state: State<AppState>) -> Result<serde_json::Value, AppError> {
    let enabled = state.config_manager.load_config()?.audio.usb_dac_exclusive;
    let guard = state
        .player
        .output
        .wasapi_player
        .lock()
        .lock_or_err("exclusive player")?;
    let info = super::aaudio::audio_route_info(enabled, guard.as_ref());
    drop(guard);
    serde_json::to_value(info).map_err(|e| AppError::msg(format!("序列化输出路由失败: {e}")))
}

#[cfg(not(target_os = "android"))]
#[command]
pub fn get_audio_route() -> Result<serde_json::Value, AppError> {
    Err(AppError::Audio(
        "USB DAC 独占输出仅在 Android 上提供".to_string(),
    ))
}

/// 开关 USB DAC 独占（位完美）输出（Android）。
///
/// 移动端按"下一首生效"：这里只记模式，不在当前曲目中途交接输出链路 —— 热切换要么两条
/// 链路同时出声，要么新流没启动就静音，还会让前端播放态与实际输出脱节。
/// 打开时独占流由 `play_track_exclusive` 在下一首开始前按需建立；关闭时若当前没在出声
/// 就立即回收（独占流会占住 USB DAC），正在播则留给下一首走共享前回收。
#[cfg(target_os = "android")]
#[command]
pub fn set_usb_dac_exclusive(state: State<'_, AppState>, enabled: bool) -> Result<(), AppError> {
    // 必须在改标志之前取：is_output_playing 按"当前"模式决定读哪条链路
    let was_playing = is_output_playing(&state);

    // 幂等判断按"用户偏好"而不是当前输出标志：后者在切歌前会刻意滞后于偏好
    let prev_preference = state
        .config_manager
        .load_config()
        .map(|c| c.audio.usb_dac_exclusive)
        .unwrap_or(false);
    if prev_preference == enabled {
        return Ok(());
    }

    // `exclusive_mode` 表示"当前这一首正在用的输出"，不是用户想要的模式：有曲目在
    // 加载/播放时不能改它，否则 pause/resume/seek 会发给一条根本没在出声的链路
    // （表现为刚开独占就 resume_track → "播放器未初始化"）。它由
    // `sync_exclusive_mode_from_config` 在下一首开始时对齐。
    let has_track = state
        .player
        .track
        .current_path
        .lock()
        .lock_or_err("current path")
        .map(|p| p.is_some())
        .unwrap_or(false);
    if !has_track {
        *state
            .player
            .output
            .exclusive_mode
            .lock()
            .lock_or_err("exclusive mode")? = enabled;
    }

    state.config_manager.update_config(|config| {
        config.audio.usb_dac_exclusive = enabled;
    })?;

    if !enabled && !was_playing {
        release_exclusive_player(&state);
    }

    log::info!("USB DAC 独占输出已设为 {enabled}（下一首生效）");
    Ok(())
}

#[cfg(not(target_os = "android"))]
#[command]
pub fn set_usb_dac_exclusive(_state: State<'_, AppState>, _enabled: bool) -> Result<(), AppError> {
    Err(AppError::Audio(
        "USB DAC 独占输出仅在 Android 上提供".to_string(),
    ))
}
