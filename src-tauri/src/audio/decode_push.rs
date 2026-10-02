//! 独占模式的解码推送线程（Windows = WASAPI 独占，Android = AAudio 独占）
//! 由 play_track_exclusive 启动：解码 -> (可选重采样) -> EQ -> 通道转换 -> 推送给独占播放器，
//! 同时驱动频谱分析器发 `spectrum-update`；靠代际计数器(generation)取消线程。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use tauri::AppHandle;

use crate::equalizer::EqSettings;

use super::decoder::LockFreeSymphoniaSource;
use super::dsp::convert_channels_into;
use super::emit::{emit_playback_position, emit_track_ended};
use super::eq_processor::EqProcessor;
use super::spectrum::SpectrumAnalyzer;

/// 根据采样率计算解码chunk 大小
/// 目标是保持约~21ms的处理块（1024@48kHz）
#[must_use]
#[cfg(any(windows, target_os = "android"))]
const fn calculate_decode_chunk_size(sample_rate: u32) -> usize {
    match sample_rate {
        0..=32000 => 512,        // ≤32kHz
        32001..=64000 => 1024,   // 44.1k/48k
        64001..=128_000 => 2048, // 88.2k/96k
        _ => 4096,               // 176.4k/192k/384k
    }
}

#[cfg(any(windows, target_os = "android"))]
pub(super) fn decode_and_push_to_wasapi(
    mut source: LockFreeSymphoniaSource,
    // 独占播放器（见 [`crate::app_state::PlatformPlayer`]），参数名沿用早期的 wasapi 叫法
    wasapi: Arc<Mutex<Option<crate::app_state::PlatformPlayer>>>,
    app: AppHandle,
    generation: Arc<AtomicU64>,
    thread_id_ref: Arc<AtomicU64>,
    my_id: u64,
    src_sr: u32,
    src_ch: u16,
    target_sr: u32,
    target_ch: u16,
    eq_settings: Arc<RwLock<EqSettings>>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    target_fps: Arc<AtomicU64>,
    gate: Arc<super::spectrum::SpectrumGate>,
    start_position: f32,
) {
    use audioadapter_buffers::direct::SequentialSliceOfVecs;
    use rubato::{
        Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType,
        WindowFunction,
    };
    // 记录启动时的代际,循环中检测代际变化即退出
    let my_generation = generation.load(Ordering::SeqCst);
    if generation.load(Ordering::SeqCst) != my_generation
        || thread_id_ref.load(Ordering::SeqCst) != my_id
    {
        return;
    }

    let mut eq_proc = EqProcessor::new(src_sr, src_ch);
    if let Ok(settings) = eq_settings.read() {
        eq_proc.update_settings(&settings);
    }
    let need_resample = src_sr != target_sr;
    let chunk_size = calculate_decode_chunk_size(src_sr);
    let resample_ratio = target_sr as f64 / src_sr as f64;
    let mut eq_update_counter: u32 = 0;
    let mut resampler: Option<Async<f32>> = if need_resample {
        // SincInterpolationParameters 走 builder 模式构造
        let params = SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2)
            .f_cutoff(0.925)
            .interpolation(SincInterpolationType::Linear)
            .oversampling_factor(128);
        Async::<f32>::new_sinc(
            resample_ratio,
            2.0,
            &params,
            chunk_size,
            src_ch as usize,
            FixedAsync::Input,
        )
        .ok()
    } else {
        None
    };

    let mut input_frames: Vec<Vec<f32>> = vec![Vec::with_capacity(chunk_size); src_ch as usize];
    // 精确计算最大输出缓冲区大小
    let max_output_frames = ((chunk_size as f64 * resample_ratio).ceil() as usize).max(chunk_size);
    let mut output_buffer: Vec<f32> = Vec::with_capacity(max_output_frames * target_ch as usize);
    // 预分配输出帧 buffer 复用,避免每次循环堆分配;
    // 每通道预留 max_output_frames + chunk_size 作安全余量 (rubato 启动延迟可能让首帧输出更多)
    let output_frames_capacity = max_output_frames + chunk_size;
    let mut output_frames_resampled: Vec<Vec<f32>> =
        vec![Vec::with_capacity(output_frames_capacity); src_ch as usize];
    // 复用 interleaved 缓冲区,避免每帧堆分配
    let samples_needed = chunk_size * src_ch as usize;
    let mut interleaved: Vec<f32> = Vec::with_capacity(samples_needed);
    // 通道转换用复用缓冲区
    let mut converted_buffer: Vec<f32> = Vec::with_capacity(max_output_frames * target_ch as usize);

    // 播放位置追踪
    let mut last_position_emit_time: u64 = 0;

    // 频谱分析器:用最终输出采样(重采样/EQ/声道转换之后)驱动,
    // 与共享模式 VisualizationSource 发送相同的 spectrum-update 事件
    let mut spectrum_analyzer = SpectrumAnalyzer::new(target_sr);

    // 发送播放位置的闭包
    let emit_position = |last_time: &mut u64| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if now - *last_time >= 100 {
            *last_time = now;
            let samples_played = lock_or_log!(wasapi.lock())
                .as_ref()
                .map_or(0, |p| p.samples_written());
            let position =
                start_position + samples_played as f32 / (target_sr as f32 * target_ch as f32);
            let _ = emit_playback_position(&app, position);
        }
    };

    // EOF 收尾:等缓冲排空后停止播放并发出 track-ended。
    // 整数倍 chunk 长度的曲目最后一轮读满块,下一轮首样本即 EOF,只能在此收尾;
    // 遗漏会让这类曲目放完不发 track-ended(独占模式表现为不自动切下一首)。
    let finish_eof = |my_gen: u64, my_tid: u64| {
        use crate::audio::PlaybackState;

        // 排空等待上限:设备异常时缓冲可能永不排空
        const MAX_DRAIN_WAIT: Duration = Duration::from_secs(5);
        let mut drain_deadline = std::time::Instant::now() + MAX_DRAIN_WAIT;
        // 只有排空才算播完;代际变化与 player 被取走属于外部取消,不发事件
        let mut drained = true;
        loop {
            if generation.load(Ordering::SeqCst) != my_gen
                || thread_id_ref.load(Ordering::SeqCst) != my_tid
            {
                drained = false;
                break;
            }
            let status = {
                let guard = lock_or_log!(wasapi.lock());
                guard.as_ref().map(|p| {
                    (
                        p.buffer_size(),
                        matches!(p.state(), PlaybackState::Paused | PlaybackState::Pausing),
                    )
                })
            };
            let Some((buf_size, paused)) = status else {
                drained = false;
                break;
            };
            if buf_size == 0 {
                break;
            }
            if std::time::Instant::now() >= drain_deadline {
                // 暂停时缓冲不会排空,不算停滞:延后截止时间继续等
                if !paused {
                    break;
                }
                drain_deadline = std::time::Instant::now() + MAX_DRAIN_WAIT;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !drained
            || generation.load(Ordering::SeqCst) != my_gen
            || thread_id_ref.load(Ordering::SeqCst) != my_tid
        {
            return;
        }
        // stop 持锁,emit 放锁外
        let stopped = {
            let guard = lock_or_log!(wasapi.lock());
            match guard.as_ref() {
                Some(p) => {
                    let _ = p.stop();
                    true
                }
                None => false,
            }
        };
        if stopped {
            if let Err(e) = emit_track_ended(&app) {
                log::warn!("track-ended 发送失败，自动续播可能中断: {e}");
            }
        }
    };

    loop {
        if generation.load(Ordering::SeqCst) != my_generation
            || thread_id_ref.load(Ordering::SeqCst) != my_id
            || lock_or_log!(wasapi.lock()).is_none()
        {
            break;
        }
        for ch in &mut input_frames {
            ch.clear();
        }

        // 复用 interleaved 缓冲区
        interleaved.clear();
        let mut eof = false;
        for _ in 0..samples_needed {
            // 本线程不是实时回调，用带节奏的取数：通道暂时为空时等解码线程追上，
            // 而不是把静音直接灌满输出环形缓冲
            if let Some(s) = source.next_paced() {
                interleaved.push(s);
            } else {
                eof = true;
                break;
            }
        }
        if interleaved.is_empty() {
            // 本轮没读到采样:eof 说明源已耗尽(整数倍长度走这里),空读异常则直接退出
            if eof {
                finish_eof(my_generation, my_id);
            }
            break;
        }

        // 发送播放位置
        emit_position(&mut last_position_emit_time);

        for (i, s) in interleaved.iter().enumerate() {
            input_frames[i % src_ch as usize].push(*s);
        }

        eq_update_counter += 1;
        if eq_update_counter >= 4 {
            eq_update_counter = 0;
            if let Ok(settings) = eq_settings.try_read() {
                eq_proc.update_settings(&settings);
            }
        }

        if eq_proc.is_enabled() {
            for (ch, frame) in input_frames.iter_mut().enumerate() {
                for s in frame.iter_mut() {
                    *s = eq_proc.process_sample_cached(*s, ch);
                }
            }
        }

        // 处理重采样 - 用 Cow 避免成功路径的 clone
        use std::borrow::Cow;
        let output_frames: Cow<'_, [Vec<f32>]> = if let Some(ref mut r) = resampler {
            let actual = input_frames[0].len();
            if actual < chunk_size && !eof {
                // 非EOF情况下填充到chunk_size
                for ch in &mut input_frames {
                    let last_sample = ch.last().copied().unwrap_or(0.0);
                    let samples_to_add = chunk_size - ch.len();
                    ch.extend((0..samples_to_add).map(|i| {
                        let fade = 1.0 - (i as f32 / samples_to_add as f32);
                        last_sample * fade
                    }));
                }
                // 用 SequentialSliceOfVecs adapter 包装输入输出
                match SequentialSliceOfVecs::new(&input_frames, src_ch as usize, chunk_size) {
                    Ok(input_adapter) => {
                        // 清空并复用输出 buffer
                        for ch in &mut output_frames_resampled {
                            ch.clear();
                            ch.resize(output_frames_capacity, 0.0);
                        }
                        match SequentialSliceOfVecs::new_mut(
                            &mut output_frames_resampled,
                            src_ch as usize,
                            output_frames_capacity,
                        ) {
                            Ok(mut output_adapter) => {
                                let indexing = Indexing::new();
                                match r.process_into_buffer(
                                    &input_adapter,
                                    &mut output_adapter,
                                    Some(&indexing),
                                ) {
                                    Ok((_in_used, out_written)) => {
                                        for ch in &mut output_frames_resampled {
                                            ch.truncate(out_written);
                                        }
                                        // 成功路径借用即可：output_frames_resampled
                                        // 在本迭代内只读，下一轮循环才会被 clear/resize
                                        Cow::Borrowed(&output_frames_resampled)
                                    }
                                    Err(_) => Cow::Borrowed(&input_frames),
                                }
                            }
                            Err(_) => Cow::Borrowed(&input_frames),
                        }
                    }
                    Err(_) => Cow::Borrowed(&input_frames),
                }
            } else if eof && actual < chunk_size {
                // EOF情况下 - 直接借用 input_frames,避免 clone
                Cow::Borrowed(&input_frames)
            } else {
                let frames_in = input_frames[0].len();
                match SequentialSliceOfVecs::new(&input_frames, src_ch as usize, frames_in) {
                    Ok(input_adapter) => {
                        for ch in &mut output_frames_resampled {
                            ch.clear();
                            ch.resize(output_frames_capacity, 0.0);
                        }
                        match SequentialSliceOfVecs::new_mut(
                            &mut output_frames_resampled,
                            src_ch as usize,
                            output_frames_capacity,
                        ) {
                            Ok(mut output_adapter) => {
                                let indexing = Indexing::new();
                                match r.process_into_buffer(
                                    &input_adapter,
                                    &mut output_adapter,
                                    Some(&indexing),
                                ) {
                                    Ok((_in_used, out_written)) => {
                                        for ch in &mut output_frames_resampled {
                                            ch.truncate(out_written);
                                        }
                                        // 成功路径借用即可：output_frames_resampled
                                        // 在本迭代内只读，下一轮循环才会被 clear/resize
                                        Cow::Borrowed(&output_frames_resampled)
                                    }
                                    Err(_) => Cow::Borrowed(&input_frames),
                                }
                            }
                            Err(_) => Cow::Borrowed(&input_frames),
                        }
                    }
                    Err(_) => Cow::Borrowed(&input_frames),
                }
            }
        } else {
            Cow::Borrowed(&input_frames)
        };

        // 交错输出帧
        output_buffer.clear();
        let out_len = output_frames.first().map_or(0, Vec::len);
        for i in 0..out_len {
            for ch in 0..output_frames.len() {
                output_buffer.push(output_frames[ch].get(i).copied().unwrap_or(0.0));
            }
        }

        // 通道转换:使用复用缓冲区,避免 clone
        // current_ch 是重采样后实际声道数
        let current_ch = output_frames.len() as u16;
        let final_out: &[f32] = if current_ch == target_ch {
            &output_buffer
        } else {
            convert_channels_into(&output_buffer, current_ch, target_ch, &mut converted_buffer);
            &converted_buffer
        };

        if !final_out.is_empty() {
            // 可视化:推送采样同时计算频谱并发送 spectrum-update。
            // 面板不在屏或应用已到后台时整段跳过 —— FFT、序列化与跨线程投递都省掉，
            // 重新显示时分析器从零开始积累窗口（约 43ms），不影响正确性。
            if gate.allowed() {
                spectrum_analyzer.push_and_maybe_emit(final_out, &spectrum_data, &target_fps, &app);
            }

            // 写入端句柄：外层锁只用于取播放器实例本身，拿到后水位等待与推送都不持锁。
            // 若在持锁期间等背压，会把切设备/停止这些命令线程一起堵死。
            let producer = {
                let guard = lock_or_log!(wasapi.lock());
                guard.as_ref().map(|p| p.producer())
            };

            if let Some(producer) = producer {
                let max_buffer = target_sr as usize * target_ch as usize * 2;
                loop {
                    if generation.load(Ordering::SeqCst) != my_generation
                        || thread_id_ref.load(Ordering::SeqCst) != my_id
                    {
                        break;
                    }
                    if producer.buffer_size() < max_buffer {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                    // 等待时继续发送播放位置
                    emit_position(&mut last_position_emit_time);
                }
                if generation.load(Ordering::SeqCst) != my_generation
                    || thread_id_ref.load(Ordering::SeqCst) != my_id
                {
                    break;
                }

                if producer.push_samples(final_out).is_err() {
                    break;
                }
            }
        }

        if eof && interleaved.len() < samples_needed {
            finish_eof(my_generation, my_id);
            break;
        }
        // 让出 CPU 给其他线程 (主要给消费线程),避免 100% 占用
        // 但用更短的时间,因为已经被 condvar 同步过
        std::thread::yield_now();
    }
}
