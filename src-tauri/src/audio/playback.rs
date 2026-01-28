//! 音频播放模块
//!
//! 提供音频播放、暂停、恢复、音量控制等功能。
//! 
//! 使用SIMD友好的批量处理
//! 预计算查找表避免热路径上的数学运算
//! 无锁设计减少线程竞争

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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

// ============================================================================
// 预计算查找表 - 避免热路径上的数学运算
// ============================================================================

/// 软削波查找表大小（覆盖 0.0 到 2.0 范围，精度 0.001）
const SOFT_CLIP_TABLE_SIZE: usize = 2001;

/// 预计算的软削波查找表
static SOFT_CLIP_TABLE: std::sync::LazyLock<[f32; SOFT_CLIP_TABLE_SIZE]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; SOFT_CLIP_TABLE_SIZE];
    for (i, item) in table.iter_mut().enumerate() {
        let x = i as f32 / 1000.0; // 0.0 到 2.0
        *item = compute_soft_clip(x);
    }
    table
});

/// 计算软削波值（用于生成查找表）
#[inline]
fn compute_soft_clip(x: f32) -> f32 {
    let threshold = 0.95;
    if x <= threshold {
        x
    } else {
        let over = x - threshold;
        threshold + (1.0 - threshold) * (over / (1.0 - threshold) * 0.5).tanh()
    }
}

/// 快速软削波 - 使用查找表
#[inline(always)]
fn soft_clip_fast(x: f32) -> f32 {
    let sign = x.signum();
    let abs_x = x.abs();
    
    // 快速路径：大多数采样在 [-0.95, 0.95] 范围内
    if abs_x <= 0.95 {
        return x;
    }
    
    // 查表路径
    let index = ((abs_x * 1000.0) as usize).min(SOFT_CLIP_TABLE_SIZE - 1);
    sign * SOFT_CLIP_TABLE[index]
}

/// Preamp 增益查找表（-8dB 到 +8dB，精度 0.1dB）
const PREAMP_TABLE_SIZE: usize = 161;

static PREAMP_TABLE: std::sync::LazyLock<[f32; PREAMP_TABLE_SIZE]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; PREAMP_TABLE_SIZE];
    for (i, item) in table.iter_mut().enumerate() {
        let db = (i as f32 - 80.0) / 10.0; // -8.0 到 +8.0 dB
        *item = 10.0_f32.powf(db / 20.0);
    }
    table
});

/// 快速 dB 到线性增益转换
#[inline(always)]
fn db_to_linear_fast(db: f32) -> f32 {
    let clamped = db.clamp(-8.0, 8.0);
    let index = ((clamped + 8.0) * 10.0) as usize;
    PREAMP_TABLE[index.min(PREAMP_TABLE_SIZE - 1)]
}

/// 批量处理块大小（对齐到 SIMD 友好的边界）
const BATCH_SIZE: usize = 64;

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

/// 频谱更新事件 - 简化结构减少序列化开销
#[derive(Debug, serde::Serialize, Clone)]
pub struct SpectrumUpdateEvent {
    pub data: Vec<f32>,
}

/// 音轨结束事件
#[derive(Debug, serde::Serialize, Clone)]
pub struct TrackEndedEvent {}

#[inline]
fn emit_spectrum_update(app: &AppHandle, data: &[f32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 直接发送数据数组，减少 JSON 包装开销
    app.emit("spectrum-update", SpectrumUpdateEvent { data: data.to_vec() })?;
    Ok(())
}

#[inline]
fn emit_track_ended(app: &AppHandle) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("track-ended", TrackEndedEvent {})?;
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

// ============================================================================
// 批量处理缓冲区 - 减少函数调用开销
// ============================================================================

/// 批量 EQ 处理器 - 一次处理多个采样
struct BatchEqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    states: Vec<Vec<crate::equalizer::BiquadState>>,
    sample_rate: f32,
    channels: usize,
    cached_enabled: bool,
    cached_preamp_multiplier: f32,
}

impl BatchEqProcessor {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            coefficients: vec![crate::equalizer::BiquadCoefficients::default(); EQ_BAND_COUNT],
            states: vec![vec![crate::equalizer::BiquadState::default(); channels as usize]; EQ_BAND_COUNT],
            sample_rate: sample_rate as f32,
            channels: channels as usize,
            cached_enabled: false,
            cached_preamp_multiplier: 1.0,
        }
    }

    /// 更新缓存的设置和滤波器系数
    fn update_settings(&mut self, settings: &EqSettings) {
        self.cached_enabled = settings.enabled;
        // 使用查找表获取 preamp 乘数
        self.cached_preamp_multiplier = db_to_linear_fast(settings.preamp);
        
        if settings.enabled {
            self.update_coefficients(settings);
        }
    }

    fn update_coefficients(&mut self, settings: &EqSettings) {
        use crate::equalizer::{BiquadCoefficients, EQ_FREQUENCIES, EQ_Q_VALUES};
        for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::peaking_eq(self.sample_rate, freq, settings.gains[i], EQ_Q_VALUES[i]);
        }
    }

    /// 批量处理采样（更高效）
    #[inline]
    fn process_batch(&mut self, samples: &mut [f32]) {
        if !self.cached_enabled { return; }
        
        let preamp = self.cached_preamp_multiplier;
        let channels = self.channels;
        
        // 应用 preamp（向量化友好的循环）
        for sample in samples.iter_mut() {
            *sample *= preamp;
        }
        
        // 逐频段处理
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            for (i, sample) in samples.iter_mut().enumerate() {
                let channel = i % channels;
                *sample = self.states[band][channel].process(*sample, coeffs);
            }
        }
        
        // 批量软削波
        for sample in samples.iter_mut() {
            *sample = soft_clip_fast(*sample);
        }
    }
}

pub struct VisualizationSource<I: Source<Item = f32> + Send> {
    input: I,
    #[allow(dead_code)]
    waveform_data: Arc<Mutex<Vec<f32>>>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
    app_handle: Option<AppHandle>,
    last_fft_time: AtomicU64,
    last_position_emit_time: AtomicU64,
    eq_settings: Arc<RwLock<EqSettings>>,
    eq_processor: BatchEqProcessor,
    eq_update_counter: u32,
    fft_buffer: Vec<f32>,
    spectrum_buffer: Vec<f32>,
    samples_played: u64,
    sample_rate: u32,
    channels: u16,
    fft_size: usize,
    // 批量处理缓冲区
    pending_samples: Vec<f32>,
    pending_processed: Vec<f32>,
    pending_index: usize,
    // EOF 标志 - 用于发送 track-ended 事件
    eof_sent: bool,
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
            last_fft_time: AtomicU64::new(0),
            last_position_emit_time: AtomicU64::new(0),
            eq_settings: Arc::new(RwLock::new(EqSettings::default())),
            eq_processor: BatchEqProcessor::new(sr, ch),
            eq_update_counter: 0,
            fft_buffer: vec![0.0; fft_size],
            spectrum_buffer: vec![0.0; 128],
            samples_played: 0,
            sample_rate: sr,
            channels: ch,
            fft_size,
            pending_samples: Vec::with_capacity(BATCH_SIZE),
            pending_processed: Vec::with_capacity(BATCH_SIZE),
            pending_index: 0,
            eof_sent: false,
        }
    }
    
    /// 设置初始播放位置（用于 seek 操作）
    #[must_use]
    pub fn with_start_position(mut self, position_secs: f32) -> Self {
        self.samples_played = (position_secs * self.sample_rate as f32 * self.channels as f32) as u64;
        self
    }

    #[must_use]
    pub fn with_eq_settings(mut self, eq_settings: Arc<RwLock<EqSettings>>) -> Self {
        self.eq_settings = eq_settings;
        if let Ok(s) = self.eq_settings.read() {
            self.eq_processor.update_settings(&s);
        }
        self
    }
    
    /// 批量从输入源读取采样并处理
    #[inline]
    fn refill_batch(&mut self) -> bool {
        self.pending_samples.clear();
        self.pending_index = 0;
        
        // 批量读取
        for _ in 0..BATCH_SIZE {
            if let Some(sample) = self.input.next() {
                self.pending_samples.push(sample);
            } else {
                break;
            }
        }
        
        if self.pending_samples.is_empty() {
            return false;
        }
        
        // 更新 EQ 设置（每批次检查一次，而不是每 512 采样）
        self.eq_update_counter += 1;
        if self.eq_update_counter >= 8 { // 每 8 批次 = 512 采样
            self.eq_update_counter = 0;
            if let Ok(s) = self.eq_settings.try_read() {
                self.eq_processor.update_settings(&s);
            }
        }
        
        // 批量 EQ 处理
        self.pending_processed = self.pending_samples.clone();
        self.eq_processor.process_batch(&mut self.pending_processed);
        
        true
    }
}

impl<I: Source<Item = f32> + Send> Iterator for VisualizationSource<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // 从批量处理缓冲区获取采样
        if self.pending_index >= self.pending_processed.len() {
            if !self.refill_batch() {
                // EOF - 发送 track-ended 事件（只发送一次）
                if !self.eof_sent {
                    self.eof_sent = true;
                    if let Some(ref app) = self.app_handle {
                        let _ = emit_track_ended(app);
                    }
                }
                return None;
            }
        }
        
        let processed = self.pending_processed[self.pending_index];
        self.pending_index += 1;
        self.samples_played += 1;
        
        // 添加到可视化缓冲区
        self.buffer.push(processed);
        
        // FFT 和事件发送逻辑（仅在缓冲区满时执行）
        if self.buffer.len() >= self.fft_size {
            self.process_visualization();
        }
        
        Some(processed)
    }
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    /// 处理可视化数据（FFT 和事件发送）
    #[inline(never)] // 避免内联到热路径
    fn process_visualization(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        // 发送播放位置（每 100ms 一次）
        let last_pos_emit = self.last_position_emit_time.load(Ordering::Relaxed);
        if now - last_pos_emit >= 100 {
            self.last_position_emit_time.store(now, Ordering::Relaxed);
            if let Some(ref app) = self.app_handle {
                let position = self.samples_played as f32 / (self.sample_rate as f32 * self.channels as f32);
                let _ = emit_playback_position(app, position);
            }
        }
        
        let last_fft = self.last_fft_time.load(Ordering::Relaxed);
        
        // 限制 FFT 计算和发送频率为约 60fps (16ms)
        if now - last_fft >= 16 {
            self.last_fft_time.store(now, Ordering::Relaxed);
            self.compute_spectrum();
        }
        
        // 保留后半部分数据用于重叠分析
        let half = self.buffer.len() / 2;
        self.buffer.drain(..half);
    }
    
    /// 计算频谱数据
    #[inline(never)]
    fn compute_spectrum(&mut self) {
        if let Ok(mut spec) = self.spectrum_data.try_lock() {
            // 复用预分配的缓冲区
            self.fft_buffer[..self.fft_size].copy_from_slice(&self.buffer[..self.fft_size]);
            let hann = hann_window(&self.fft_buffer);
            
            if let Ok(spectrum) = samples_fft_to_spectrum(
                &hann,
                self.sample_rate,
                FrequencyLimit::Range(20.0, 20000.0),
                Some(&divide_by_N_sqrt),
            ) {
                // 重置频谱缓冲区
                self.spectrum_buffer.fill(0.0);
                
                // AE 风格：线性频率分布
                const NUM_BINS: usize = 128;
                const FREQ_MIN: f32 = 20.0;
                const FREQ_MAX: f32 = 16000.0;
                const FREQ_STEP: f32 = (FREQ_MAX - FREQ_MIN) / NUM_BINS as f32;
                
                for (freq, value) in spectrum.data() {
                    let f = freq.val();
                    if !(FREQ_MIN..=FREQ_MAX).contains(&f) { continue; }
                    
                    let bin = ((f - FREQ_MIN) / FREQ_STEP).floor() as usize;
                    let bin = bin.min(NUM_BINS - 1);
                    
                    let v = value.val();
                    if v > self.spectrum_buffer[bin] {
                        self.spectrum_buffer[bin] = v;
                    }
                }
                
                // AE 风格的平滑：快速上升，缓慢下降
                for i in 0..128 {
                    let target = self.spectrum_buffer[i];
                    let current = self.prev_spectrum[i];
                    
                    self.prev_spectrum[i] = if target > current {
                        current * 0.3 + target * 0.7 // 快速上升
                    } else {
                        current * 0.85 + target * 0.15 // 缓慢下降
                    };
                }
                
                spec.clear();
                spec.extend_from_slice(&self.prev_spectrum);
            }
        }
        
        // 发送事件 - 与 FFT 计算同步，不再单独节流
        if let Some(ref app) = self.app_handle {
            let _ = emit_spectrum_update(app, &self.prev_spectrum);
        }
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
            println!("Symphonia decoder: {path}");
            Box::new(
                VisualizationSource::new(LockFreeSymphoniaSource::new(dec), waveform, spectrum, Some(app.clone()))
                    .with_start_position(start_pos)
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(80)) // 稍长的淡入来补偿没有淡出
            )
        }
        Err(e) => {
            println!("Symphonia decoder failed, fallback to rodio: {e}");
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
    let start_pos = position.unwrap_or(0.0);
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
            decode_and_push_to_wasapi(source, wasapi_clone, waveform, spectrum, app_clone, stop_flag, thread_id, new_thread_id, src_sr, src_ch, target_sr, target_ch, eq_settings, start_pos)
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
struct EqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    states: Vec<Vec<crate::equalizer::BiquadState>>,
    sample_rate: f32,
    #[allow(dead_code)]
    channels: usize,
    cached_enabled: bool,
    cached_preamp_multiplier: f32,
}

#[cfg(windows)]
impl EqProcessor {
    fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            coefficients: vec![crate::equalizer::BiquadCoefficients::default(); EQ_BAND_COUNT],
            states: vec![vec![crate::equalizer::BiquadState::default(); channels as usize]; EQ_BAND_COUNT],
            sample_rate: sample_rate as f32,
            channels: channels as usize,
            cached_enabled: false,
            cached_preamp_multiplier: 1.0,
        }
    }

    fn update_settings(&mut self, settings: &EqSettings) {
        self.cached_enabled = settings.enabled;
        self.cached_preamp_multiplier = 10.0_f32.powf(settings.preamp / 20.0);
        
        if settings.enabled {
            self.update_coefficients(settings);
        }
    }

    fn update_coefficients(&mut self, settings: &EqSettings) {
        use crate::equalizer::{BiquadCoefficients, EQ_FREQUENCIES, EQ_Q_VALUES};
        for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::peaking_eq(self.sample_rate, freq, settings.gains[i], EQ_Q_VALUES[i]);
        }
    }

    #[inline(always)]
    fn process_sample_cached(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled { return input; }
        let mut sample = input * self.cached_preamp_multiplier;
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            sample = self.states[band][channel].process(sample, coeffs);
        }
        soft_clip_fast(sample)
    }

    #[inline(always)]
    const fn is_enabled(&self) -> bool {
        self.cached_enabled
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
    thread_id_ref: Arc<AtomicU64>,
    my_id: u64,
    src_sr: u32,
    src_ch: u16,
    target_sr: u32,
    target_ch: u16,
    eq_settings: Arc<RwLock<EqSettings>>,
    start_position: f32,
) {
    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id { return; }

    let mut eq_proc = EqProcessor::new(src_sr, src_ch);
    if let Ok(settings) = eq_settings.read() {
        eq_proc.update_settings(&settings);
    }
    let need_resample = src_sr != target_sr;
    let chunk_size = calculate_decode_chunk_size(src_sr);
    let mut eq_update_counter: u32 = 0;
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
    
    // 播放位置追踪
    let mut last_position_emit_time: u64 = 0;
    
    // 发送播放位置的闭包
    let emit_position = |last_time: &mut u64| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if now - *last_time >= 100 {
            *last_time = now;
            let samples_played = wasapi.lock().unwrap()
                .as_ref()
                .map_or(0, |p| p.get_samples_written());
            let position = start_position + samples_played as f32 / (target_sr as f32 * target_ch as f32);
            let _ = emit_playback_position(&app, position);
        }
    };

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
            for ch in 0..src_ch as usize {
                for s in &mut input_frames[ch] {
                    *s = eq_proc.process_sample_cached(*s, ch);
                }
            }
        }

        let output_frames: Vec<Vec<f32>> = if let Some(ref mut r) = resampler {
            let actual = input_frames[0].len();
            if actual < chunk_size {
                for ch in &mut input_frames {
                    let last_sample = ch.last().copied().unwrap_or(0.0);
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
            // 等待缓冲区有空间（防止解码过快导致内存无限增长）
            loop {
                if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }
                let buf_size = wasapi.lock().unwrap().as_ref().map_or(0, |p| p.get_buffer_size());
                // 缓冲区容量约为 target_sr * target_ch * 4秒，保持在2秒以下
                let max_buffer = target_sr as usize * target_ch as usize * 2;
                if buf_size < max_buffer { break; }
                // 等待时继续发送播放位置
                emit_position(&mut last_position_emit_time);
                std::thread::sleep(Duration::from_millis(10));
            }
            if stop_flag.load(Ordering::SeqCst) || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }
            
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
