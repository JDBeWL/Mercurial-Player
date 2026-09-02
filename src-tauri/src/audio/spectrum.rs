//! 可复用的频谱分析器
//!
//! 共享模式由 [`super::playback::VisualizationSource`] 在 rodio 拉取采样时驱动;
//! 独占模式由 [`super::decode_push`] 解码推送线程在推送采样后驱动。
//! 两者最终都通过 `spectrum-update` 事件把频谱数据发给前端可视化面板。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::{FrequencyLimit, samples_fft_to_spectrum};
use tauri::{AppHandle, Emitter};

use super::dsp::precompute_hann_window;

/// 频谱更新事件 - 简化结构减少序列化开销
#[derive(Debug, serde::Serialize, Clone)]
pub struct SpectrumUpdateEvent {
    pub data: Vec<f32>,
}

#[inline]
pub(super) fn emit_spectrum_update(
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

/// 根据采样率计算最佳FFT缓冲区大小
/// 目标是保持约~43ms的分析窗口（2048@48kHz）
#[must_use]
pub(super) const fn calculate_fft_size(sample_rate: u32) -> usize {
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

/// 当前 Unix 时间戳(毫秒)
#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 频谱分析器:Hann 窗 + FFT + AE 风格分bin/平滑
///
/// 维护一个滚动采样缓冲(交错采样),缓冲满且距上次计算达到目标帧率
/// 间隔时计算频谱、更新共享 `spectrum_data` 并发送 `spectrum-update` 事件。
pub(super) struct SpectrumAnalyzer {
    /// 滚动采样缓冲(交错采样)
    buffer: Vec<f32>,
    fft_buffer: Vec<f32>,
    /// 预计算的 Hann 窗口(按 fft_size 一次预计算,避免每次 FFT 堆分配)
    hann_window: Vec<f32>,
    spectrum_buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
    fft_size: usize,
    sample_rate: u32,
    last_fft_time: u64,
}

impl SpectrumAnalyzer {
    #[must_use]
    pub fn new(sample_rate: u32) -> Self {
        let fft_size = calculate_fft_size(sample_rate);
        Self {
            buffer: Vec::with_capacity(fft_size),
            fft_buffer: vec![0.0; fft_size],
            hann_window: precompute_hann_window(fft_size),
            spectrum_buffer: vec![0.0; 128],
            prev_spectrum: vec![0.0; 128],
            fft_size,
            sample_rate,
            last_fft_time: 0,
        }
    }

    pub const fn fft_size(&self) -> usize {
        self.fft_size
    }

    /// 当前滚动缓冲中的采样数
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// 追加单个采样(共享模式逐采样驱动)
    pub fn push_sample(&mut self, sample: f32) {
        self.buffer.push(sample);
    }

    /// 追加一批交错采样;缓冲满且到达目标帧率间隔时计算并发射频谱
    /// (独占模式解码线程按块驱动)
    #[cfg(windows)]
    pub fn push_and_maybe_emit(
        &mut self,
        samples: &[f32],
        spectrum_data: &Arc<Mutex<Vec<f32>>>,
        target_fps: &AtomicU64,
        app: &AppHandle,
    ) {
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() < self.fft_size {
            return;
        }
        let now = now_ms();
        if self.should_compute(now, target_fps) {
            self.compute_and_emit(now, spectrum_data, Some(app));
        }
        // 保留后半部分数据用于重叠分析
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
    /// 要求 `buffer_len() >= fft_size()`。
    #[inline(never)]
    pub fn compute_and_emit(
        &mut self,
        now: u64,
        spectrum_data: &Arc<Mutex<Vec<f32>>>,
        app: Option<&AppHandle>,
    ) {
        self.last_fft_time = now;

        if let Ok(mut spec) = spectrum_data.try_lock() {
            // 复用预分配的缓冲区
            self.fft_buffer
                .copy_from_slice(&self.buffer[..self.fft_size]);
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
        if let Some(app) = app {
            let _ = emit_spectrum_update(app, &self.prev_spectrum);
        }
    }
}
