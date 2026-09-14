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
use super::dsp::soft_clip_fast;
use super::sample_ring::SampleRing;
use super::spectrum::{SpectrumAnalyzer, now_ms};
use crate::error::AppError;

use super::{FADE_IN_MS, FADE_IN_ON_SEEK_MS};

use crate::AppState;
use crate::equalizer::{EQ_BAND_COUNT, EqSettings};
use rodio::Source;
use std::fs::File;
use std::io::BufReader;
// AtomicBool 供 VisualizationSource 通知频谱分析线程退出
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

/// 音轨结束事件
#[derive(Debug, serde::Serialize, Clone)]
pub struct TrackEndedEvent {}

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
pub struct EqProcessor {
    coefficients: Vec<crate::equalizer::BiquadCoefficients>,
    /// 扁平布局: states[channel * EQ_BAND_COUNT + band]
    states: Vec<crate::equalizer::BiquadState>,
    sample_rate: f32,
    channels: usize,
    cached_enabled: bool,
    cached_preamp_multiplier: f32,
}

impl EqProcessor {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
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
    pub fn update_settings(&mut self, settings: &EqSettings) {
        self.cached_enabled = settings.enabled;
        // 自动增益补偿: preamp 减去最大提升量,保证提升 band 后峰值不超 0dBFS
        // (preamp 每次设置变更只计算一次,无需查表)
        self.cached_preamp_multiplier = 10.0_f32.powf(settings.effective_preamp_db() / 20.0);

        if settings.enabled {
            self.update_coefficients(settings);
        }
    }

    fn update_coefficients(&mut self, settings: &EqSettings) {
        use crate::equalizer::BiquadCoefficients;
        for (i, &gain) in settings.gains.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::for_band(self.sample_rate, i, gain);
        }
    }

    /// 批量处理采样 - 合并三阶段循环为单次遍历,提升 cache 局部性
    #[inline]
    pub fn process_batch(&mut self, samples: &mut [f32]) {
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
    // 独占模式仅存在于 Windows(WASAPI),Linux 编译时这两个方法无调用方
    /// 处理单个采样(preamp + biquad + soft_clip),公开供独占模式与 benchmark 复用。
    /// 调用方需保证按交错声道依次调用(channel = 采样在帧内声道下标)。
    #[inline(always)]
    pub fn process_sample(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled {
            return input;
        }
        self.process_one(input, channel, self.cached_preamp_multiplier)
    }

    #[cfg(windows)]
    #[inline(always)]
    pub(super) fn process_sample_cached(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled {
            return input;
        }
        self.process_one(input, channel, self.cached_preamp_multiplier)
    }

    #[cfg(windows)]
    pub(super) const fn is_enabled(&self) -> bool {
        self.cached_enabled
    }
}

/// 共享模式音源:EQ 批量处理 + 把采样交给频谱分析线程
///
/// `next` 跑在 rodio 音频回调线程上:只做无锁写入与置标志,
/// FFT 与 spectrum-update / playback-position / track-ended 的发送见
/// [`spawn_spectrum_thread`]。
pub struct VisualizationSource<I: Source<Item = f32> + Send> {
    input: I,
    eq_settings: Arc<RwLock<EqSettings>>,
    eq_processor: EqProcessor,
    eq_update_counter: u32,
    /// 已播放采样数,与分析线程共享(播放位置事件在分析线程据此计算)
    samples_played: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
    // 批量处理缓冲区:直接存 EQ 处理后的采样,避免原始采样的中间拷贝
    pending_processed: Vec<f32>,
    pending_index: usize,
    /// 音频线程 → 分析线程的无锁采样通道
    sample_ring: Arc<SampleRing>,
    /// 源已耗尽(EOF):音频线程置位,分析线程负责发出 track-ended 并清位
    eof_reached: Arc<AtomicBool>,
    /// 置为 true 后分析线程退出(本值在 drop 时设置)
    analysis_stop: Arc<AtomicBool>,
}

/// 采样通道容量(向上取整到 2 的幂)
///
/// 192kHz 立体声约 38.4 万采样/秒,32K 约合 85ms,留足余量吸收调度抖动。
const SPECTRUM_RING_CAPACITY: usize = 32 * 1024;
/// 分析线程单次最多取用的采样数
const SPECTRUM_DRAIN_MAX: usize = 8192;
/// 分析线程轮询间隔
const SPECTRUM_POLL_INTERVAL: Duration = Duration::from_millis(1);
/// 播放位置事件的最小发送间隔(毫秒)
const POSITION_EMIT_INTERVAL_MS: u64 = 100;

/// 启动频谱分析线程
///
/// FFT 会堆分配、`app.emit` 要 JSON 序列化并跨线程投递,都不能在音频回调线程上做。
/// 音频线程只写 [`SampleRing`] 与置 EOF 标志,这里负责计算与发送。
fn spawn_spectrum_thread(
    sample_rate: u32,
    channels: u16,
    ring: Arc<SampleRing>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    target_fps: Arc<AtomicU64>,
    app: Option<AppHandle>,
    samples_played: Arc<AtomicU64>,
    eof_reached: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut analyzer = SpectrumAnalyzer::new(sample_rate);
        let mut scratch: Vec<f32> = Vec::with_capacity(SPECTRUM_DRAIN_MAX);
        // 初值取当前时间:调用方的 with_start_position 晚于此,避免先发一次 position=0
        let mut last_position_emit: u64 = now_ms();
        // 初值取 MAX,保证首个发送窗口一定发一次位置
        let mut last_emitted_samples: u64 = u64::MAX;

        // 消费一次 EOF 标志并发送 track-ended(用 swap 保证只发一次)
        fn emit_ended_once(app: Option<&AppHandle>, eof: &AtomicBool) {
            if eof.swap(false, Ordering::SeqCst) {
                if let Some(app) = app {
                    let _ = emit_track_ended(app);
                }
            }
        }

        while !stop.load(Ordering::SeqCst) {
            // 曲目结束事件:音频线程只置位,发送在这里做
            emit_ended_once(app.as_ref(), &eof_reached);

            let drained = ring.drain_into(&mut scratch, SPECTRUM_DRAIN_MAX);
            if drained > 0 {
                analyzer.extend_buffer(&scratch);
                scratch.clear();
            }

            let now = now_ms();
            analyzer.compute_if_ready(now, &spectrum_data, &target_fps, app.as_ref());

            // 播放位置事件同样不能从音频线程发送
            if now.saturating_sub(last_position_emit) >= POSITION_EMIT_INTERVAL_MS {
                last_position_emit = now;
                let played = samples_played.load(Ordering::Relaxed);
                // 暂停时计数不再增长,重复投递相同位置只是白白占用 IPC
                if played != last_emitted_samples {
                    last_emitted_samples = played;
                    if let Some(app) = app.as_ref() {
                        let position = played as f32 / (sample_rate as f32 * channels as f32);
                        let _ = emit_playback_position(app, position);
                    }
                }
            }

            // 固定节奏轮询:不因缓冲非空而忙等,避免空转吃满一个核
            std::thread::sleep(SPECTRUM_POLL_INTERVAL);
        }

        // EOF 与 drop 几乎同时发生(音频线程先置 EOF、再在 drop 里置 stop),
        // 退出前再消费一次,避免漏发
        emit_ended_once(app.as_ref(), &eof_reached);
    });
}

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    pub fn new(
        input: I,
        spectrum_data: Arc<Mutex<Vec<f32>>>,
        app_handle: Option<AppHandle>,
        target_fps: Arc<AtomicU64>,
    ) -> Self {
        let (sr, ch) = (input.sample_rate().get(), input.channels().get());
        let samples_played = Arc::new(AtomicU64::new(0));
        let sample_ring = Arc::new(SampleRing::new(SPECTRUM_RING_CAPACITY));
        let eof_reached = Arc::new(AtomicBool::new(false));
        let analysis_stop = Arc::new(AtomicBool::new(false));

        // 分析线程接管 FFT 与事件发送,spectrum_data / app_handle 所有权整体移交
        spawn_spectrum_thread(
            sr,
            ch,
            Arc::clone(&sample_ring),
            spectrum_data,
            target_fps,
            app_handle,
            Arc::clone(&samples_played),
            Arc::clone(&eof_reached),
            Arc::clone(&analysis_stop),
        );

        Self {
            input,
            eq_settings: Arc::new(RwLock::new(EqSettings::default())),
            eq_processor: EqProcessor::new(sr, ch),
            eq_update_counter: 0,
            samples_played,
            sample_rate: sr,
            channels: ch,
            pending_processed: Vec::with_capacity(BATCH_SIZE),
            pending_index: 0,
            sample_ring,
            eof_reached,
            analysis_stop,
        }
    }

    /// 设置初始播放位置（用于seek操作）
    ///
    /// 需紧跟 `new` 调用:分析线程已启动,位置事件有最小间隔,首个事件必然晚于此。
    #[must_use]
    pub fn with_start_position(self, position_secs: f32) -> Self {
        let samples = (position_secs * self.sample_rate as f32 * self.channels as f32) as u64;
        // 计数走原子量:无需 mut self,位置由分析线程读取后计算播放进度
        self.samples_played.store(samples, Ordering::Relaxed);
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
                // EOF - 只置标志,由分析线程发送 track-ended(音频线程不做 IPC)
                self.eof_reached.store(true, Ordering::SeqCst);
                return None;
            }
        }

        let processed = self.pending_processed[self.pending_index];
        self.pending_index += 1;
        self.samples_played.fetch_add(1, Ordering::Relaxed);

        // 只做无锁写入;缓冲满时丢弃该采样,音频回调不阻塞
        self.sample_ring.push(processed);

        Some(processed)
    }
}

impl<I: Source<Item = f32> + Send> Drop for VisualizationSource<I> {
    fn drop(&mut self) {
        // 通知分析线程退出;不 join(丢弃音源的可能是音频线程)。
        // SeqCst 与 eof_reached 配对,保证分析线程能看到 EOF 标志。
        self.analysis_stop.store(true, Ordering::SeqCst);
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
            let file = File::open(path).map_err(|e| e.to_string())?;
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
    drop(player_lock);
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
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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

#[cfg(test)]
mod eq_processor_tests {
    use super::*;
    use crate::equalizer::EqSettings;
    use std::f32::consts::PI;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    fn enabled_settings(band: usize, gain_db: f32, preamp: f32) -> EqSettings {
        let mut settings = EqSettings {
            enabled: true,
            ..EqSettings::default()
        };
        settings.gains[band] = gain_db;
        settings.preamp = preamp;
        settings
    }

    /// 逐采样处理指定声道(EqProcessor 的 process_batch 无单采样 API)。
    /// 占位声道填入 0.0,不影响目标声道各自的 biquad 状态。
    fn process_channel(ep: &mut EqProcessor, input: f32, channel: usize) -> f32 {
        let mut buf = [0.0_f32; 2];
        buf[channel] = input;
        ep.process_batch(&mut buf);
        buf[channel]
    }

    /// 稳态正弦峰值(丢弃前 warmup 个采样,取峰值)
    fn steady_state_amplitude(ep: &mut EqProcessor, freq: f32, amplitude: f32) -> f32 {
        let sample_rate = 48000.0_f32;
        let warmup = 4800;
        let mut peak = 0.0_f32;
        for n in 0..warmup * 2 {
            let t = n as f32 / sample_rate;
            let x = (2.0 * PI * freq * t).sin() * amplitude;
            let mut buf = [x];
            ep.process_batch(&mut buf);
            if n >= warmup {
                peak = peak.max(buf[0].abs());
            }
        }
        peak
    }

    #[test]
    fn test_eq_processor_disabled_passthrough() {
        let mut ep = EqProcessor::new(48000, 2);
        // 未 update_settings: cached_enabled = false
        for &x in &[0.5_f32, -0.3, 0.8, 0.0] {
            let y = process_channel(&mut ep, x, 0);
            assert!(approx_eq(y, x), "disabled EQ output {y} != input {x}");
        }
    }

    #[test]
    fn test_eq_processor_enabled_changes_signal() {
        let mut ep = EqProcessor::new(48000, 2);
        ep.update_settings(&enabled_settings(5, 6.0, 0.0)); // 提升 1kHz +6dB
        let sample_rate = 48000.0_f32;
        let freq = 1000.0_f32;
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for n in 0..480_i32 {
            let t = n as f32 / sample_rate;
            let x = (2.0 * PI * freq * t).sin() * 0.5;
            let y = process_channel(&mut ep, x, 0);
            input_energy += f64::from(x) * f64::from(x);
            output_energy += f64::from(y) * f64::from(y);
        }
        assert!(
            (output_energy - input_energy).abs() > 1e-4,
            "enabled EQ should change signal energy: in={input_energy}, out={output_energy}"
        );
    }

    #[test]
    fn test_low_shelf_boosts_bass_region() {
        let mut ep = EqProcessor::new(48000, 2);
        // 31Hz low shelf +6dB;preamp 补偿最大提升量以便单独验证滤波器响应
        ep.update_settings(&enabled_settings(0, 6.0, 6.0));
        let boosted = steady_state_amplitude(&mut ep, 8.0, 0.25);
        assert!(
            (boosted - 0.5).abs() < 0.06,
            "8Hz should be boosted ~+6dB by 31Hz low shelf, got peak {boosted}"
        );
        let mut untouched_ep = EqProcessor::new(48000, 2);
        untouched_ep.update_settings(&enabled_settings(0, 6.0, 6.0));
        let untouched = steady_state_amplitude(&mut untouched_ep, 1000.0, 0.25);
        assert!(
            (untouched - 0.25).abs() < 0.03,
            "1kHz should be nearly unaffected by 31Hz low shelf, got peak {untouched}"
        );
    }

    #[test]
    fn test_high_shelf_boosts_treble_region() {
        let mut ep = EqProcessor::new(48000, 2);
        ep.update_settings(&enabled_settings(9, 6.0, 6.0)); // 16kHz high shelf
        let boosted = steady_state_amplitude(&mut ep, 22000.0, 0.25);
        assert!(
            (boosted - 0.5).abs() < 0.06,
            "22kHz should be boosted ~+6dB by 16kHz high shelf, got peak {boosted}"
        );
        let mut untouched_ep = EqProcessor::new(48000, 2);
        untouched_ep.update_settings(&enabled_settings(9, 6.0, 6.0));
        let untouched = steady_state_amplitude(&mut untouched_ep, 500.0, 0.25);
        assert!(
            (untouched - 0.25).abs() < 0.03,
            "500Hz should be nearly unaffected by 16kHz high shelf, got peak {untouched}"
        );
    }

    #[test]
    fn test_auto_preamp_prevents_clipping_on_boost() {
        // +8dB boost + 满幅正弦:自动补偿后输出峰值不应超过 soft clip 阈值附近
        let mut ep = EqProcessor::new(48000, 2);
        ep.update_settings(&enabled_settings(5, 8.0, 0.0));
        let peak = steady_state_amplitude(&mut ep, 1000.0, 0.95);
        assert!(
            peak <= 0.96,
            "auto preamp should keep boosted peak <=0dBFS, got {peak}"
        );
    }

    #[test]
    fn test_eq_processor_batch_is_chunk_splitting_invariant() {
        // 交错立体声缓冲:process_batch 单次整批处理与分多次小批处理结果应一致
        // (biquad 状态跨调用保持,等价于旧的 process_buffer/process_sample 一致性)
        let n_frames: usize = 480;
        let buffer: Vec<f32> = (0..n_frames * 2)
            .map(|i| {
                let frame = i / 2;
                let t = frame as f32 / 48000.0_f32;
                (2.0 * PI * 1000.0 * t).sin() * 0.3
            })
            .collect();

        let mut whole = EqProcessor::new(48000, 2);
        whole.update_settings(&enabled_settings(5, 4.0, 0.0));
        let mut expected = buffer.clone();
        whole.process_batch(&mut expected);

        // 分块(每块 13 帧 = 26 采样,保持交错声道对齐)处理,结果应与整批一致
        let mut chunked = EqProcessor::new(48000, 2);
        chunked.update_settings(&enabled_settings(5, 4.0, 0.0));
        let mut got = buffer.clone();
        const FRAME: usize = 2; // 声道数
        let chunk = 13 * FRAME;
        for start in (0..got.len()).step_by(chunk) {
            let end = (start + chunk).min(got.len());
            chunked.process_batch(&mut got[start..end]);
        }
        for (i, (a, b)) in expected.iter().zip(got.iter()).enumerate() {
            assert!(
                approx_eq(*a, *b),
                "chunked[{i}] (={b}) != whole-batch (={a})",
            );
        }

        let any_changed = buffer
            .iter()
            .zip(expected.iter())
            .any(|(a, b)| (a - b).abs() > 1e-4);
        assert!(
            any_changed,
            "process_batch should modify samples with EQ enabled"
        );
    }
}
