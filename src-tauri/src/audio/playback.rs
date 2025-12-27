//! 音频播放模块
//!
//! 提供音频播放、暂停、恢复、音量控制等功能。

use super::decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};

#[cfg(windows)]
use super::wasapi::PlaybackState;
use crate::equalizer::{EqSettings, EQ_BAND_COUNT};
use crate::AppState;
use rodio::Source;
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    pub is_playing: bool,
    pub position_secs: f32,
    pub volume: f32,
}

impl PlaybackStatus {
    #[must_use]
    pub const fn new(is_playing: bool, position_secs: f32, volume: f32) -> Self {
        Self { is_playing, position_secs, volume }
    }
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumUpdateEvent {
    pub data: Vec<f32>,
    pub timestamp: u64,
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackEndedEvent {
    pub timestamp: u64,
}

fn emit_spectrum_update(app: &AppHandle, data: &[f32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("spectrum-update", SpectrumUpdateEvent {
        data: data.to_vec(),
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64,
    })?;
    Ok(())
}

fn emit_track_ended(app: &AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("track-ended", TrackEndedEvent {
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64,
    })?;
    Ok(())
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPositionEvent {
    pub position: f32, // 秒
}

fn emit_playback_position(app: &AppHandle, position: f32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("playback-position", PlaybackPositionEvent { position })?;
    Ok(())
}

pub struct VisualizationSource<I: Source<Item = f32> + Send> {
    input: I,
    #[allow(dead_code)]
    waveform_data: Arc<Mutex<Vec<f32>>>, // 保留用于未来波形显示功能
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
    app_handle: Option<AppHandle>,
    last_emit_time: std::sync::atomic::AtomicU64,
    last_fft_time: std::sync::atomic::AtomicU64,
    last_position_emit_time: std::sync::atomic::AtomicU64,
    eq_settings: Arc<RwLock<EqSettings>>,
    eq_processor: EqProcessor,
    // 缓存的 EQ 设置，减少锁读取频率
    cached_eq_enabled: bool,
    eq_update_counter: u32,
    // 预分配的 FFT 工作缓冲区（动态大小）
    fft_buffer: Vec<f32>,
    spectrum_buffer: Vec<f32>,
    // 播放位置追踪
    samples_played: u64,
    sample_rate: u32,
    channels: u16,
    // 动态 FFT 缓冲区大小（基于采样率）
    fft_size: usize,
}

struct EqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    states: Vec<Vec<crate::equalizer::BiquadState>>,
    sample_rate: f32,
    channels: usize,
}

impl EqProcessor {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            coefficients: vec![crate::equalizer::BiquadCoefficients::default(); EQ_BAND_COUNT],
            states: vec![vec![crate::equalizer::BiquadState::default(); channels as usize]; EQ_BAND_COUNT],
            sample_rate: sample_rate as f32,
            channels: channels as usize,
        }
    }

    fn update_coefficients(&mut self, settings: &EqSettings) {
        use crate::equalizer::{BiquadCoefficients, EQ_FREQUENCIES, EQ_Q_VALUES};
        for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::peaking_eq(self.sample_rate, freq, settings.gains[i], EQ_Q_VALUES[i]);
        }
    }

    fn process_sample(&mut self, input: f32, channel: usize, settings: &EqSettings) -> f32 {
        if !settings.enabled { return input; }
        let mut sample = input * 10.0_f32.powf(settings.preamp / 20.0);
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            sample = self.states[band][channel].process(sample, coeffs);
        }
        soft_clip(sample)
    }
}

fn soft_clip(x: f32) -> f32 {
    // 使用更平滑的软削波，阈值提高到 0.95，过渡更柔和
    let threshold = 0.95;
    if x.abs() <= threshold {
        x
    } else {
        // 使用 tanh 进行平滑压缩，保留更多动态范围
        let sign = x.signum();
        let abs_x = x.abs();
        let over = abs_x - threshold;
        // 更平滑的过渡：threshold + (1 - threshold) * tanh(over / (1 - threshold))
        sign * (threshold + (1.0 - threshold) * (over / (1.0 - threshold) * 0.5).tanh())
    }
}

/// 根据采样率计算最佳 FFT 缓冲区大小
/// 目标是保持约 ~43ms 的分析窗口（2048 @ 48kHz）
#[must_use]
const fn calculate_fft_size(sample_rate: u32) -> usize {
    // 基准：48kHz 使用 2048 样本 ≈ 42.7ms
    // 公式：fft_size = sample_rate * 0.0427
    // 但 FFT 大小必须是 2 的幂次
    match sample_rate {
        0..=32000 => 1024,      // ≤32kHz: 1024 样本
        32001..=64000 => 2048,  // 44.1k/48k: 2048 样本
        64001..=128000 => 4096, // 88.2k/96k: 4096 样本
        _ => 8192,              // 176.4k/192k/384k: 8192 样本
    }
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    pub fn new(input: I, waveform_data: Arc<Mutex<Vec<f32>>>, spectrum_data: Arc<Mutex<Vec<f32>>>, app_handle: Option<AppHandle>) -> Self {
        let (sr, ch) = (input.sample_rate(), input.channels());
        let fft_size = calculate_fft_size(sr);
        Self {
            input,
            waveform_data,
            spectrum_data,
            buffer: Vec::with_capacity(fft_size),
            prev_spectrum: vec![0.0; 128],
            app_handle,
            last_emit_time: std::sync::atomic::AtomicU64::new(0),
            last_fft_time: std::sync::atomic::AtomicU64::new(0),
            last_position_emit_time: std::sync::atomic::AtomicU64::new(0),
            eq_settings: Arc::new(RwLock::new(EqSettings::default())),
            eq_processor: EqProcessor::new(sr, ch),
            cached_eq_enabled: false,
            eq_update_counter: 0,
            fft_buffer: vec![0.0; fft_size],
            spectrum_buffer: vec![0.0; 128],
            samples_played: 0,
            sample_rate: sr,
            channels: ch,
            fft_size,
        }
    }
    
    /// 设置初始播放位置（用于 seek 操作）
    #[must_use]
    pub fn with_start_position(mut self, position_secs: f32) -> Self {
        // 将秒转换为采样数
        self.samples_played = (position_secs * self.sample_rate as f32 * self.channels as f32) as u64;
        self
    }

    #[must_use]
    pub fn with_eq_settings(mut self, eq_settings: Arc<RwLock<EqSettings>>) -> Self {
        self.eq_settings = eq_settings;
        if let Ok(s) = self.eq_settings.read() {
            self.eq_processor.update_coefficients(&s);
            self.cached_eq_enabled = s.enabled;
        }
        self
    }
}

impl<I: Source<Item = f32> + Send> Iterator for VisualizationSource<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;
        let ch = self.buffer.len() % self.eq_processor.channels;
        
        // 追踪播放位置
        self.samples_played += 1;
        
        // 每 512 个采样才检查一次 EQ 设置，减少锁竞争
        self.eq_update_counter += 1;
        let should_update_eq = self.eq_update_counter >= 512;
        
        let processed = if should_update_eq {
            self.eq_update_counter = 0;
            if let Ok(s) = self.eq_settings.try_read() {
                self.cached_eq_enabled = s.enabled;
                if self.cached_eq_enabled {
                    self.eq_processor.update_coefficients(&s);
                }
                self.eq_processor.process_sample(sample, ch, &s)
            } else {
                // 锁获取失败时使用缓存的状态
                if self.cached_eq_enabled {
                    // 使用上次的系数处理
                    let settings = EqSettings { enabled: true, ..Default::default() };
                    self.eq_processor.process_sample(sample, ch, &settings)
                } else {
                    sample
                }
            }
        } else {
            // 不更新设置，直接使用缓存的状态处理
            if self.cached_eq_enabled {
                let settings = EqSettings { enabled: true, ..Default::default() };
                self.eq_processor.process_sample(sample, ch, &settings)
            } else {
                sample
            }
        };

        self.buffer.push(processed);
        if self.buffer.len() >= self.fft_size {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
            
            // 发送播放位置（每 100ms 一次）
            let last_pos_emit = self.last_position_emit_time.load(Ordering::Relaxed);
            if now - last_pos_emit >= 100 {
                self.last_position_emit_time.store(now, Ordering::Relaxed);
                if let Some(ref app) = self.app_handle {
                    // 计算当前播放位置（秒）
                    let position = self.samples_played as f32 / (self.sample_rate as f32 * self.channels as f32);
                    let _ = emit_playback_position(app, position);
                }
            }
            
            let last_fft = self.last_fft_time.load(Ordering::Relaxed);
            
            // 限制 FFT 计算频率为约 60fps (16ms)
            if now - last_fft >= 16 {
                self.last_fft_time.store(now, Ordering::Relaxed);
                
                if let Ok(mut spec) = self.spectrum_data.try_lock() {
                    // 复用预分配的缓冲区（动态大小）
                    self.fft_buffer.copy_from_slice(&self.buffer[..self.fft_size]);
                    let hann = hann_window(&self.fft_buffer);
                    
                    if let Ok(spectrum) = samples_fft_to_spectrum(&hann, self.input.sample_rate(), FrequencyLimit::Range(20.0, 20000.0), Some(&divide_by_N_sqrt)) {
                        // 重置频谱缓冲区
                        self.spectrum_buffer.fill(0.0);
                        
                        // AE 风格：线性频率分布，每个 bin 覆盖相等的频率范围
                        let num_bins = 128;
                        let freq_min = 20.0_f32;
                        let freq_max = 16000.0_f32; // 人耳敏感范围
                        let freq_step = (freq_max - freq_min) / num_bins as f32;
                        
                        let mut bin_counts = [0u32; 128];
                        
                        for (freq, value) in spectrum.data() {
                            let f = freq.val();
                            if f < freq_min || f > freq_max { continue; }
                            
                            // 线性映射
                            let bin = ((f - freq_min) / freq_step).floor() as usize;
                            let bin = bin.min(num_bins - 1);
                            
                            // 取该 bin 内的最大值（峰值检测）
                            let v = value.val();
                            if v > self.spectrum_buffer[bin] {
                                self.spectrum_buffer[bin] = v;
                            }
                            bin_counts[bin] += 1;
                        }
                        
                        // AE 风格的平滑：快速上升，缓慢下降（峰值保持）
                        for i in 0..128 {
                            let target = self.spectrum_buffer[i];
                            let current = self.prev_spectrum[i];
                            
                            if target > current {
                                // 快速上升
                                self.prev_spectrum[i] = current * 0.3 + target * 0.7;
                            } else {
                                // 缓慢下降（重力感）
                                self.prev_spectrum[i] = current * 0.85 + target * 0.15;
                            }
                        }
                        
                        // 直接复制到共享数据，避免 clone
                        spec.clear();
                        spec.extend_from_slice(&self.prev_spectrum);
                    }
                }
                
                // 发送事件（限制在 16ms 一次）
                if let Some(ref app) = self.app_handle {
                    let last_emit = self.last_emit_time.load(Ordering::Relaxed);
                    if now - last_emit >= 16 {
                        let _ = emit_spectrum_update(app, &self.prev_spectrum);
                        self.last_emit_time.store(now, Ordering::Relaxed);
                    }
                }
            }
            
            // 保留后半部分数据用于重叠分析，提高平滑度
            let half = self.buffer.len() / 2;
            self.buffer.drain(..half);
        }
        Some(processed)
    }
}

impl<I: Source<Item = f32> + Send> Source for VisualizationSource<I> {
    fn current_span_len(&self) -> Option<usize> { self.input.current_span_len() }
    fn channels(&self) -> u16 { self.input.channels() }
    fn sample_rate(&self) -> u32 { self.input.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

/// 播放音轨（共享模式）
pub fn play_track_shared(app: &AppHandle, state: &State<AppState>, path: &str, position: Option<f32>) -> Result<(), String> {
    let player = &state.player;
    {
        let sink = player.sink.lock().unwrap();
        // 直接停止，不做淡出（淡出会阻塞主线程）
        // 新音源会有 fade_in 效果来平滑过渡
        sink.stop();
        sink.set_volume(*player.target_volume.lock().unwrap());
    }
    *player.current_path.lock().unwrap() = Some(path.to_string());
    *player.current_source.lock().unwrap() = None;
    let (waveform, spectrum, eq_settings) = (
        Arc::clone(&player.waveform_data),
        Arc::clone(&player.spectrum_data),
        state.equalizer.get_settings_handle(),
    );

    let source: Box<dyn Source<Item = f32> + Send> = match SymphoniaDecoder::new(path) {
        Ok(mut dec) => {
            let start_pos = position.unwrap_or(0.0);
            if let Some(t) = position { let _ = dec.seek(Duration::from_secs_f32(t)); }
            let _ = dec.prefill_buffer();
            println!("使用Symphonia解码器: {path}");
            Box::new(
                VisualizationSource::new(LockFreeSymphoniaSource::new(dec), waveform, spectrum, Some(app.clone()))
                    .with_start_position(start_pos)
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(80)) // 稍长的淡入来补偿没有淡出
            )
        }
        Err(e) => {
            println!("Symphonia解码失败，回退到rodio: {e}");
            let file = File::open(path).map_err(|e| e.to_string())?;
            Box::new(
                VisualizationSource::new(rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?, waveform, spectrum, Some(app.clone()))
                    .with_start_position(position.unwrap_or(0.0))
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(80))
            )
        }
    };
    let sink = player.sink.lock().unwrap();
    sink.append(source);
    sink.play();
    Ok(())
}

/// 播放音轨（独占模式）
#[cfg(windows)]
pub fn play_track_exclusive(app: &AppHandle, state: &State<AppState>, path: &str, position: Option<f32>) -> Result<(), String> {
    let player = &state.player;
    player.decode_thread_stop.store(true, Ordering::SeqCst);
    let new_thread_id = player.decode_thread_id.fetch_add(1, Ordering::SeqCst) + 1;
    {
        if let Some(ref wasapi) = *player.wasapi_player.lock().unwrap() {
            let _ = wasapi.stop();
            let _ = wasapi.clear_buffer();
        }
    }
    // 使用更短的等待时间，并在后台线程中处理
    std::thread::sleep(Duration::from_millis(50));
    player.decode_thread_stop.store(false, Ordering::SeqCst);
    std::sync::atomic::fence(Ordering::SeqCst);

    let (target_sr, target_ch) = {
        let g = player.wasapi_player.lock().unwrap();
        let wasapi = g.as_ref().ok_or("WASAPI player not initialized")?;
        (wasapi.get_sample_rate(), wasapi.get_channels())
    };
    if position.is_none() {
        *player.current_path.lock().unwrap() = Some(path.to_string());
    }
    println!("WASAPI Exclusive: {path} @ {target_sr}Hz, {target_ch} ch");

    let mut decoder = SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    if let Some(t) = position { let _ = decoder.seek(Duration::from_secs_f32(t)); }
    let _ = decoder.prefill_buffer();
    let (src_sr, src_ch) = (decoder.sample_rate(), decoder.channels());
    println!("Source: {src_sr}Hz, {src_ch} ch -> Target: {target_sr}Hz, {target_ch} ch");

    let source = LockFreeSymphoniaSource::new(decoder);
    let (wasapi_clone, waveform, spectrum, stop_flag, thread_id, eq_settings) = (
        Arc::clone(&player.wasapi_player),
        Arc::clone(&player.waveform_data),
        Arc::clone(&player.spectrum_data),
        Arc::clone(&player.decode_thread_stop),
        Arc::clone(&player.decode_thread_id),
        state.equalizer.get_settings_handle(),
    );
    let app_clone = app.clone();
    let thread_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_started_clone = Arc::clone(&thread_started);

    std::thread::spawn(move || {
        thread_started_clone.store(true, Ordering::SeqCst);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_and_push_to_wasapi(source, wasapi_clone, waveform, spectrum, app_clone, stop_flag, thread_id, new_thread_id, src_sr, src_ch, target_sr, target_ch, eq_settings)
        }));
    });

    // 减少等待时间
    let mut wait = 0;
    while !thread_started.load(Ordering::SeqCst) && wait < 20 {
        std::thread::sleep(Duration::from_millis(5));
        wait += 1;
    }
    {
        if let Some(ref wasapi) = *player.wasapi_player.lock().unwrap() {
            std::thread::sleep(Duration::from_millis(80));
            wasapi.start().map_err(|e| format!("Failed to start WASAPI: {e:?}"))?;
        }
    }
    Ok(())
}

/// 播放音轨（独占模式）
#[cfg(not(windows))]
pub fn play_track_exclusive(_app: &AppHandle, _state: &State<AppState>, _path: &str, _position: Option<f32>) -> Result<(), String> {
    Err("Exclusive mode is only supported on Windows".to_string())
}

/// 根据采样率计算解码 chunk 大小
/// 目标是保持约 ~21ms 的处理块（1024 @ 48kHz）
#[must_use]
const fn calculate_decode_chunk_size(sample_rate: u32) -> usize {
    match sample_rate {
        0..=32000 => 512,       // ≤32kHz
        32001..=64000 => 1024,  // 44.1k/48k
        64001..=128000 => 2048, // 88.2k/96k
        _ => 4096,              // 176.4k/192k/384k
    }
}

#[cfg(windows)]
fn decode_and_push_to_wasapi(
    mut source: LockFreeSymphoniaSource,
    wasapi: Arc<Mutex<Option<super::wasapi::WasapiExclusivePlayback>>>,
    _waveform: Arc<Mutex<Vec<f32>>>,
    _spectrum: Arc<Mutex<Vec<f32>>>,
    app: AppHandle,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    thread_id_ref: Arc<std::sync::atomic::AtomicU64>,
    my_id: u64,
    src_sr: u32,
    src_ch: u16,
    target_sr: u32,
    target_ch: u16,
    eq_settings: Arc<RwLock<EqSettings>>,
) {
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id { return; }

    let mut eq_proc = EqProcessor::new(src_sr, src_ch);
    let need_resample = src_sr != target_sr;
    // 根据源采样率动态计算 chunk 大小
    let chunk_size = calculate_decode_chunk_size(src_sr);
    let mut resampler: Option<SincFixedIn<f32>> = if need_resample {
        SincFixedIn::<f32>::new(
            target_sr as f64 / src_sr as f64,
            2.0,
            SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.925,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            },
            chunk_size,
            src_ch as usize,
        ).ok()
    } else { None };

    let mut input_frames: Vec<Vec<f32>> = vec![Vec::with_capacity(chunk_size * 2); src_ch as usize];
    let mut output_buffer: Vec<f32> = Vec::with_capacity(chunk_size * target_ch as usize * 4);

    loop {
        if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id || wasapi.lock().unwrap().is_none() { break; }
        for ch in &mut input_frames { ch.clear(); }

        let samples_needed = chunk_size * src_ch as usize;
        let mut interleaved = Vec::with_capacity(samples_needed);
        let mut eof = false;
        for _ in 0..samples_needed {
            if let Some(s) = source.next() { interleaved.push(s); }
            else { eof = true; break; }
        }
        if interleaved.is_empty() { break; }

        for (i, s) in interleaved.iter().enumerate() {
            input_frames[i % src_ch as usize].push(*s);
        }

        if let Ok(settings) = eq_settings.try_read() {
            if settings.enabled {
                eq_proc.update_coefficients(&settings);
                for ch in 0..src_ch as usize {
                    for s in &mut input_frames[ch] {
                        *s = eq_proc.process_sample(*s, ch, &settings);
                    }
                }
            }
        }

        let output_frames: Vec<Vec<f32>> = if let Some(ref mut r) = resampler {
            let actual = input_frames[0].len();
            if actual < chunk_size {
                // 使用最后一个样本值进行平滑填充，而不是用 0 填充
                // 这样可以避免突然的静音导致的爆音
                for ch in &mut input_frames {
                    let last_sample = ch.last().copied().unwrap_or(0.0);
                    // 渐变到 0，而不是直接填充 0
                    let samples_to_add = chunk_size - ch.len();
                    for i in 0..samples_to_add {
                        let fade = 1.0 - (i as f32 / samples_to_add as f32);
                        ch.push(last_sample * fade);
                    }
                }
            }
            r.process(&input_frames, None).unwrap_or_else(|_| input_frames.clone())
        } else { input_frames.clone() };

        output_buffer.clear();
        let out_len = output_frames.first().map_or(0, Vec::len);
        for i in 0..out_len {
            for ch in 0..output_frames.len() {
                output_buffer.push(output_frames[ch].get(i).copied().unwrap_or(0.0));
            }
        }

        let final_out: Vec<f32> = if src_ch != target_ch {
            convert_channels(&output_buffer, src_ch, target_ch)
        } else { output_buffer.clone() };

        if !final_out.is_empty() {
            if let Some(ref p) = *wasapi.lock().unwrap() {
                if p.push_samples(final_out).is_err() { break; }
            }
        }

        if eof && interleaved.len() < samples_needed {
            loop {
                if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }
                let buf_size = wasapi.lock().unwrap().as_ref().map_or(0, |p| p.get_buffer_size());
                if buf_size == 0 { break; }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !stop_flag.load(Ordering::SeqCst) && thread_id_ref.load(Ordering::SeqCst) == my_id {
                if let Some(ref p) = *wasapi.lock().unwrap() { let _ = p.stop(); }
                // 发送播放结束事件
                let _ = emit_track_ended(&app);
            }
            break;
        }
        std::thread::sleep(Duration::from_micros(500));
    }
}

/// 5.1/7.1 环绕声到立体声的专业混音
/// 使用 ITU-R BS.775-1 标准的下混系数
fn downmix_surround_to_stereo(samples: &[f32], src_ch: usize, frame: usize) -> (f32, f32) {
    let start = frame * src_ch;
    
    let fl = samples[start];
    let fr = samples[start + 1];
    let fc = if src_ch > 2 { samples[start + 2] } else { 0.0 };
    let _lfe = if src_ch > 3 { samples[start + 3] } else { 0.0 };
    
    const CENTER_MIX: f32 = 0.707;
    const SURROUND_MIX: f32 = 0.707;
    const BACK_MIX: f32 = 0.5;
    
    let (mut left, mut right) = (fl, fr);
    
    left += fc * CENTER_MIX;
    right += fc * CENTER_MIX;
    
    match src_ch {
        6 => {
            let sl = samples[start + 4];
            let sr = samples[start + 5];
            left += sl * SURROUND_MIX;
            right += sr * SURROUND_MIX;
        }
        8 => {
            let bl = samples[start + 4];
            let br = samples[start + 5];
            let sl = samples[start + 6];
            let sr = samples[start + 7];
            left += sl * SURROUND_MIX + bl * BACK_MIX;
            right += sr * SURROUND_MIX + br * BACK_MIX;
        }
        _ => {}
    }
    
    let normalize = match src_ch {
        6 => 0.707,
        8 => 0.667,
        _ => 0.8,
    };
    
    (
        (left * normalize).clamp(-1.0, 1.0),
        (right * normalize).clamp(-1.0, 1.0)
    )
}

fn convert_channels(samples: &[f32], src_ch: u16, target_ch: u16) -> Vec<f32> {
    if src_ch == target_ch { return samples.to_vec(); }
    let (src, tgt) = (src_ch as usize, target_ch as usize);
    let frames = samples.len() / src;
    let mut out = Vec::with_capacity(frames * tgt);
    
    for f in 0..frames {
        let start = f * src;
        match (src, tgt) {
            (1, 2) => { 
                let s = samples[start]; 
                out.push(s); 
                out.push(s); 
            }
            (2, 1) => {
                out.push((samples[start] + samples[start + 1]) / 2.0);
            }
            (6, 2) | (8, 2) => {
                // 5.1/7.1 到立体声的专业混音
                let (left, right) = downmix_surround_to_stereo(samples, src, f);
                out.push(left);
                out.push(right);
            }
            _ => {
                // 其他情况：简单截取或填充
                for ch in 0..tgt {
                    out.push(if ch < src { samples[start + ch] } else { samples[start] });
                }
            }
        }
    }
    out
}

/// Seek 共享模式
pub fn seek_track_shared(app: &AppHandle, state: &State<AppState>, path: &str, time: f32) -> Result<(), String> {
    let player = &state.player;
    let eq_settings = state.equalizer.get_settings_handle();
    let mut decoder = SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    decoder.seek(Duration::from_secs_f32(time))?;
    let _ = decoder.prefill_buffer();
    let source: Box<dyn Source<Item = f32> + Send> = Box::new(
        VisualizationSource::new(
            LockFreeSymphoniaSource::new(decoder),
            Arc::clone(&player.waveform_data),
            Arc::clone(&player.spectrum_data),
            Some(app.clone()),
        )
        .with_start_position(time)
        .with_eq_settings(eq_settings)
        .fade_in(Duration::from_millis(50)) // seek 时使用较短的淡入
    );
    {
        let sink = player.sink.lock().unwrap();
        // 直接停止，不做阻塞的淡出
        sink.stop();
        sink.set_volume(*player.target_volume.lock().unwrap());
    }
    let sink = player.sink.lock().unwrap();
    sink.append(source);
    sink.play();
    Ok(())
}

/// 获取播放状态
pub fn get_status(state: &State<AppState>) -> Result<PlaybackStatus, String> {
    // 使用 try_lock 避免阻塞主线程
    let volume = state.player.target_volume.try_lock()
        .map(|g| *g)
        .unwrap_or(1.0);
    
    let is_playing = {
        let exclusive_mode = state.player.exclusive_mode.try_lock()
            .map(|g| *g)
            .unwrap_or(false);
        
        if exclusive_mode {
            #[cfg(windows)]
            {
                state.player.wasapi_player.try_lock()
                    .map(|g| g.as_ref().map_or(false, |wasapi| wasapi.get_state() == PlaybackState::Playing))
                    .unwrap_or(false)
            }
            #[cfg(not(windows))]
            {
                false
            }
        } else {
            state.player.sink.try_lock()
                .map(|sink| !sink.is_paused())
                .unwrap_or(false)
        }
    };
    Ok(PlaybackStatus::new(is_playing, 0.0, volume))
}

/// 检查音轨是否播放完毕
pub fn check_track_finished(state: &State<AppState>) -> Result<bool, String> {
    // 使用 try_lock 避免阻塞主线程
    let exclusive_mode = state.player.exclusive_mode.try_lock()
        .map(|g| *g)
        .unwrap_or(false);
    
    if exclusive_mode {
        #[cfg(windows)]
        {
            Ok(state.player.wasapi_player.try_lock()
                .map(|g| g.as_ref().map_or(true, |wasapi| wasapi.get_state() == PlaybackState::Stopped))
                .unwrap_or(false))
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    } else {
        state.player.sink.try_lock()
            .map(|sink| Ok(sink.empty() && !sink.is_paused()))
            .unwrap_or(Ok(false))
    }
}
