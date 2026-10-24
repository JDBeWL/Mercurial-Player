//! 频谱：分析器 + 共享模式音源 + 分析线程。
//!
//! 共享模式由 `VisualizationSource` 驱动，独占模式由 `decode_push` 驱动，最终都发 `spectrum-update`。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use rodio::Source;
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::{FrequencyLimit, samples_fft_to_spectrum};
use tauri::{AppHandle, Emitter};

use super::dsp::precompute_hann_window;
use super::emit::{emit_playback_position, emit_track_ended};
use super::eq_processor::EqProcessor;
use super::sample_ring::SampleRing;
use crate::equalizer::EqSettings;

/// 频谱 bin 数量：分析端分箱、平滑与前端可视化的柱数三方必须一致
///
/// 另两处是 `app_state.rs` 的 `spectrum_data` 与 `VisualizerPanel.vue` 的 `SPECTRUM_SIZE`
pub(crate) const SPECTRUM_BINS: usize = 128;

/// 频谱更新事件 - 简化结构减少序列化开销
///
/// 借用语义而不是持有 `Vec`：调用方已经有 `prev_spectrum`，再 `to_vec()` 只是每秒几十次堆分配。
#[derive(Debug, serde::Serialize, Clone)]
pub struct SpectrumUpdateEvent<'a> {
    pub data: &'a [f32],
}

#[inline]
pub(super) fn emit_spectrum_update(
    app: &AppHandle,
    data: &[f32],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("spectrum-update", SpectrumUpdateEvent { data })?;
    Ok(())
}

/// 频谱门控：`panel` 与 `foreground` 同时为真才计算并发送。
/// `foreground` 必须由 Kotlin 生命周期写入：退到后台时 wry 已冻结 JS，前端补不了。
pub struct SpectrumGate {
    foreground: AtomicBool,
    panel: AtomicBool,
}

impl SpectrumGate {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground: AtomicBool::new(true),
            panel: AtomicBool::new(false),
        }
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.foreground.store(foreground, Ordering::Relaxed);
    }

    pub fn set_panel_visible(&self, visible: bool) {
        self.panel.store(visible, Ordering::Relaxed);
    }

    #[must_use]
    pub fn allowed(&self) -> bool {
        self.foreground.load(Ordering::Relaxed) && self.panel.load(Ordering::Relaxed)
    }
}

impl Default for SpectrumGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 根据采样率取约 43ms 分析窗口的 FFT 大小（2 的幂，2048@48kHz）
#[must_use]
pub(super) const fn calculate_fft_size(sample_rate: u32) -> usize {
    match sample_rate {
        0..=32000 => 1024,       // <=32kHz: 1024 样本
        32001..=64000 => 2048,   // 44.1k/48k: 2048 样本
        64001..=128_000 => 4096, // 88.2k/96k: 4096 样本
        _ => 8192,               // 176.4k/192k/384k: 8192 样本
    }
}

/// 当前 Unix 时间戳(毫秒)
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 频谱分析器：Hann 窗 + FFT + AE 风格分 bin/平滑。
/// 入口先把多声道按帧平均成单声道，否则频率轴翻倍、窗口减半且 L/R 梳状叠加导致闪烁。
pub(super) struct SpectrumAnalyzer {
    /// 滚动采样缓冲(已下混为单声道)
    buffer: Vec<f32>,
    fft_buffer: Vec<f32>,
    /// 预计算的 Hann 窗口(按 fft_size 一次预计算,避免每次 FFT 堆分配)
    hann_window: Vec<f32>,
    spectrum_buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
    fft_size: usize,
    sample_rate: u32,
    /// 入口采样的声道数(1 表示已是单声道,直接透传)
    channels: usize,
    last_fft_time: u64,
    /// FFT 失败只报一次:低频源/NaN 会每帧都失败,高频日志本身也是开销
    fft_error_logged: bool,
}

impl SpectrumAnalyzer {
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let fft_size = calculate_fft_size(sample_rate);
        Self {
            buffer: Vec::with_capacity(fft_size),
            fft_buffer: vec![0.0; fft_size],
            hann_window: precompute_hann_window(fft_size),
            spectrum_buffer: vec![0.0; SPECTRUM_BINS],
            prev_spectrum: vec![0.0; SPECTRUM_BINS],
            fft_size,
            sample_rate,
            channels: usize::from(channels).max(1),
            last_fft_time: 0,
            fft_error_logged: false,
        }
    }

    /// 追加一批交错采样(分析线程按块驱动)，按帧平均下混为单声道后入缓冲
    pub fn extend_buffer(&mut self, samples: &[f32]) {
        self.push_interleaved(samples);
    }

    /// 交错多声道 -> 单声道：逐帧求平均。声道数为 1 时就是原样追加
    fn push_interleaved(&mut self, samples: &[f32]) {
        let ch = self.channels;
        if ch == 1 {
            self.buffer.extend_from_slice(samples);
            return;
        }
        self.buffer.reserve(samples.len() / ch);
        for frame in samples.chunks_exact(ch) {
            self.buffer.push(frame.iter().sum::<f32>() / ch as f32);
        }
        // 尾部的零头(不足一帧)直接丢掉：调用方按块投递，长期看不会积累
    }

    /// 缓冲满且到达目标帧率间隔时计算并发送频谱
    ///
    /// 否则什么都不做：分析线程按固定节奏轮询，未到间隔时保留完整窗口，不提前 retain_half
    pub fn compute_if_ready(
        &mut self,
        now: u64,
        spectrum_data: &Arc<Mutex<Vec<f32>>>,
        target_fps: &AtomicU64,
        app: Option<&AppHandle>,
    ) {
        if self.buffer.len() < self.fft_size {
            return;
        }
        if !self.should_compute(now, target_fps) {
            return;
        }
        self.compute_and_emit(now, spectrum_data, app);
        // 保留后半部分用于重叠分析，见 retain_half
        self.retain_half();
    }

    /// 追加一批交错采样并按需计算发送（独占模式解码线程按块驱动）
    #[cfg(any(windows, target_os = "android"))]
    pub fn push_and_maybe_emit(
        &mut self,
        samples: &[f32],
        spectrum_data: &Arc<Mutex<Vec<f32>>>,
        target_fps: &AtomicU64,
        app: &AppHandle,
    ) {
        self.push_interleaved(samples);
        if self.buffer.len() < self.fft_size {
            return;
        }
        let now = now_ms();
        if self.should_compute(now, target_fps) {
            self.compute_and_emit(now, spectrum_data, Some(app));
        }
        self.retain_half();
    }

    /// 距上次计算是否已达到目标帧率间隔
    pub fn should_compute(&self, now: u64, target_fps: &AtomicU64) -> bool {
        // 限制FFT计算和发送频率（与目标帧率一致；画面同步由前端 rAF 天然保证）
        let target_fps = target_fps.load(Ordering::Relaxed).max(1);
        let fft_interval_ms = 1000 / target_fps;
        now.saturating_sub(self.last_fft_time) >= fft_interval_ms
    }

    /// 保留缓冲区后半部分用于重叠分析(缓冲满并完成本轮处理后调用)
    pub fn retain_half(&mut self) {
        let half = self.buffer.len() / 2;
        self.buffer.drain(..half);
    }

    /// 计算频谱:更新共享 `spectrum_data` 并发送 `spectrum-update` 事件。
    ///
    /// 要求缓冲中至少有 `fft_size` 个采样。
    #[inline(never)]
    pub fn compute_and_emit(
        &mut self,
        now: u64,
        spectrum_data: &Arc<Mutex<Vec<f32>>>,
        app: Option<&AppHandle>,
    ) {
        self.last_fft_time = now;

        if let Ok(mut spec) = spectrum_data.try_lock() {
            self.fft_buffer
                .copy_from_slice(&self.buffer[..self.fft_size]);
            // 手动应用预计算的 Hann 窗口(避免 hann_window() 每次堆分配 Vec)
            for i in 0..self.fft_size {
                self.fft_buffer[i] *= self.hann_window[i];
            }

            // 上限必须留在 Nyquist 内：spectrum-analyzer 会校验 Range 并直接返回 Err，
            // 写死 20kHz 会让 22.05k/32k 这类低频源每帧都算不出频谱且没有任何提示
            let nyquist = self.sample_rate as f32 / 2.0;
            let freq_hi = 20000.0f32.min(nyquist - 1.0).max(21.0);
            let fft = samples_fft_to_spectrum(
                &self.fft_buffer,
                self.sample_rate,
                FrequencyLimit::Range(20.0, freq_hi),
                Some(&divide_by_N_sqrt),
            );
            if let Ok(spectrum) = &fft {
                self.spectrum_buffer.fill(0.0);

                // AE风格：线性频率分布
                const FREQ_MIN: f32 = 20.0;
                const FREQ_MAX: f32 = 16000.0;
                const FREQ_STEP: f32 = (FREQ_MAX - FREQ_MIN) / SPECTRUM_BINS as f32;

                for (freq, value) in spectrum.data() {
                    let f = freq.val();
                    if !(FREQ_MIN..=FREQ_MAX).contains(&f) {
                        continue;
                    }

                    let bin = ((f - FREQ_MIN) / FREQ_STEP).floor() as usize;
                    let bin = bin.min(SPECTRUM_BINS - 1);

                    let v = value.val();
                    if v > self.spectrum_buffer[bin] {
                        self.spectrum_buffer[bin] = v;
                    }
                }

                // AE风格的平滑：快速上升，缓慢下降
                for i in 0..SPECTRUM_BINS {
                    let target = self.spectrum_buffer[i];
                    let current = self.prev_spectrum[i];

                    self.prev_spectrum[i] = if target > current {
                        current * 0.3 + target * 0.7
                    } else {
                        current * 0.85 + target * 0.15
                    };
                }

                spec.clear();
                spec.extend_from_slice(&self.prev_spectrum);
            } else if !self.fft_error_logged {
                // 只报一次：出错的源会每帧都失败，日志本身比 FFT 更贵
                self.fft_error_logged = true;
                log::warn!(
                    "频谱计算失败(sr={}, channels={}, limit=20..{freq_hi}): {:?}",
                    self.sample_rate,
                    self.channels,
                    fft.as_ref().err()
                );
            }
        }

        // 发送事件 - 与FFT计算同步，不再单独节流
        if let Some(app) = app {
            let _ = emit_spectrum_update(app, &self.prev_spectrum);
        }
    }
}

/// 批量处理块大小（对齐到SIMD友好的边界）
const BATCH_SIZE: usize = 64;
/// 共享模式音源:EQ 批量处理 + 把采样交给频谱分析线程
///
/// `next` 跑在 rodio 音频回调线程上:只做无锁写入与置标志，FFT 与 spectrum-update /
/// playback-position / track-ended 的发送见 `spawn_spectrum_thread`。
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
    /// 音频线程 -> 分析线程的无锁采样通道
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
/// 音频线程只写 `SampleRing` 与置 EOF 标志,这里负责计算与发送。
fn spawn_spectrum_thread(
    sample_rate: u32,
    channels: u16,
    ring: Arc<SampleRing>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    target_fps: Arc<AtomicU64>,
    gate: Arc<SpectrumGate>,
    app: Option<AppHandle>,
    samples_played: Arc<AtomicU64>,
    eof_reached: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut analyzer = SpectrumAnalyzer::new(sample_rate, channels);
        let mut scratch: Vec<f32> = Vec::with_capacity(SPECTRUM_DRAIN_MAX);
        // 初值取当前时间:调用方的 with_start_position 晚于此,避免先发一次 position=0
        let mut last_position_emit: u64 = now_ms();
        // 初值取 MAX,保证首个发送窗口一定发一次位置
        let mut last_emitted_samples: u64 = u64::MAX;

        // 消费一次 EOF 标志并发送 track-ended(用 swap 保证只发一次)
        fn emit_ended_once(app: Option<&AppHandle>, eof: &AtomicBool) {
            if eof.swap(false, Ordering::SeqCst) {
                if let Some(app) = app {
                    if let Err(e) = emit_track_ended(app) {
                        log::warn!("track-ended 发送失败，自动续播可能中断: {e}");
                    }
                }
            }
        }

        while !stop.load(Ordering::SeqCst) {
            emit_ended_once(app.as_ref(), &eof_reached);

            let now = now_ms();
            // 门控见 SpectrumGate；未放行时采样堆到 ring 上限后由 push 侧丢弃
            if gate.allowed() {
                let drained = ring.drain_into(&mut scratch, SPECTRUM_DRAIN_MAX);
                if drained > 0 {
                    analyzer.extend_buffer(&scratch);
                    scratch.clear();
                }
                analyzer.compute_if_ready(now, &spectrum_data, &target_fps, app.as_ref());
            }

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
        gate: Arc<SpectrumGate>,
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
            gate,
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

        // 每 8 批（512 采样）刷新一次 EQ 设置：try_read 拿不到就跳过，不阻塞音频线程
        self.eq_update_counter += 1;
        if self.eq_update_counter >= 8 {
            self.eq_update_counter = 0;
            if let Ok(s) = self.eq_settings.try_read() {
                self.eq_processor.update_settings(&s);
            }
        }

        self.eq_processor.process_batch(&mut self.pending_processed);

        true
    }
}

impl<I: Source<Item = f32> + Send> Iterator for VisualizationSource<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
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

        // 满则丢弃，理由见 `SampleRing::push`
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
