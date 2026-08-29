//! 音频播放模块
//!
//! 提供音频播放、暂停、恢复、音量控制等功能。
//!
//! 使用SIMD友好的批量处理
//! 预计算查找表避免热路径上的数学运算
//! 无锁设计减少线程竞争

#[cfg(windows)]
use super::decode_push::decode_and_push_to_wasapi;
use super::decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
use super::dsp::{db_to_linear_fast, precompute_hann_window, soft_clip_fast};
use crate::error::AppError;

use super::{FADE_IN_MS, FADE_IN_ON_SEEK_MS, LockOrErr};

#[cfg(windows)]
use super::wasapi::PlaybackState;
use crate::AppState;
use crate::equalizer::{EQ_BAND_COUNT, EqSettings};
use rodio::Source;
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::{FrequencyLimit, samples_fft_to_spectrum};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

// ============================================================================
// 预计算查找表
// ============================================================================

/// 软削波查找表大小（覆盖0.0到2.0范围，精度0.001）
/// 批量处理块大小（对齐到SIMD友好的边界）
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
        Self {
            is_playing,
            position_secs,
            volume,
        }
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
fn emit_spectrum_update(
    app: &AppHandle,
    data: &[f32],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 直接发送数据数组，减少JSON包装开销
    app.emit(
        "spectrum-update",
        SpectrumUpdateEvent {
            data: data.to_vec(),
        },
    )?;
    Ok(())
}

#[inline]
pub(super) fn emit_track_ended(
    app: &AppHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("track-ended", TrackEndedEvent {})?;
    Ok(())
}

#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPositionEvent {
    pub position: f32, // 秒
}

pub(super) fn emit_playback_position(
    app: &AppHandle,
    position: f32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("playback-position", PlaybackPositionEvent { position })?;
    Ok(())
}

// ============================================================================
// 批量处理缓冲区
// ============================================================================

/// EQ 处理器(共享模式与独占模式共用)
///
/// 共享模式通过 [`EqProcessor::process_batch`] 批量处理,
/// 独占模式通过 [`EqProcessor::process_sample_cached`] 逐采样处理。
///
/// 性能优化要点:
/// 1. states 采用扁平布局 `[channel * EQ_BAND_COUNT + band]`,同一 channel 的所有
///    band 状态在内存中连续,避免双层 Vec 解引用,提升 cache 命中率
/// 2. 三个处理阶段(preamp/biquad/soft_clip)合并为单次循环,提升 cache 局部性
/// 3. i % channels 在 channels=2 时编译器会优化为位运算,无需手动展开
pub(super) struct EqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    /// 扁平布局: states[channel * EQ_BAND_COUNT + band]
    states: Vec<crate::equalizer::BiquadState>,
    sample_rate: f32,
    channels: usize,
    cached_enabled: bool,
    cached_preamp_multiplier: f32,
}

impl EqProcessor {
    pub(super) fn new(sample_rate: u32, channels: u16) -> Self {
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
    pub(super) fn update_settings(&mut self, settings: &EqSettings) {
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
            self.coefficients[i] = BiquadCoefficients::peaking_eq(
                self.sample_rate,
                freq,
                settings.gains[i],
                EQ_Q_VALUES[i],
            );
        }
    }

    /// 批量处理采样 - 合并三阶段循环为单次遍历,提升 cache 局部性
    #[inline]
    fn process_batch(&mut self, samples: &mut [f32]) {
        if !self.cached_enabled {
            return;
        }

        let preamp = self.cached_preamp_multiplier;
        let channels = self.channels;

        // 单次循环完成 preamp + biquad + soft_clip,提升 cache 局部性
        // 注: i % channels 在 channels=2 时编译器优化为 i & 1,无取模开销
        for (i, sample) in samples.iter_mut().enumerate() {
            let channel = i % channels;
            *sample = self.process_one(*sample, channel, preamp);
        }
    }

    /// 处理单个采样(preamp + biquad + soft_clip)
    #[inline(always)]
    fn process_one(&mut self, input: f32, channel: usize, preamp: f32) -> f32 {
        let mut sample = input * preamp;
        let state_base = channel * EQ_BAND_COUNT;
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            let state = &mut self.states[state_base + band];
            sample = state.process(sample, coeffs);
        }
        soft_clip_fast(sample)
    }

    /// 逐采样处理(独占模式解码线程使用)
    #[inline(always)]
    pub(super) fn process_sample_cached(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled {
            return input;
        }
        self.process_one(input, channel, self.cached_preamp_multiplier)
    }

    pub(super) const fn is_enabled(&self) -> bool {
        self.cached_enabled
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
    eq_processor: EqProcessor,
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
        0..=32000 => 1024,       // ≤32kHz: 1024 样本
        32001..=64000 => 2048,   // 44.1k/48k: 2048 样本
        64001..=128_000 => 4096, // 88.2k/96k: 4096 样本
        _ => 8192,               // 176.4k/192k/384k: 8192 样本
    }
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    pub fn new(
        input: I,
        waveform_data: Arc<Mutex<Vec<f32>>>,
        spectrum_data: Arc<Mutex<Vec<f32>>>,
        app_handle: Option<AppHandle>,
        target_fps: Arc<AtomicU64>,
        enable_vertical_sync: Arc<AtomicBool>,
    ) -> Self {
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
            eq_processor: EqProcessor::new(sr, ch),
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
        self.samples_played =
            (position_secs * self.sample_rate as f32 * self.channels as f32) as u64;
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
        if self.eq_update_counter >= 8 {
            // 每8批次 = 512采样
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
                let position =
                    self.samples_played as f32 / (self.sample_rate as f32 * self.channels as f32);
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
                    if !(FREQ_MIN..=FREQ_MAX).contains(&f) {
                        continue;
                    }

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
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }
    fn channels(&self) -> std::num::NonZero<u16> {
        self.input.channels()
    }
    fn sample_rate(&self) -> std::num::NonZero<u32> {
        self.input.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

/// 播放音轨（共享模式）
pub fn play_track_shared(
    app: &AppHandle,
    state: &State<AppState>,
    path: &str,
    position: Option<f32>,
) -> Result<(), AppError> {
    let player = &state.player;
    // 取消任何正在进行的淡入淡出,防止其 on_complete(pause) 在新歌播放后执行
    player.fade.generation.fetch_add(1, Ordering::SeqCst);
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
    *lock_or_log!(player.track.current_source.lock()) = None;
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
                VisualizationSource::new(
                    LockFreeSymphoniaSource::new(dec),
                    waveform,
                    spectrum,
                    Some(app.clone()),
                    target_fps,
                    enable_vertical_sync,
                )
                .with_start_position(start_pos)
                .with_eq_settings(eq_settings)
                .fade_in(Duration::from_millis(FADE_IN_MS)), // 稍长的淡入来补偿没有淡出
            )
        }
        Err(e) => {
            log::warn!("Symphonia decoder failed, fallback to rodio: {e}");
            let file = File::open(path).map_err(|e| e.to_string())?;
            Box::new(
                VisualizationSource::new(
                    rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?,
                    waveform,
                    spectrum,
                    Some(app.clone()),
                    target_fps,
                    enable_vertical_sync,
                )
                .with_start_position(position.unwrap_or(0.0))
                .with_eq_settings(eq_settings)
                .fade_in(Duration::from_millis(FADE_IN_MS)),
            )
        }
    };

    // 获取 mixer 输出配置，手动重采样到 mixer 的采样率
    // rodio 0.22 的 UniformSourceIterator 在 queue keep_alive 模式下，
    // 当 source.current_span_len() 返回 None 时不会重新 bootstrap SampleRateConverter，
    // 导致高采样率音频以错误的速率播放（降速）。
    // 解决方案：在 append 之前手动将 source 重采样到 mixer 的采样率。
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
    Ok(())
}

/// 播放音轨（独占模式）
///
/// 异步实现：等待淡出/解码线程启动/缓冲区填充的 sleep 改用 `tokio::time::sleep`,
/// 避免阻塞 Tauri 命令线程导致前端 UI 卡顿（原本最坏阻塞 ~600ms）。
/// 解码推送线程内部仍有自己的 sleep，那是后台线程内的等待，不在此处理。
#[cfg(windows)]
pub async fn play_track_exclusive(
    app: &AppHandle,
    state: &State<'_, AppState>,
    path: &str,
    position: Option<f32>,
) -> Result<(), AppError> {
    let player = &state.player;
    // 递增代际计数器取消旧解码推送线程(替代 stop 布尔标志,避免 70ms 窗口内状态不一致)
    player.decode.generation.fetch_add(1, Ordering::SeqCst);
    let new_thread_id = player.decode.id.fetch_add(1, Ordering::SeqCst) + 1;
    {
        if let Some(ref wasapi) = *lock_or_log!(player.output.wasapi_player.lock()) {
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
        (wasapi.get_sample_rate(), wasapi.get_channels())
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
            decode_and_push_to_wasapi(
                source,
                wasapi_clone,
                waveform,
                spectrum,
                app_clone,
                generation,
                thread_id,
                new_thread_id,
                src_sr,
                src_ch.get(),
                target_sr,
                target_ch,
                eq_settings,
                start_pos,
            );
        }));
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
                g.as_ref().map_or(0, |p| p.get_buffer_size())
            };
            if current_size >= min_buffer_samples || buffer_wait >= 50 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            buffer_wait += 1;
        }
        // 额外等待一小段时间确保数据稳定
        tokio::time::sleep(Duration::from_millis(20)).await;
        // 启动播放(重新获取锁,不跨 await)
        let g = lock_or_log!(player.output.wasapi_player.lock());
        if let Some(ref wasapi) = *g {
            wasapi
                .start()
                .map_err(|e| format!("Failed to start WASAPI: {e:?}"))?;
        }
    }
    Ok(())
}

/// 播放音轨（独占模式）
#[cfg(not(windows))]
pub async fn play_track_exclusive(
    _app: &AppHandle,
    _state: &State<'_, AppState>,
    _path: &str,
    _position: Option<f32>,
) -> Result<(), AppError> {
    Err(AppError::Audio(
        "Exclusive mode is only supported on Windows".to_string(),
    ))
}

/// Seek共享模式
pub fn seek_track_shared(
    app: &AppHandle,
    state: &State<AppState>,
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
            Arc::clone(&player.visualization.waveform_data),
            Arc::clone(&player.visualization.spectrum_data),
            Some(app.clone()),
            Arc::clone(&player.visualization.target_fps),
            Arc::clone(&player.visualization.enable_vertical_sync),
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
    }
    let sink = lock_or_log!(player.output.sink.lock());
    sink.append(resampled);
    sink.play();
    Ok(())
}

/// 获取播放状态
pub fn get_status(state: &State<AppState>) -> Result<PlaybackStatus, AppError> {
    let volume = state
        .player
        .output
        .target_volume
        .try_lock()
        .lock_or_err("target volume")
        .map(|g| *g)?;

    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    let is_playing = if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .try_lock()
                .lock_or_err("WASAPI player")?;
            guard
                .as_ref()
                // Stopping/Pausing 期间音频仍在淡出(可听见),视为正在播放
                .map(|wasapi| {
                    matches!(
                        wasapi.get_state(),
                        PlaybackState::Playing | PlaybackState::Stopping | PlaybackState::Pausing
                    )
                })
                .ok_or_else(|| "WASAPI player not initialized".to_string())?
        }
        #[cfg(not(windows))]
        {
            return Err(AppError::Audio(
                "Exclusive mode is only supported on Windows".to_string(),
            ));
        }
    } else {
        let player = state.player.output.sink.try_lock().lock_or_err("player")?;
        !player.is_paused() && !player.empty()
    };

    Ok(PlaybackStatus::new(is_playing, 0.0, volume))
}

/// 检查音轨是否播放完毕
pub fn check_track_finished(state: &State<AppState>) -> Result<bool, AppError> {
    let exclusive_mode = state
        .player
        .output
        .exclusive_mode
        .try_lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive_mode {
        #[cfg(windows)]
        {
            let guard = state
                .player
                .output
                .wasapi_player
                .try_lock()
                .lock_or_err("WASAPI player")?;
            let wasapi = guard
                .as_ref()
                .ok_or_else(|| "WASAPI player not initialized".to_string())?;
            Ok(wasapi.get_state() == PlaybackState::Stopped)
        }
        #[cfg(not(windows))]
        {
            Err(AppError::Audio(
                "Exclusive mode is only supported on Windows".to_string(),
            ))
        }
    } else {
        let player = state.player.output.sink.try_lock().lock_or_err("player")?;
        Ok(player.empty() && !player.is_paused())
    }
}
