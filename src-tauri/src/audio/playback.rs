//! 音频播放模块：共享模式（rodio sink）的播放与 seek，独占模式的解码线程派发与预缓冲等待。
//!
//! 采样级的 EQ/频谱处理不在这里：音源适配器见 [`super::spectrum::VisualizationSource`]，
//! EQ 见 [`super::eq_processor`]，事件发送见 [`super::emit`]。

#[cfg(any(windows, target_os = "android"))]
use super::decode_push::decode_and_push_to_wasapi;
use super::decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
use super::spectrum::VisualizationSource;
use crate::error::AppError;

use super::{FADE_IN_MS, FADE_IN_ON_SEEK_MS};

use crate::AppState;
use rodio::Source;
use std::io::BufReader;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, State};

/// 播放音轨（共享模式）
pub fn play_track_shared(
    app: &AppHandle,
    state: &AppState,
    path: &str,
    position: Option<f32>,
) -> Result<(), AppError> {
    let player = &state.player;
    // 取消任何正在进行的淡入淡出,防止其 on_complete(pause) 在新歌播放后执行
    player.fade.generation.fetch_add(1, Ordering::SeqCst);
    // 上一首可能还挂在独占输出上（移动端"下一首生效"留下的旧流）：走共享前回收，
    // 否则它会一直占着 USB DAC
    super::commands::release_exclusive_player(state);
    // 先读取 target_volume 再锁 sink,避免嵌套锁死锁风险
    let vol = *lock_or_log!(player.output.target_volume.lock());
    {
        let player_lock = lock_or_log!(player.output.sink.lock());
        // 直接停止，不做淡出（淡出会阻塞主线程）
        // 新音源会有fade_in效果来平滑过渡
        player_lock.stop();
        player_lock.set_volume(vol);
    }
    *lock_or_log!(player.track.current_path.lock()) = Some(path.to_string());
    let (spectrum, eq_settings, target_fps) = (
        Arc::clone(&player.visualization.spectrum_data),
        state.equalizer.get_settings_handle(),
        Arc::clone(&player.visualization.target_fps),
    );

    let source: Box<dyn Source<Item = f32> + Send> = match SymphoniaDecoder::new(path) {
        Ok(mut dec) => {
            let start_pos = position.unwrap_or(0.0);
            if let Some(t) = position {
                if let Err(e) = dec.seek(Duration::from_secs_f32(t)) {
                    log::warn!("Seek failed for track, starting from beginning: {e}");
                }
            }
            let _ = dec.prefill_buffer();
            log::debug!("Symphonia decoder: {path}");
            Box::new(
                VisualizationSource::new(
                    LockFreeSymphoniaSource::new(dec),
                    spectrum,
                    Some(app.clone()),
                    target_fps,
                )
                .with_start_position(start_pos)
                .with_eq_settings(eq_settings)
                .fade_in(Duration::from_millis(FADE_IN_MS)), // 稍长的淡入来补偿没有淡出
            )
        }
        Err(e) => {
            log::warn!("Symphonia decoder failed, fallback to rodio: {e}");
            let file = crate::android::saf::open_media_file(path)?;
            Box::new(
                VisualizationSource::new(
                    rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?,
                    spectrum,
                    Some(app.clone()),
                    target_fps,
                )
                .with_start_position(position.unwrap_or(0.0))
                .with_eq_settings(eq_settings)
                .fade_in(Duration::from_millis(FADE_IN_MS)),
            )
        }
    };

    // 手动重采样到 mixer 的采样率：rodio 0.22 的 UniformSourceIterator 在 queue keep_alive 模式下，
    // source.current_span_len() 返回 None 时不会重新 bootstrap SampleRateConverter，高采样率音轨
    // 会被以错误的速率播放（降速）；因此在 append 之前先手动重采样。
    let resampled: Box<dyn Source<Item = f32> + Send> = {
        let stream_guard = lock_or_log!(player.output.output_stream.lock());
        if let Some(ref mixer_sink) = *stream_guard {
            let mixer_sr = mixer_sink.config().sample_rate();
            let mixer_ch = mixer_sink.config().channel_count();
            #[cfg(debug_assertions)]
            let source_sr = source.sample_rate();
            #[cfg(debug_assertions)]
            println!(
                "Source sample_rate: {source_sr}, Mixer sample_rate: {mixer_sr}, Mixer channels: {mixer_ch}"
            );
            Box::new(rodio::source::UniformSourceIterator::new(
                source, mixer_ch, mixer_sr,
            ))
        } else {
            log::warn!("No output stream available, skipping manual resample");
            source
        }
    };

    let player_lock = lock_or_log!(player.output.sink.lock());
    player_lock.append(resampled);
    player_lock.play();
    drop(player_lock);
    Ok(())
}

/// 播放音轨（独占模式），Windows（WASAPI）与 Android（AAudio）共用，差异只在 ensure_format。
///
/// 异步：淡出/解码线程启动/缓冲填充的等待用 `tokio::time::sleep`，否则阻塞 Tauri 命令线程（最坏 ~600ms）。
/// `start_playback=false` 时只加载并预缓冲、不 start 流，用于热切换后保持暂停，resume 时从缓冲继续。
#[cfg(any(windows, target_os = "android"))]
pub async fn play_track_exclusive(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: &str,
    position: Option<f32>,
    start_playback: bool,
) -> Result<(), AppError> {
    let player = &state.player;
    // 递增代际计数器取消旧解码推送线程（布尔标志会有约 70ms 的状态不一致窗口）
    player.decode.generation.fetch_add(1, Ordering::SeqCst);
    let new_thread_id = player.decode.id.fetch_add(1, Ordering::SeqCst) + 1;
    // 独占输出接管前必须停掉共享 sink：在播放中把输出切到独占（设置页开关）时共享链路
    // 还在播同一首，两条解码同时出声，听感就是两遍重叠
    lock_or_log!(player.output.sink.lock()).stop();
    // 移动端"下一首生效"：切开关时不预建流，到这里才按需创建。提前建流会占住 DAC，
    // 让切换前已经在播的共享输出没声
    #[cfg(target_os = "android")]
    if lock_or_log!(player.output.wasapi_player.lock()).is_none() {
        let created = crate::audio::aaudio::AaudioExclusivePlayer::new();
        created.initialize(None)?;
        *lock_or_log!(player.output.wasapi_player.lock()) = Some(created);
    }
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player.lock()) {
            // 切歌淡出 50ms，音频线程淡出完成后自行 stop_stream + clear_buffer；
            // 关闭淡入淡出时直接 stop + clear_buffer
            if player.fade.enabled.load(Ordering::SeqCst) {
                let _ = wasapi.stop_with_fade_out(50);
            } else {
                let _ = wasapi.stop();
                let _ = wasapi.clear_buffer();
            }
        }
    }
    // fade 启用时等待淡出完成(50ms) + 旧解码线程退出(20ms buffer)
    // fade 禁用时只等旧解码线程退出
    let wait_ms = if player.fade.enabled.load(Ordering::SeqCst) {
        70
    } else {
        50
    };
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    // 兜底:确保缓冲区被清空(防止淡出未完成的极端情况)
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player.lock()) {
            let _ = wasapi.clear_buffer();
        }
    }

    let (target_sr, target_ch) = {
        let g = lock_or_log!(player.output.wasapi_player.lock());
        let wasapi = g.as_ref().ok_or("WASAPI player not initialized")?;
        let sr_ch = (wasapi.sample_rate(), wasapi.channels());
        drop(g);
        sr_ch
    };
    if position.is_none() {
        *lock_or_log!(player.track.current_path.lock()) = Some(path.to_string());
    }
    log::info!("WASAPI Exclusive: {path} @ {target_sr}Hz, {target_ch} ch");

    let mut decoder =
        SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    if let Some(t) = position {
        if let Err(e) = decoder.seek(Duration::from_secs_f32(t)) {
            log::warn!("Seek failed for track, starting from beginning: {e}");
        }
    }
    let _ = decoder.prefill_buffer();
    let (src_sr, src_ch) = (decoder.sample_rate(), decoder.channels());

    // Android：AAudio 独占流不接受任意采样率，必须按曲目原生速率重建一次。
    // 设备支持原生速率时 = 位完美直出；不支持时退到最接近的一档（解码线程重采样）。
    // Windows 不需要：WASAPI 独占的格式在 initialize 时已与设备协商好。
    #[cfg(target_os = "android")]
    let (target_sr, target_ch) = {
        let (rate, channels) = {
            let guard = lock_or_log!(player.output.wasapi_player.lock());
            let player_ref = guard.as_ref().ok_or("AAudio player not initialized")?;
            let formatted = player_ref
                .ensure_format(src_sr, src_ch.get())
                .map_err(|e| format!("Failed to align AAudio format: {e}"))?;
            drop(guard);
            formatted
        };
        log::info!(
            "AAudio Exclusive: {path} 原生 {src_sr}Hz/{src_ch}ch -> 输出 {rate}Hz/{channels}ch"
        );
        (rate, channels)
    };

    log::debug!("Source: {src_sr}Hz, {src_ch} ch -> Target: {target_sr}Hz, {target_ch} ch");

    let source = LockFreeSymphoniaSource::new(decoder);
    let start_pos = position.unwrap_or(0.0);
    let (wasapi_clone, generation, thread_id, eq_settings, spectrum_data, target_fps) = (
        Arc::clone(&player.output.wasapi_player),
        Arc::clone(&player.decode.generation),
        Arc::clone(&player.decode.id),
        state.equalizer.get_settings_handle(),
        Arc::clone(&player.visualization.spectrum_data),
        Arc::clone(&player.visualization.target_fps),
    );
    let app_clone = app.clone();
    let thread_started = Arc::new(AtomicBool::new(false));
    let thread_started_clone = Arc::clone(&thread_started);

    std::thread::spawn(move || {
        thread_started_clone.store(true, Ordering::SeqCst);
        // 不包 catch_unwind：release 构建是 panic="abort"，接不住；debug 下接住也只会
        // 把解码线程的 bug 变成"这首歌静音了"，现场什么都不留
        decode_and_push_to_wasapi(
            source,
            wasapi_clone,
            app_clone,
            generation,
            thread_id,
            new_thread_id,
            src_sr,
            src_ch.get(),
            target_sr,
            target_ch,
            eq_settings,
            spectrum_data,
            target_fps,
            start_pos,
        );
    });

    // 等待解码线程启动
    let mut wait = 0;
    while !thread_started.load(Ordering::SeqCst) && wait < 20 {
        tokio::time::sleep(Duration::from_millis(5)).await;
        wait += 1;
    }

    // 等待缓冲区有足够数据再开始播放，避免音频开头欠载
    // 注意:不能在持有 wasapi_player 锁守卫的情况下 await(守卫非 Send),
    // 因此每次检查后立即释放锁再 sleep。
    {
        // 等待至少200ms的音频数据（约 1/5 秒）
        let min_buffer_samples = target_sr as usize * target_ch as usize / 5;
        let mut buffer_wait = 0;
        loop {
            let current_size = {
                let g = lock_or_log!(player.output.wasapi_player.lock());
                g.as_ref().map_or(0, |p| p.buffer_size())
            };
            if current_size >= min_buffer_samples || buffer_wait >= 50 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            buffer_wait += 1;
        }
        // 额外等待一小段时间确保数据稳定
        tokio::time::sleep(Duration::from_millis(20)).await;
        // 仅在需要时启动播放(重新获取锁,不跨 await)
        // 保持暂停时(start_playback=false)不启动,解码线程持续预缓冲,resume 随时可用
        if start_playback {
            let g = lock_or_log!(player.output.wasapi_player.lock());
            if let Some(ref wasapi) = *g {
                wasapi
                    .start()
                    .map_err(|e| format!("Failed to start WASAPI: {e:?}"))?;
            }
        }
    }
    Ok(())
}

/// 播放音轨（独占模式）
#[cfg(not(any(windows, target_os = "android")))]
pub async fn play_track_exclusive(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _path: &str,
    _position: Option<f32>,
    _start_playback: bool,
) -> Result<(), AppError> {
    Err(AppError::Audio(
        "Exclusive mode is only supported on Windows / Android".to_string(),
    ))
}

/// Seek共享模式
pub fn seek_track_shared(
    app: &AppHandle,
    state: &AppState,
    path: &str,
    time: f32,
) -> Result<(), AppError> {
    let player = &state.player;
    // 取消任何正在进行的淡入淡出,防止其 on_complete(pause) 在 seek 后执行
    player.fade.generation.fetch_add(1, Ordering::SeqCst);
    let eq_settings = state.equalizer.get_settings_handle();
    let mut decoder =
        SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    decoder.seek(Duration::from_secs_f32(time))?;
    let _ = decoder.prefill_buffer();
    let source: Box<dyn Source<Item = f32> + Send> = Box::new(
        VisualizationSource::new(
            LockFreeSymphoniaSource::new(decoder),
            Arc::clone(&player.visualization.spectrum_data),
            Some(app.clone()),
            Arc::clone(&player.visualization.target_fps),
        )
        .with_start_position(time)
        .with_eq_settings(eq_settings)
        .fade_in(Duration::from_millis(FADE_IN_ON_SEEK_MS)), // seek时使用较短的淡入
    );

    // 手动重采样到 mixer 采样率（同 play_track_shared 的修复）
    let resampled: Box<dyn Source<Item = f32> + Send> = {
        let stream_guard = lock_or_log!(player.output.output_stream.lock());
        if let Some(ref mixer_sink) = *stream_guard {
            let mixer_sr = mixer_sink.config().sample_rate();
            let mixer_ch = mixer_sink.config().channel_count();
            Box::new(rodio::source::UniformSourceIterator::new(
                source, mixer_ch, mixer_sr,
            ))
        } else {
            source
        }
    };

    {
        let sink = lock_or_log!(player.output.sink.lock());
        // 直接停止，不做阻塞的淡出
        sink.stop();
        sink.set_volume(*lock_or_log!(player.output.target_volume.lock()));
        drop(sink);
    }
    let sink = lock_or_log!(player.output.sink.lock());
    sink.append(resampled);
    sink.play();
    drop(sink);
    Ok(())
}
