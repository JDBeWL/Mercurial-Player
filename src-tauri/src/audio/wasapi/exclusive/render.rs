//! WASAPI 独占渲染线程：命令处理、淡入淡出状态机与输出缓冲搬运。

use super::device::initialize_exclusive_device;
use super::ring::{SpscSampleRing, UnderrunLogger};
use super::simd::convert_samples_to_bytes_into;
use super::{AudioCommand, AudioResponse, FadeAction, PlaybackState};
use crossbeam_channel::{Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// RT 循环单轮搬运的交错采样上限：16384 约合 48kHz 立体声 170ms，
/// 大于任何实际设备周期，据此一次分配到位后循环内不再扩容
const RT_CHUNK_SAMPLES: usize = 16_384;

/// 把结果送回等待方；发送失败说明对方已超时放弃，记一条日志再丢弃，
/// 否则调用方只会看到一个误导性的"超时"
fn respond(tx: &Sender<AudioResponse>, response: AudioResponse) {
    if let Err(e) = tx.send(response) {
        log::warn!("音频线程应答未送达（等待方可能已超时放弃）: {e}");
    }
}

/// 淡入淡出状态机(音频线程内部维护,不阻塞主线程)
#[derive(Debug, Clone, Copy)]
enum FadeState {
    /// 无淡入淡出,fade_factor = 1.0
    Idle,
    /// 正在淡出：fade_factor 从当前值线性走向 target_factor（通常为 0.0），
    /// 走完 remaining_frames 后执行 on_complete
    FadingOut {
        target_factor: f32,
        remaining_frames: usize,
        total_frames: usize,
        on_complete: FadeAction,
    },
    /// 正在淡入：target_factor 通常为 1.0
    FadingIn {
        target_factor: f32,
        remaining_frames: usize,
        total_frames: usize,
    },
}

pub(super) fn audio_thread_main(
    command_rx: Receiver<AudioCommand>,
    response_tx: Sender<AudioResponse>,
    state: Arc<Mutex<PlaybackState>>,
    is_running: Arc<AtomicBool>,
    sample_buffer: Arc<SpscSampleRing>,
    samples_written: Arc<AtomicU64>,
    sample_rate_atomic: Arc<AtomicU32>,
) {
    let _ = wasapi::initialize_mta();

    let mut audio_client: Option<wasapi::AudioClient> = None;
    let mut render_client: Option<wasapi::AudioRenderClient> = None;
    let mut event_handle: Option<wasapi::Handle> = None;
    let mut current_channels: u16 = 2;
    let mut current_bits: u16 = 32;
    let mut current_sample_type_is_float: bool = true;
    let mut is_playing = false;
    let mut current_volume = 1.0f32;
    // 淡入淡出状态(音频线程内部维护,不阻塞主线程)
    // fade_factor 是当前实际应用到采样的系数(0.0..=1.0)
    let mut fade_state = FadeState::Idle;
    let mut fade_factor: f32 = 1.0;

    // 欠载统计与节流上报
    let mut underrun_logger = UnderrunLogger::new();

    // 复用缓冲区:按 RT_CHUNK_SAMPLES 一次给足,循环内只做 clear+resize(不触发扩容)
    let mut reusable_samples: Vec<f32> = vec![0.0; RT_CHUNK_SAMPLES];
    let mut reusable_bytes: Vec<u8> = vec![0; RT_CHUNK_SAMPLES * 4];

    log::info!("WASAPI audio thread started");

    // 淡入淡出按挂钟时间推进的上一跳时刻
    let mut last_fade_tick = Instant::now();

    while is_running.load(Ordering::SeqCst) {
        match command_rx.try_recv() {
            Ok(AudioCommand::Initialize { device_name }) => {
                handle_initialize(
                    device_name.as_deref(),
                    &response_tx,
                    &mut audio_client,
                    &mut render_client,
                    &mut event_handle,
                    &mut current_channels,
                    &mut current_bits,
                    &mut current_sample_type_is_float,
                );
            }
            Ok(AudioCommand::Start) => {
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        fade_state = FadeState::Idle;
                        fade_factor = 1.0;
                        *lock_or_log!(state.lock()) = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::Stop) => {
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    fade_state = FadeState::Idle;
                    fade_factor = 1.0;
                    *lock_or_log!(state.lock()) = PlaybackState::Stopped;
                    sample_buffer.clear();
                }
            }
            Ok(AudioCommand::Pause) => {
                // 无淡出版暂停：用户关闭淡入淡出时走这里
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    fade_state = FadeState::Idle;
                    fade_factor = 1.0;
                    *lock_or_log!(state.lock()) = PlaybackState::Paused;
                }
            }
            Ok(AudioCommand::Resume) => {
                // 无淡出版恢复：同上
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        fade_state = FadeState::Idle;
                        fade_factor = 1.0;
                        *lock_or_log!(state.lock()) = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::SetVolume(vol)) => {
                current_volume = vol;
                // 注意：wasapi crate的AudioClient没有直接的音量控制方法
                // 音量在process_audio_output中通过软件乘法应用
            }
            Ok(AudioCommand::ClearBuffer) => sample_buffer.clear(),
            Ok(AudioCommand::Shutdown) => break,
            Ok(AudioCommand::StopWithFadeOut { duration_ms }) => {
                // 切歌/退出场景:启动淡出,完成后 stop_stream + clear_buffer
                let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                let frames = (sr * duration_ms / 1000).max(1) as usize;
                fade_state = FadeState::FadingOut {
                    target_factor: 0.0,
                    remaining_frames: frames,
                    total_frames: frames,
                    on_complete: FadeAction::Stop,
                };
                // 不立即 is_playing = false,让淡出继续播放
            }
            Ok(AudioCommand::PauseWithFadeOut { duration_ms }) => {
                // 用户暂停:启动淡出,完成后 stop_stream(保留缓冲区,不 clear)
                let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                let frames = (sr * duration_ms / 1000).max(1) as usize;
                fade_state = FadeState::FadingOut {
                    target_factor: 0.0,
                    remaining_frames: frames,
                    total_frames: frames,
                    on_complete: FadeAction::Pause,
                };
            }
            Ok(AudioCommand::ResumeWithFadeIn { duration_ms }) => {
                // 用户恢复:start_stream 后立即启动淡入(从 0 渐升到 1.0)
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        fade_factor = 0.0;
                        let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                        let frames = (sr * duration_ms / 1000).max(1) as usize;
                        fade_state = FadeState::FadingIn {
                            target_factor: 1.0,
                            remaining_frames: frames,
                            total_frames: frames,
                        };
                        *lock_or_log!(state.lock()) = PlaybackState::Playing;
                    }
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
        }

        // 更新淡入淡出状态(每帧推进一次)
        match fade_state {
            FadeState::Idle => {}
            FadeState::FadingOut {
                target_factor,
                ref mut remaining_frames,
                total_frames,
                on_complete,
            } => {
                if *remaining_frames > 0 {
                    let progress = 1.0 - (*remaining_frames as f32 / total_frames as f32);
                    fade_factor = 1.0 - progress * (1.0 - target_factor);
                }
                if *remaining_frames == 0 {
                    fade_factor = target_factor;
                    let action = on_complete;
                    fade_state = FadeState::Idle;
                    // 执行淡出后的动作
                    match action {
                        FadeAction::Pause => {
                            if let Some(ref client) = audio_client {
                                let _ = client.stop_stream();
                                is_playing = false;
                                // 不重置 fade_factor,resume 时从 0 渐升
                                *lock_or_log!(state.lock()) = PlaybackState::Paused;
                            }
                        }
                        FadeAction::Stop => {
                            if let Some(ref client) = audio_client {
                                let _ = client.stop_stream();
                                is_playing = false;
                                sample_buffer.clear();
                            }
                            fade_factor = 1.0; // 重置为 1.0,准备下一次播放
                            *lock_or_log!(state.lock()) = PlaybackState::Stopped;
                        }
                    }
                }
            }
            FadeState::FadingIn {
                target_factor,
                ref mut remaining_frames,
                total_frames,
            } => {
                if *remaining_frames > 0 {
                    let progress = 1.0 - (*remaining_frames as f32 / total_frames as f32);
                    fade_factor = progress * target_factor;
                }
                if *remaining_frames == 0 {
                    fade_factor = target_factor;
                    fade_state = FadeState::Idle;
                }
            }
        }

        if is_playing {
            // 应用 fade_factor:实际音量 = current_volume * fade_factor
            let effective_volume = current_volume * fade_factor;
            process_audio_output(
                audio_client.as_ref(),
                render_client.as_ref(),
                event_handle.as_ref(),
                &sample_buffer,
                current_channels,
                current_bits,
                current_sample_type_is_float,
                effective_volume,
                &mut is_playing,
                &state,
                &samples_written,
                &mut reusable_samples,
                &mut reusable_bytes,
                &mut underrun_logger,
            );
        } else {
            thread::sleep(Duration::from_millis(10));
        }

        // 按挂钟时间推进淡入淡出，不按已处理帧数：设备未起播或停转时一帧都处理不了，
        // 按帧计数会让 Stop/Pause 的淡出动作永不落地，状态卡在 Stopping/Pausing
        let elapsed_frames = {
            let now = Instant::now();
            let elapsed = now.duration_since(last_fade_tick);
            last_fade_tick = now;
            let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
            (elapsed.as_micros() as u64 * sr as u64 / 1_000_000) as usize
        };
        if elapsed_frames > 0 {
            match &mut fade_state {
                FadeState::FadingOut {
                    remaining_frames, ..
                }
                | FadeState::FadingIn {
                    remaining_frames, ..
                } => {
                    *remaining_frames = remaining_frames.saturating_sub(elapsed_frames);
                }
                FadeState::Idle => {}
            }
        }
    }

    if let Some(ref client) = audio_client {
        let _ = client.stop_stream();
    }

    log::info!("WASAPI audio thread stopped");
}

fn handle_initialize(
    device_name: Option<&str>,
    response_tx: &Sender<AudioResponse>,
    audio_client: &mut Option<wasapi::AudioClient>,
    render_client: &mut Option<wasapi::AudioRenderClient>,
    event_handle: &mut Option<wasapi::Handle>,
    current_channels: &mut u16,
    current_bits: &mut u16,
    current_sample_type_is_float: &mut bool,
) {
    match initialize_exclusive_device(device_name) {
        Ok((client, format_info)) => {
            let (sr, ch, name, bits, is_float) = format_info;
            *current_channels = ch;
            *current_bits = bits;
            *current_sample_type_is_float = is_float;

            log::info!("Audio format: {sr}Hz, {ch} channels, {bits} bits, float: {is_float}");

            match client.get_audiorenderclient() {
                Ok(rc) => match client.set_get_eventhandle() {
                    Ok(eh) => {
                        *render_client = Some(rc);
                        *event_handle = Some(eh);
                        *audio_client = Some(client);
                        respond(
                            response_tx,
                            AudioResponse::Initialized {
                                sample_rate: sr,
                                channels: ch,
                                device_name: name,
                            },
                        );
                    }
                    Err(e) => {
                        respond(
                            response_tx,
                            AudioResponse::InitFailed(format!("Failed to get event handle: {e:?}")),
                        );
                    }
                },
                Err(e) => {
                    respond(
                        response_tx,
                        AudioResponse::InitFailed(format!("Failed to get render client: {e:?}")),
                    );
                }
            }
        }
        Err(e) => {
            respond(response_tx, AudioResponse::InitFailed(e.to_string()));
        }
    }
}

fn process_audio_output(
    audio_client: Option<&wasapi::AudioClient>,
    render_client: Option<&wasapi::AudioRenderClient>,
    event_handle: Option<&wasapi::Handle>,
    sample_buffer: &SpscSampleRing,
    current_channels: u16,
    current_bits: u16,
    current_sample_type_is_float: bool,
    current_volume: f32,
    is_playing: &mut bool,
    state: &Arc<Mutex<PlaybackState>>,
    samples_written: &Arc<AtomicU64>,
    reusable_samples: &mut Vec<f32>,
    reusable_bytes: &mut Vec<u8>,
    underrun_logger: &mut UnderrunLogger,
) -> usize {
    if let (Some(client), Some(rc), Some(eh)) = (audio_client, render_client, event_handle) {
        // 使用更短的超时以获得更低延迟
        if eh.wait_for_event(5).is_ok() {
            if let Ok(frames_available) = client.get_available_space_in_frames() {
                if frames_available > 0 {
                    let channels = usize::from(current_channels.max(1));
                    // 单轮处理量封顶在复用缓冲区内，多出的空间留给下一轮，
                    // 这样 RT 循环里不会再出现堆分配
                    let frames_available =
                        frames_available.min((RT_CHUNK_SAMPLES / channels).max(1) as u32);
                    let samples_needed = frames_available as usize * channels;

                    // 无锁批量取走采样,不足部分填 0(欠载)
                    reusable_samples.clear();
                    reusable_samples.resize(samples_needed, 0.0);
                    let popped = sample_buffer.pop_slice(reusable_samples);

                    // 记录欠载情况(节流上报,避免高频日志恶化实时性)
                    underrun_logger.record(samples_needed - popped, samples_needed);

                    // 音量乘法(无锁争用;乘 1.0 的开销可忽略,无需特判)
                    for s in reusable_samples.iter_mut() {
                        *s *= current_volume;
                    }

                    // 复用 Vec<u8> 缓冲区
                    convert_samples_to_bytes_into(
                        reusable_samples,
                        current_bits,
                        current_sample_type_is_float,
                        reusable_bytes,
                    );

                    if rc
                        .write_to_device(frames_available as usize, reusable_bytes, None)
                        .is_ok()
                    {
                        // 更新已写入硬件的采样数
                        samples_written.fetch_add(samples_needed as u64, Ordering::SeqCst);
                        return frames_available as usize;
                    }
                    *is_playing = false;
                    *lock_or_log!(state.lock()) = PlaybackState::Stopped;
                }
            }
        }
    }
    0
}
