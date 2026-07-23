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
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// 获取锁,poison 错误时记录日志并返回内部数据(而非 panic)
macro_rules! lock_or_log {
    ($mutex:expr) => {
        match $mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("Mutex poisoned, recovering: {}", poisoned);
                poisoned.into_inner()
            }
        }
    };
}

// ============================================================================
// 预计算查找表
// ============================================================================

/// 软削波查找表大小（覆盖0.0到2.0范围，精度0.001）
const SOFT_CLIP_TABLE_SIZE: usize = 2001;

/// 预计算的软削波查找表
static SOFT_CLIP_TABLE: std::sync::LazyLock<[f32; SOFT_CLIP_TABLE_SIZE]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; SOFT_CLIP_TABLE_SIZE];
    for (i, item) in table.iter_mut().enumerate() {
        let x = i as f32 / 1000.0; // 0.0到2.0
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
    
    // 快速路径：大多数采样在[-0.95, 0.95]范围内
    if abs_x <= 0.95 {
        return x;
    }
    
    // 查表路径
    let index = ((abs_x * 1000.0) as usize).min(SOFT_CLIP_TABLE_SIZE - 1);
    sign * SOFT_CLIP_TABLE[index]
}

/// Preamp增益查找表（-8dB到+8dB，精度0.1dB）
const PREAMP_TABLE_SIZE: usize = 161;

static PREAMP_TABLE: std::sync::LazyLock<[f32; PREAMP_TABLE_SIZE]> = std::sync::LazyLock::new(|| {
    let mut table = [0.0f32; PREAMP_TABLE_SIZE];
    for (i, item) in table.iter_mut().enumerate() {
        let db = (i as f32 - 80.0) / 10.0; // -8.0 到 +8.0 dB
        *item = 10.0_f32.powf(db / 20.0);
    }
    table
});

/// 快速dB到线性增益转换
#[inline(always)]
fn db_to_linear_fast(db: f32) -> f32 {
    let clamped = db.clamp(-8.0, 8.0);
    let index = ((clamped + 8.0) * 10.0) as usize;
    PREAMP_TABLE[index.min(PREAMP_TABLE_SIZE - 1)]
}

/// 批量处理块大小（对齐到SIMD友好的边界）
const BATCH_SIZE: usize = 64;

/// 预计算 Hann 窗口(避免每次 FFT 都堆分配)
/// 公式: hann[i] = 0.5 - 0.5 * cos(2π * i / N)
fn precompute_hann_window(size: usize) -> Vec<f32> {
    let n = size as f32;
    (0..size).map(|i| {
        let angle = 2.0 * std::f32::consts::PI * (i as f32) / n;
        0.5 - 0.5 * angle.cos()
    }).collect()
}

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
    // 直接发送数据数组，减少JSON包装开销
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
// 批量处理缓冲区
// ============================================================================

/// 批量EQ处理器
///
/// 注意:本 EQ 处理器与 EqProcessor(独占模式)是两套独立实现。
/// 修改 EQ 算法(频段增益、Q 因子、滤波器类型)时必须同步修改另一处。
///
/// 性能优化要点:
/// 1. states 采用扁平布局 `[channel][band]` (channel × EQ_BAND_COUNT),避免双层 Vec 解引用
/// 2. 三个处理阶段(preamp/biquad/soft_clip)合并为单次循环,提升 cache 局部性
/// 3. i % channels 在 channels=2 时编译器会优化为位运算,无需手动展开
struct BatchEqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    /// 扁平布局: states[channel * EQ_BAND_COUNT + band]
    /// 这样同一 channel 的所有 band 状态在内存中连续,提升 cache 命中率
    states: Vec<crate::equalizer::BiquadState>,
    sample_rate: f32,
    channels: usize,
    cached_enabled: bool,
    cached_preamp_multiplier: f32,
}

impl BatchEqProcessor {
    fn new(sample_rate: u32, channels: u16) -> Self {
        let channels = channels as usize;
        Self {
            coefficients: vec![crate::equalizer::BiquadCoefficients::default(); EQ_BAND_COUNT],
            // 扁平数组: channels × EQ_BAND_COUNT,一次性分配,提升 cache 局部性
            states: vec![crate::equalizer::BiquadState::default(); channels * EQ_BAND_COUNT],
            sample_rate: sample_rate as f32,
            channels,
            cached_enabled: false,
            cached_preamp_multiplier: 1.0,
        }
    }

    /// 更新缓存的设置和滤波器系数
    fn update_settings(&mut self, settings: &EqSettings) {
        self.cached_enabled = settings.enabled;
        // 使用查找表获取preamp乘数
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

    /// 批量处理采样 - 合并三阶段循环为单次遍历,提升 cache 局部性
    #[inline]
    fn process_batch(&mut self, samples: &mut [f32]) {
        if !self.cached_enabled { return; }

        let preamp = self.cached_preamp_multiplier;
        let channels = self.channels;
        let band_count = EQ_BAND_COUNT;

        // 单次循环完成 preamp + biquad + soft_clip,提升 cache 局部性
        // 注: i % channels 在 channels=2 时编译器优化为 i & 1,无取模开销
        for (i, sample) in samples.iter_mut().enumerate() {
            let channel = i % channels;
            let state_base = channel * band_count;

            // 1. preamp
            *sample *= preamp;

            // 2. 逐频段 biquad 处理(使用扁平 states 数组,连续内存访问)
            for band in 0..band_count {
                let coeffs = &self.coefficients[band];
                let state = &mut self.states[state_base + band];
                *sample = state.process(*sample, coeffs);
            }

            // 3. 软削波
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
    /// 预计算的 Hann 窗口(按 fft_size 一次预计算,避免每次 FFT 堆分配)
    hann_window: Vec<f32>,
    spectrum_buffer: Vec<f32>,
    samples_played: u64,
    sample_rate: u32,
    channels: u16,
    fft_size: usize,
    // 批量处理缓冲区:直接存 EQ 处理后的采样,避免原始采样的中间拷贝
    pending_processed: Vec<f32>,
    pending_index: usize,
    // EOF标志 - 用于发送track-ended事件
    eof_sent: bool,
    /// 目标刷新率（用于FFT计算频率）
    target_fps: Arc<AtomicU64>,
    /// 是否启用垂直同步（启用后FFT与屏幕刷新率同步）
    enable_vertical_sync: Arc<AtomicBool>,
}

/// 根据采样率计算最佳FFT缓冲区大小
/// 目标是保持约~43ms的分析窗口（2048@48kHz）
#[must_use]
const fn calculate_fft_size(sample_rate: u32) -> usize {
    // 基准：48kHz使用2048样本 ≈ 42.7ms
    // 公式：fft_size = sample_rate * 0.0427
    // FFT大小必须是2的幂次
    match sample_rate {
        0..=32000 => 1024,      // ≤32kHz: 1024 样本
        32001..=64000 => 2048,  // 44.1k/48k: 2048 样本
        64001..=128_000 => 4096, // 88.2k/96k: 4096 样本
        _ => 8192,              // 176.4k/192k/384k: 8192 样本
    }
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    pub fn new(input: I, waveform_data: Arc<Mutex<Vec<f32>>>, spectrum_data: Arc<Mutex<Vec<f32>>>, app_handle: Option<AppHandle>, target_fps: Arc<AtomicU64>, enable_vertical_sync: Arc<AtomicBool>) -> Self {
        let (sr, ch) = (input.sample_rate().get(), input.channels().get());
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
            hann_window: precompute_hann_window(fft_size),
            spectrum_buffer: vec![0.0; 128],
            samples_played: 0,
            sample_rate: sr,
            channels: ch,
            fft_size,
            pending_processed: Vec::with_capacity(BATCH_SIZE),
            pending_index: 0,
            eof_sent: false,
            target_fps,
            enable_vertical_sync,
        }
    }
    
    /// 设置初始播放位置（用于seek操作）
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
        self.pending_processed.clear();
        self.pending_index = 0;

        // 批量读取 - 直接写入 pending_processed,避免中间 clone
        for _ in 0..BATCH_SIZE {
            if let Some(sample) = self.input.next() {
                self.pending_processed.push(sample);
            } else {
                break;
            }
        }

        if self.pending_processed.is_empty() {
            return false;
        }

        // 更新EQ设置（每批次检查一次，而不是每512采样）
        self.eq_update_counter += 1;
        if self.eq_update_counter >= 8 { // 每8批次 = 512采样
            self.eq_update_counter = 0;
            if let Ok(s) = self.eq_settings.try_read() {
                self.eq_processor.update_settings(&s);
            }
        }

        // 批量EQ处理(原地处理,无 clone)
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
        
        // FFT和事件发送逻辑（仅在缓冲区满时执行）
        if self.buffer.len() >= self.fft_size {
            self.process_visualization();
        }
        
        Some(processed)
    }
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    /// 处理可视化数据（FFT和事件发送）
    #[inline(never)] // 避免内联到热路径
    fn process_visualization(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        // 发送播放位置（每100ms一次）
        let last_pos_emit = self.last_position_emit_time.load(Ordering::Relaxed);
        if now - last_pos_emit >= 100 {
            self.last_position_emit_time.store(now, Ordering::Relaxed);
            if let Some(ref app) = self.app_handle {
                let position = self.samples_played as f32 / (self.sample_rate as f32 * self.channels as f32);
                let _ = emit_playback_position(app, position);
            }
        }
        
        let last_fft = self.last_fft_time.load(Ordering::Relaxed);
        
        // 根据垂直同步设置决定FFT频率
        let enable_vsync = self.enable_vertical_sync.load(Ordering::Relaxed);
        let target_fps = self.target_fps.load(Ordering::Relaxed).max(1);
        // 当前实现中垂直同步和目标帧率使用相同算法(1000/fps)
        // 保留 enable_vsync 标志供未来扩展真实屏幕刷新率读取
        let _ = enable_vsync;
        let fft_interval_ms = 1000 / target_fps;
        
        // 限制FFT计算和发送频率
        if now - last_fft >= fft_interval_ms {
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
            // 手动应用预计算的 Hann 窗口(避免 hann_window() 每次堆分配 Vec)
            for i in 0..self.fft_size {
                self.fft_buffer[i] *= self.hann_window[i];
            }

            if let Ok(spectrum) = samples_fft_to_spectrum(
                &self.fft_buffer,
                self.sample_rate,
                FrequencyLimit::Range(20.0, 20000.0),
                Some(&divide_by_N_sqrt),
            ) {
                // 重置频谱缓冲区
                self.spectrum_buffer.fill(0.0);
                
                // AE风格：线性频率分布
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
                
                // AE风格的平滑：快速上升，缓慢下降
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
        
        // 发送事件 - 与FFT计算同步，不再单独节流
        if let Some(ref app) = self.app_handle {
            let _ = emit_spectrum_update(app, &self.prev_spectrum);
        }
    }
}

impl<I: Source<Item = f32> + Send> Source for VisualizationSource<I> {
    fn current_span_len(&self) -> Option<usize> { self.input.current_span_len() }
    fn channels(&self) -> std::num::NonZero<u16> { self.input.channels() }
    fn sample_rate(&self) -> std::num::NonZero<u32> { self.input.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

/// 播放音轨（共享模式）
pub fn play_track_shared(app: &AppHandle, state: &State<AppState>, path: &str, position: Option<f32>) -> Result<(), String> {
    let player = &state.player;
    // 取消任何正在进行的淡入淡出,防止其 on_complete(pause) 在新歌播放后执行
    player.fade.generation.fetch_add(1, Ordering::SeqCst);
    // 先读取 target_volume 再锁 sink,避免嵌套锁死锁风险
    let vol = *lock_or_log!(player.output.target_volume);
    {
        let player_lock = lock_or_log!(player.output.sink);
        // 直接停止，不做淡出（淡出会阻塞主线程）
        // 新音源会有fade_in效果来平滑过渡
        player_lock.stop();
        player_lock.set_volume(vol);
    }
    *lock_or_log!(player.track.current_path) = Some(path.to_string());
    *lock_or_log!(player.track.current_source) = None;
    let (waveform, spectrum, eq_settings, target_fps, enable_vertical_sync) = (
        Arc::clone(&player.visualization.waveform_data),
        Arc::clone(&player.visualization.spectrum_data),
        state.equalizer.get_settings_handle(),
        Arc::clone(&player.visualization.target_fps),
        Arc::clone(&player.visualization.enable_vertical_sync),
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
                VisualizationSource::new(LockFreeSymphoniaSource::new(dec), waveform, spectrum, Some(app.clone()), target_fps, enable_vertical_sync)
                    .with_start_position(start_pos)
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(80)) // 稍长的淡入来补偿没有淡出
            )
        }
        Err(e) => {
            log::warn!("Symphonia decoder failed, fallback to rodio: {e}");
            let file = File::open(path).map_err(|e| e.to_string())?;
            Box::new(
                VisualizationSource::new(rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?, waveform, spectrum, Some(app.clone()), target_fps, enable_vertical_sync)
                    .with_start_position(position.unwrap_or(0.0))
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(80))
            )
        }
    };

    // 获取 mixer 输出配置，手动重采样到 mixer 的采样率
    // rodio 0.22 的 UniformSourceIterator 在 queue keep_alive 模式下，
    // 当 source.current_span_len() 返回 None 时不会重新 bootstrap SampleRateConverter，
    // 导致高采样率音频以错误的速率播放（降速）。
    // 解决方案：在 append 之前手动将 source 重采样到 mixer 的采样率。
    let resampled: Box<dyn Source<Item = f32> + Send> = {
        let stream_guard = lock_or_log!(player.output.output_stream);
        if let Some(ref mixer_sink) = *stream_guard {
            let mixer_sr = mixer_sink.config().sample_rate();
            let mixer_ch = mixer_sink.config().channel_count();
            #[cfg(debug_assertions)]
            let source_sr = source.sample_rate();
            #[cfg(debug_assertions)]
            println!("Source sample_rate: {source_sr}, Mixer sample_rate: {mixer_sr}, Mixer channels: {mixer_ch}");
            Box::new(rodio::source::UniformSourceIterator::new(source, mixer_ch, mixer_sr))
        } else {
            log::warn!("No output stream available, skipping manual resample");
            source
        }
    };

    let player_lock = lock_or_log!(player.output.sink);
    player_lock.append(resampled);
    player_lock.play();
    Ok(())
}

/// 播放音轨（独占模式）
#[cfg(windows)]
pub fn play_track_exclusive(app: &AppHandle, state: &State<AppState>, path: &str, position: Option<f32>) -> Result<(), String> {
    let player = &state.player;
    // 递增代际计数器取消旧解码推送线程(替代 stop 布尔标志,避免 70ms 窗口内状态不一致)
    player.decode.generation.fetch_add(1, Ordering::SeqCst);
    let new_thread_id = player.decode.id.fetch_add(1, Ordering::SeqCst) + 1;
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player) {
            // 切歌淡出:50ms 平滑过渡到静音,消除 audible click
            // 音频线程内部完成淡出后会自动 stop_stream + clear_buffer
            // fade 禁用时直接 stop + clear_buffer
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
    let wait_ms = if player.fade.enabled.load(Ordering::SeqCst) { 70 } else { 50 };
    std::thread::sleep(Duration::from_millis(wait_ms));
    // 兜底:确保缓冲区被清空(防止淡出未完成的极端情况)
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player) {
            let _ = wasapi.clear_buffer();
        }
    }

    let (target_sr, target_ch) = {
        let g = lock_or_log!(player.output.wasapi_player);
        let wasapi = g.as_ref().ok_or("WASAPI player not initialized")?;
        (wasapi.get_sample_rate(), wasapi.get_channels())
    };
    if position.is_none() {
        *lock_or_log!(player.track.current_path) = Some(path.to_string());
    }
    log::info!("WASAPI Exclusive: {path} @ {target_sr}Hz, {target_ch} ch");

    let mut decoder = SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    if let Some(t) = position {
        if let Err(e) = decoder.seek(Duration::from_secs_f32(t)) {
            log::warn!("Seek failed for track, starting from beginning: {e}");
        }
    }
    let _ = decoder.prefill_buffer();
    let (src_sr, src_ch) = (decoder.sample_rate(), decoder.channels());
    log::debug!("Source: {src_sr}Hz, {src_ch} ch -> Target: {target_sr}Hz, {target_ch} ch");

    let source = LockFreeSymphoniaSource::new(decoder);
    let start_pos = position.unwrap_or(0.0);
    let (wasapi_clone, waveform, spectrum, generation, thread_id, eq_settings) = (
        Arc::clone(&player.output.wasapi_player),
        Arc::clone(&player.visualization.waveform_data),
        Arc::clone(&player.visualization.spectrum_data),
        Arc::clone(&player.decode.generation),
        Arc::clone(&player.decode.id),
        state.equalizer.get_settings_handle(),
    );
    let app_clone = app.clone();
    let thread_started = Arc::new(AtomicBool::new(false));
    let thread_started_clone = Arc::clone(&thread_started);

    std::thread::spawn(move || {
        thread_started_clone.store(true, Ordering::SeqCst);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_and_push_to_wasapi(source, wasapi_clone, waveform, spectrum, app_clone, generation, thread_id, new_thread_id, src_sr, src_ch.get(), target_sr, target_ch, eq_settings, start_pos);
        }));
    });

    // 等待解码线程启动
    let mut wait = 0;
    while !thread_started.load(Ordering::SeqCst) && wait < 20 {
        std::thread::sleep(Duration::from_millis(5));
        wait += 1;
    }

    // 等待缓冲区有足够数据再开始播放，避免音频开头欠载
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player) {
            // 等待至少200ms的音频数据（约 1/5 秒）
            let min_buffer_samples = target_sr as usize * target_ch as usize / 5;
            let mut buffer_wait = 0;
            while wasapi.get_buffer_size() < min_buffer_samples && buffer_wait < 50 {
                std::thread::sleep(Duration::from_millis(10));
                buffer_wait += 1;
            }
            // 额外等待一小段时间确保数据稳定
            std::thread::sleep(Duration::from_millis(20));
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

/// 根据采样率计算解码chunk 大小
/// 目标是保持约~21ms的处理块（1024@48kHz）
#[must_use]
const fn calculate_decode_chunk_size(sample_rate: u32) -> usize {
    match sample_rate {
        0..=32000 => 512,       // ≤32kHz
        32001..=64000 => 1024,  // 44.1k/48k
        64001..=128_000 => 2048, // 88.2k/96k
        _ => 4096,              // 176.4k/192k/384k
    }
}

/// 注意:本 EQ 处理器与 BatchEqProcessor(共享模式)是两套独立实现。
/// 修改 EQ 算法(频段增益、Q 因子、滤波器类型)时必须同步修改另一处。
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
    generation: Arc<AtomicU64>,
    thread_id_ref: Arc<AtomicU64>,
    my_id: u64,
    src_sr: u32,
    src_ch: u16,
    target_sr: u32,
    target_ch: u16,
    eq_settings: Arc<RwLock<EqSettings>>,
    start_position: f32,
) {
    use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};
    use audioadapter_buffers::direct::SequentialSliceOfVecs;
    // 记录启动时的代际,循环中检测代际变化即退出(替代 stop 布尔标志)
    let my_generation = generation.load(Ordering::SeqCst);
    if generation.load(Ordering::SeqCst) != my_generation || thread_id_ref.load(Ordering::SeqCst) != my_id { return; }

    let mut eq_proc = EqProcessor::new(src_sr, src_ch);
    if let Ok(settings) = eq_settings.read() {
        eq_proc.update_settings(&settings);
    }
    let need_resample = src_sr != target_sr;
    let chunk_size = calculate_decode_chunk_size(src_sr);
    let resample_ratio = target_sr as f64 / src_sr as f64;
    let mut eq_update_counter: u32 = 0;
    let mut resampler: Option<Async<f32>> = if need_resample {
        // rubato 4.0: 用 builder 模式构造 SincInterpolationParameters
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
        ).ok()
    } else { None };

    let mut input_frames: Vec<Vec<f32>> = vec![Vec::with_capacity(chunk_size); src_ch as usize];
    // 精确计算最大输出缓冲区大小
    let max_output_frames = ((chunk_size as f64 * resample_ratio).ceil() as usize).max(chunk_size);
    let mut output_buffer: Vec<f32> = Vec::with_capacity(max_output_frames * target_ch as usize);
    // rubato 4.0: 预分配输出帧 buffer (复用,避免每次循环堆分配)
    // 每通道预留 max_output_frames + chunk_size 作为安全余量 (rubato 启动延迟可能导致首帧输出更多)
    let output_frames_capacity = max_output_frames + chunk_size;
    let mut output_frames_resampled: Vec<Vec<f32>> = vec![Vec::with_capacity(output_frames_capacity); src_ch as usize];
    // 复用 interleaved 缓冲区,避免每帧堆分配
    let samples_needed = chunk_size * src_ch as usize;
    let mut interleaved: Vec<f32> = Vec::with_capacity(samples_needed);
    // 通道转换用复用缓冲区
    let mut converted_buffer: Vec<f32> = Vec::with_capacity(max_output_frames * target_ch as usize);
    
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
            let samples_played = lock_or_log!(wasapi)
                .as_ref()
                .map_or(0, |p| p.get_samples_written());
            let position = start_position + samples_played as f32 / (target_sr as f32 * target_ch as f32);
            let _ = emit_playback_position(&app, position);
        }
    };

    loop {
        if generation.load(Ordering::SeqCst) != my_generation || thread_id_ref.load(Ordering::SeqCst) != my_id || lock_or_log!(wasapi).is_none() { break; }
        for ch in &mut input_frames { ch.clear(); }

        // 复用 interleaved 缓冲区
        interleaved.clear();
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
                // rubato 4.0: 用 SequentialSliceOfVecs adapter 包装输入输出
                match SequentialSliceOfVecs::new(&input_frames, src_ch as usize, chunk_size) {
                    Ok(input_adapter) => {
                        // 清空并预分配输出 buffer
                        for ch in &mut output_frames_resampled { ch.clear(); ch.resize(output_frames_capacity, 0.0); }
                        match SequentialSliceOfVecs::new_mut(&mut output_frames_resampled, src_ch as usize, output_frames_capacity) {
                            Ok(mut output_adapter) => {
                                let indexing = Indexing::new();
                                match r.process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing)) {
                                    Ok((_in_used, out_written)) => {
                                        for ch in &mut output_frames_resampled { ch.truncate(out_written); }
                                        Cow::Owned(output_frames_resampled.clone())
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
                // rubato 4.0: 正常路径
                let frames_in = input_frames[0].len();
                match SequentialSliceOfVecs::new(&input_frames, src_ch as usize, frames_in) {
                    Ok(input_adapter) => {
                        for ch in &mut output_frames_resampled { ch.clear(); ch.resize(output_frames_capacity, 0.0); }
                        match SequentialSliceOfVecs::new_mut(&mut output_frames_resampled, src_ch as usize, output_frames_capacity) {
                            Ok(mut output_adapter) => {
                                let indexing = Indexing::new();
                                match r.process_into_buffer(&input_adapter, &mut output_adapter, Some(&indexing)) {
                                    Ok((_in_used, out_written)) => {
                                        for ch in &mut output_frames_resampled { ch.truncate(out_written); }
                                        Cow::Owned(output_frames_resampled.clone())
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
            // 等待缓冲区有空间 (Condvar 等待,被 WASAPI 消费线程唤醒)
            // 缓冲区容量约为 target_sr * target_ch * 4 秒,保持在 2 秒以下
            let max_buffer = target_sr as usize * target_ch as usize * 2;
            loop {
                if generation.load(Ordering::SeqCst) != my_generation || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }
                // 用 condvar 等待 50ms 超时,期间 WASAPI 消费端 notify 会唤醒本线程
                let has_space = lock_or_log!(wasapi).as_ref().is_none_or(|p| p.wait_for_buffer_space(max_buffer, Duration::from_millis(50)));
                if has_space { break; }
                // 等待时继续发送播放位置
                emit_position(&mut last_position_emit_time);
            }
            if generation.load(Ordering::SeqCst) != my_generation || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }

            if let Some(ref p) = *lock_or_log!(wasapi) {
                if p.push_samples(final_out).is_err() { break; }
            }
        }

        if eof && interleaved.len() < samples_needed {
            loop {
                if generation.load(Ordering::SeqCst) != my_generation || thread_id_ref.load(Ordering::SeqCst) != my_id { break; }
                let buf_size = lock_or_log!(wasapi).as_ref().map_or(0, |p| p.get_buffer_size());
                if buf_size == 0 { break; }
                std::thread::sleep(Duration::from_millis(50));
            }
            if generation.load(Ordering::SeqCst) == my_generation && thread_id_ref.load(Ordering::SeqCst) == my_id {
                if let Some(ref p) = *lock_or_log!(wasapi) { let _ = p.stop(); }
                let _ = emit_track_ended(&app);
            }
            break;
        }
        // 让出 CPU 给其他线程 (主要给消费线程),避免 100% 占用
        // 但用更短的时间,因为已经被 condvar 同步过
        std::thread::yield_now();
    }
}

/// 5.1/7.1环绕声到立体声的专业混音
/// 使用ITU-R BS.775-1标准的下混系数
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

/// 通道转换(in-place 版本):写入预分配缓冲区,避免每帧堆分配。
/// out 会被 clear 并填充结果。
fn convert_channels_into(samples: &[f32], src_ch: u16, target_ch: u16, out: &mut Vec<f32>) {
    out.clear();
    if src_ch == target_ch {
        out.extend_from_slice(samples);
        return;
    }
    let (src, tgt) = (src_ch as usize, target_ch as usize);
    let frames = samples.len() / src;
    out.reserve(frames * tgt);

    for f in 0..frames {
        let start = f * src;
        match (src, tgt) {
            (1, 2) => {
                let s = samples[start];
                out.push(s);
                out.push(s);
            }
            (2, 1) => {
                out.push(f32::midpoint(samples[start], samples[start + 1]));
            }
            (6 | 8, 2) => {
                // 5.1/7.1到立体声的专业混音
                let (left, right) = downmix_surround_to_stereo(samples, src, f);
                out.push(left);
                out.push(right);
            }
            _ => {
                // 其他情况:简单截取或填充
                for ch in 0..tgt {
                    out.push(if ch < src { samples[start + ch] } else { samples[start] });
                }
            }
        }
    }
}

/// Seek共享模式
pub fn seek_track_shared(app: &AppHandle, state: &State<AppState>, path: &str, time: f32) -> Result<(), String> {
    let player = &state.player;
    // 取消任何正在进行的淡入淡出,防止其 on_complete(pause) 在 seek 后执行
    player.fade.generation.fetch_add(1, Ordering::SeqCst);
    let eq_settings = state.equalizer.get_settings_handle();
    let mut decoder = SymphoniaDecoder::new(path).map_err(|e| format!("Failed to create decoder: {e}"))?;
    decoder.seek(Duration::from_secs_f32(time))?;
    let _ = decoder.prefill_buffer();
    let source: Box<dyn Source<Item = f32> + Send> = Box::new(
        VisualizationSource::new(
            LockFreeSymphoniaSource::new(decoder),
            Arc::clone(&player.visualization.waveform_data),
            Arc::clone(&player.visualization.spectrum_data),
            Some(app.clone()),
            Arc::clone(&player.visualization.target_fps),
            Arc::clone(&player.visualization.enable_vertical_sync),
        )
        .with_start_position(time)
        .with_eq_settings(eq_settings)
        .fade_in(Duration::from_millis(50)) // seek时使用较短的淡入
    );

    // 手动重采样到 mixer 采样率（同 play_track_shared 的修复）
    let resampled: Box<dyn Source<Item = f32> + Send> = {
        let stream_guard = lock_or_log!(player.output.output_stream);
        if let Some(ref mixer_sink) = *stream_guard {
            let mixer_sr = mixer_sink.config().sample_rate();
            let mixer_ch = mixer_sink.config().channel_count();
            Box::new(rodio::source::UniformSourceIterator::new(source, mixer_ch, mixer_sr))
        } else {
            source
        }
    };

    {
        let sink = lock_or_log!(player.output.sink);
        // 直接停止，不做阻塞的淡出
        sink.stop();
        sink.set_volume(*lock_or_log!(player.output.target_volume));
    }
    let sink = lock_or_log!(player.output.sink);
    sink.append(resampled);
    sink.play();
    Ok(())
}

/// 获取播放状态
pub fn get_status(state: &State<AppState>) -> Result<PlaybackStatus, String> {
    let volume = state
        .player
        .output
        .target_volume
        .try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire target volume lock".to_string())?;

    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    let is_playing = if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .try_lock()
                .map_err(|_| "Failed to acquire WASAPI player lock".to_string())?;
            guard
                .as_ref()
                // Stopping/Pausing 期间音频仍在淡出(可听见),视为正在播放
                .map(|wasapi| matches!(wasapi.get_state(), PlaybackState::Playing | PlaybackState::Stopping | PlaybackState::Pausing))
                .ok_or_else(|| "WASAPI player not initialized".to_string())?
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
            .try_lock()
            .map_err(|_| "Failed to acquire player lock".to_string())?;
        !player.is_paused() && !player.empty()
    };

    Ok(PlaybackStatus::new(is_playing, 0.0, volume))
}

/// 检查音轨是否播放完毕
pub fn check_track_finished(state: &State<AppState>) -> Result<bool, String> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .map(|g| *g)
        .map_err(|_| "Failed to acquire exclusive mode lock".to_string())?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .try_lock()
                .map_err(|_| "Failed to acquire WASAPI player lock".to_string())?;
            let wasapi = guard
                .as_ref()
                .ok_or_else(|| "WASAPI player not initialized".to_string())?;
            Ok(wasapi.get_state() == PlaybackState::Stopped)
        }
        #[cfg(not(windows))]
        {
            Err("Exclusive mode is only supported on Windows".to_string())
        }
    } else {
        let player = state
            .player
            .output
            .sink
            .try_lock()
            .map_err(|_| "Failed to acquire player lock".to_string())?;
        Ok(player.empty() && !player.is_paused())
    }
}
