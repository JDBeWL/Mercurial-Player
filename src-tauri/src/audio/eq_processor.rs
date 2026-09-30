//! EQ 处理器：播放链路里实际跑滤波的那个环节。
//!
//! 系数与设置类型在 [`crate::equalizer`]，这里按交错声道布局做批量/逐采样处理，
//! 软削波用 [`super::dsp`] 的预计算查找表。共享与独占两条链路都复用它。

use super::dsp::soft_clip_fast;
use crate::equalizer::{EQ_BAND_COUNT, EqSettings};

/// EQ 处理器(共享模式与独占模式共用):共享走 [`EqProcessor::process_batch`] 批量处理,
/// 独占走 [`EqProcessor::process_sample_cached`] 逐采样。
/// states 用扁平布局 `[channel * EQ_BAND_COUNT + band]` 且三阶段合并为单次循环,均为 cache 局部性。
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

    /// 处理单个采样(preamp + biquad + soft_clip),公开供独占模式解码线程与 benchmark 复用。
    /// 调用方需保证按交错声道依次调用(channel = 采样在帧内声道下标)。
    /// 独占模式只有 Windows(WASAPI) 与 Android(AAudio)，其它平台无调用方。
    #[inline(always)]
    pub fn process_sample(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled {
            return input;
        }
        self.process_one(input, channel, self.cached_preamp_multiplier)
    }

    #[cfg(any(windows, target_os = "android"))]
    #[inline(always)]
    pub(super) fn process_sample_cached(&mut self, input: f32, channel: usize) -> f32 {
        if !self.cached_enabled {
            return input;
        }
        self.process_one(input, channel, self.cached_preamp_multiplier)
    }

    #[cfg(any(windows, target_os = "android"))]
    pub(super) const fn is_enabled(&self) -> bool {
        self.cached_enabled
    }
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
