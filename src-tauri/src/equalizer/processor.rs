//! EQ 均衡器模块
//!
//! 实现 10 段参数均衡器，支持实时调节。

use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::f32::consts::PI;
use std::sync::{Arc, RwLock};

pub const EQ_BAND_COUNT: usize = 10;
pub const EQ_FREQUENCIES: [f32; EQ_BAND_COUNT] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const EQ_Q_VALUES: [f32; EQ_BAND_COUNT] = [0.7, 0.7, 0.8, 0.9, 1.0, 1.0, 1.1, 1.2, 1.3, 1.4];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPreset {
    pub name: String,
    pub gains: [f32; EQ_BAND_COUNT],
}

impl EqPreset {
    #[must_use]
    pub fn flat() -> Self {
        Self {
            name: "Flat".to_string(),
            gains: [0.0; EQ_BAND_COUNT],
        }
    }
    #[must_use]
    pub fn bass_boost() -> Self {
        Self {
            name: "Bass Boost".to_string(),
            gains: [4.0, 3.5, 2.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        }
    }
    #[must_use]
    pub fn treble_boost() -> Self {
        Self {
            name: "Treble Boost".to_string(),
            gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 2.5, 3.5, 4.0],
        }
    }
    #[must_use]
    pub fn vocal() -> Self {
        Self {
            name: "Vocal".to_string(),
            gains: [-1.5, -1.0, 0.0, 1.5, 2.5, 2.5, 2.0, 0.5, 0.0, -0.5],
        }
    }
    #[must_use]
    pub fn rock() -> Self {
        Self {
            name: "Rock".to_string(),
            gains: [3.5, 2.5, 1.5, 0.0, -0.5, 0.0, 1.5, 2.0, 2.5, 3.0],
        }
    }
    #[must_use]
    pub fn pop() -> Self {
        Self {
            name: "Pop".to_string(),
            gains: [-0.5, 0.0, 1.5, 2.0, 2.5, 2.0, 0.5, 0.0, -0.5, -1.0],
        }
    }
    #[must_use]
    pub fn jazz() -> Self {
        Self {
            name: "Jazz".to_string(),
            gains: [2.0, 1.5, 0.5, 1.0, -1.0, -1.0, 0.0, 1.5, 2.0, 2.5],
        }
    }
    #[must_use]
    pub fn classical() -> Self {
        Self {
            name: "Classical".to_string(),
            gains: [2.5, 2.0, 1.5, 0.5, -0.5, -0.5, 0.0, 1.5, 2.0, 2.5],
        }
    }
    #[must_use]
    pub fn electronic() -> Self {
        Self {
            name: "Electronic".to_string(),
            gains: [3.5, 3.0, 0.5, 0.0, -1.0, 1.0, 0.5, 2.0, 2.5, 3.5],
        }
    }
    #[must_use]
    pub fn acoustic() -> Self {
        Self {
            name: "Acoustic".to_string(),
            gains: [2.0, 1.5, 0.5, 0.5, 1.5, 1.5, 1.5, 2.0, 1.5, 0.5],
        }
    }
}

#[must_use]
pub fn get_all_presets() -> Vec<EqPreset> {
    vec![
        EqPreset::bass_boost(),
        EqPreset::treble_boost(),
        EqPreset::vocal(),
        EqPreset::rock(),
        EqPreset::pop(),
        EqPreset::jazz(),
        EqPreset::classical(),
        EqPreset::electronic(),
        EqPreset::acoustic(),
    ]
}

#[derive(Debug, Clone, Copy)]
pub struct BiquadCoefficients {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for BiquadCoefficients {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl BiquadCoefficients {
    /// 根据频段序号设计该 band 的滤波器:首尾用 shelf,中间用 peaking。
    ///
    /// 两端用 shelf 是因为用户对 31Hz/16kHz 的直觉是"低音/高音整体多一点",
    /// peaking 的钟形响应只在中心隆起、两端回落,容易诱导用户过量提升。
    #[must_use]
    pub fn for_band(sample_rate: f32, band: usize, gain_db: f32) -> Self {
        let freq = EQ_FREQUENCIES[band];
        if band == 0 {
            Self::low_shelf(sample_rate, freq, gain_db)
        } else if band == EQ_BAND_COUNT - 1 {
            Self::high_shelf(sample_rate, freq, gain_db)
        } else {
            Self::peaking_eq(sample_rate, freq, gain_db, EQ_Q_VALUES[band])
        }
    }

    /// RBJ peaking EQ。系数用 f64 计算后转 f32 存储以保留精度
    /// (31Hz@44.1kHz 的归一化频率极低,f32 直接计算会有可观的系数量化误差)。
    #[must_use]
    pub fn peaking_eq(sample_rate: f32, frequency: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        let sr = f64::from(sample_rate);
        let f0 = f64::from(frequency);
        let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
        let omega = std::f64::consts::TAU * f0 / sr;
        let (sin_omega, cos_omega) = omega.sin_cos();
        let alpha = sin_omega / (2.0 * f64::from(q));
        let b0 = 1.0 + alpha * a;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: (b0 / a0) as f32,
            b1: (-2.0 * cos_omega / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (-2.0 * cos_omega / a0) as f32,
            a2: (a2 / a0) as f32,
        }
    }

    /// RBJ low shelf(Q = 1/√2,等价于 shelf slope S = 1 的 Butterworth 特性)。
    #[must_use]
    pub fn low_shelf(sample_rate: f32, frequency: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        Self::shelf(sample_rate, frequency, gain_db, false)
    }

    /// RBJ high shelf(参数同 [`BiquadCoefficients::low_shelf`])。
    #[must_use]
    pub fn high_shelf(sample_rate: f32, frequency: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        Self::shelf(sample_rate, frequency, gain_db, true)
    }

    fn shelf(sample_rate: f32, frequency: f32, gain_db: f32, high: bool) -> Self {
        let sr = f64::from(sample_rate);
        let f0 = f64::from(frequency);
        let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
        let omega = std::f64::consts::TAU * f0 / sr;
        let (sin_omega, cos_omega) = omega.sin_cos();
        // Q = 1/√2 → alpha = sin/2·√2,即 S = 1
        let alpha = sin_omega / std::f64::consts::SQRT_2;
        let term = 2.0 * a.sqrt() * alpha;
        let (a_plus, a_minus) = (a + 1.0, a - 1.0);

        let (b0, b1, b2, a0, a1, a2) = if high {
            (
                a * (a_plus + a_minus * cos_omega + term),
                -2.0 * a * (a_minus + a_plus * cos_omega),
                a * (a_plus + a_minus * cos_omega - term),
                a_plus - a_minus * cos_omega + term,
                2.0 * (a_minus - a_plus * cos_omega),
                a_plus - a_minus * cos_omega - term,
            )
        } else {
            (
                a * (a_plus - a_minus * cos_omega + term),
                2.0 * a * (a_minus - a_plus * cos_omega),
                a * (a_plus - a_minus * cos_omega - term),
                a_plus + a_minus * cos_omega + term,
                -2.0 * (a_minus + a_plus * cos_omega),
                a_plus + a_minus * cos_omega - term,
            )
        };
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    pub fn process(&mut self, input: f32, coeffs: &BiquadCoefficients) -> f32 {
        let output = coeffs.b0 * input + coeffs.b1 * self.x1 + coeffs.b2 * self.x2
            - coeffs.a1 * self.y1
            - coeffs.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqSettings {
    pub enabled: bool,
    pub gains: [f32; EQ_BAND_COUNT],
    pub preamp: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gains: [0.0; EQ_BAND_COUNT],
            preamp: 0.0,
        }
    }
}

impl EqSettings {
    /// 有效 preamp(dB) = 用户 preamp - 最大提升量。
    ///
    /// 均衡器提升某频段后,该频段的峰值可能超过 0dBFS 触发 soft clip 全曲失真。
    /// 自动把整体电平拉低最大提升量即可保证峰值不超(削减 band 只会降低电平,
    /// 不需要补偿)。与专业均衡器(Equalizer APO / VLC auto gain)行为一致。
    #[must_use]
    pub fn effective_preamp_db(&self) -> f32 {
        self.preamp - self.gains.iter().copied().fold(0.0_f32, f32::max)
    }
}

pub struct Equalizer {
    settings: Arc<RwLock<EqSettings>>,
    coefficients: Vec<BiquadCoefficients>,
    states: Vec<Vec<BiquadState>>,
    sample_rate: f32,
    channels: usize,
}

impl Equalizer {
    #[must_use]
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let mut eq = Self {
            settings: Arc::new(RwLock::new(EqSettings::default())),
            coefficients: vec![BiquadCoefficients::default(); EQ_BAND_COUNT],
            states: vec![vec![BiquadState::default(); channels as usize]; EQ_BAND_COUNT],
            sample_rate: sample_rate as f32,
            channels: channels as usize,
        };
        eq.update_coefficients();
        eq
    }

    #[must_use]
    pub fn get_settings_handle(&self) -> Arc<RwLock<EqSettings>> {
        Arc::clone(&self.settings)
    }

    pub fn update_coefficients(&mut self) {
        let settings = lock_or_log!(self.settings.read());
        for (i, &gain) in settings.gains.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::for_band(self.sample_rate, i, gain);
        }
    }

    pub fn process_sample(&mut self, input: f32, channel: usize) -> f32 {
        let settings = lock_or_log!(self.settings.read());
        if !settings.enabled {
            return input;
        }
        let preamp_gain = 10.0_f32.powf(settings.effective_preamp_db() / 20.0);
        let mut sample = input * preamp_gain;
        drop(settings);
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            sample = self.states[band][channel].process(sample, coeffs);
        }
        soft_clip(sample)
    }

    pub fn process_buffer(&mut self, buffer: &mut [f32]) {
        let settings = lock_or_log!(self.settings.read());
        if !settings.enabled {
            return;
        }
        let preamp_gain = 10.0_f32.powf(settings.effective_preamp_db() / 20.0);
        drop(settings);
        for (i, sample) in buffer.iter_mut().enumerate() {
            let channel = i % self.channels;
            let mut s = *sample * preamp_gain;
            for (band, coeffs) in self.coefficients.iter().enumerate() {
                s = self.states[band][channel].process(s, coeffs);
            }
            *sample = soft_clip(s);
        }
    }

    pub fn reset(&mut self) {
        for band_states in &mut self.states {
            for state in band_states {
                state.reset();
            }
        }
    }

    pub fn set_gains(&mut self, gains: [f32; EQ_BAND_COUNT]) {
        lock_or_log!(self.settings.write()).gains = gains;
        self.update_coefficients();
    }

    pub fn set_band_gain(&mut self, band: usize, gain: f32) {
        if band < EQ_BAND_COUNT {
            lock_or_log!(self.settings.write()).gains[band] = gain.clamp(-8.0, 8.0);
            self.update_coefficients();
        }
    }

    pub fn set_preamp(&mut self, preamp: f32) {
        lock_or_log!(self.settings.write()).preamp = preamp.clamp(-8.0, 8.0);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        lock_or_log!(self.settings.write()).enabled = enabled;
    }

    #[must_use]
    pub fn get_settings(&self) -> EqSettings {
        lock_or_log!(self.settings.read()).clone()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate as f32;
        self.update_coefficients();
    }
}

fn soft_clip(x: f32) -> f32 {
    // 软限制器: 在 DAC 0dBFS 之前平滑压缩,避免硬削波破音
    //
    // 设计要点:
    // - 阈值 0.9: 留 10% 余量才开始压缩,对正常音量音乐几乎无影响
    // - 上限 0.97: 给 DAC 留 3% 余量,避免硬件削波
    // - tanh 曲线: 在阈值处导数连续(=1),从线性段平滑过渡到压缩段,无折角
    //   y = threshold + range * tanh((|x| - threshold) / range)
    //   在 |x|=threshold 时 dy/dx = 1(与线性段匹配)
    //   当 |x|→∞ 时 y → threshold + range = ceiling
    let threshold = 0.9;
    let ceiling = 0.97;
    let abs_x = x.abs();
    if abs_x <= threshold {
        x
    } else {
        let sign = x.signum();
        let over = abs_x - threshold;
        let range = ceiling - threshold;
        sign * (threshold + range * (over / range).tanh())
    }
}

pub struct GlobalEqualizer {
    settings: Arc<RwLock<EqSettings>>,
}

impl GlobalEqualizer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(EqSettings::default())),
        }
    }

    #[must_use]
    pub fn get_settings_handle(&self) -> Arc<RwLock<EqSettings>> {
        Arc::clone(&self.settings)
    }

    #[must_use]
    pub fn get_settings(&self) -> EqSettings {
        lock_or_log!(self.settings.read()).clone()
    }

    pub fn set_settings(&self, settings: EqSettings) {
        *lock_or_log!(self.settings.write()) = settings;
    }

    pub fn set_enabled(&self, enabled: bool) {
        lock_or_log!(self.settings.write()).enabled = enabled;
    }

    pub fn set_gains(&self, gains: [f32; EQ_BAND_COUNT]) {
        lock_or_log!(self.settings.write()).gains = gains;
    }

    pub fn set_band_gain(&self, band: usize, gain: f32) {
        if band < EQ_BAND_COUNT {
            lock_or_log!(self.settings.write()).gains[band] = gain.clamp(-8.0, 8.0);
        }
    }

    pub fn set_preamp(&self, preamp: f32) {
        lock_or_log!(self.settings.write()).preamp = preamp.clamp(-8.0, 8.0);
    }
}

impl Default for GlobalEqualizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_peaking_eq_zero_gain_returns_default() {
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 0.0, 1.0);
        let default = BiquadCoefficients::default();
        assert!(approx_eq(coeffs.b0, default.b0));
        assert!(approx_eq(coeffs.b1, default.b1));
        assert!(approx_eq(coeffs.b2, default.b2));
        assert!(approx_eq(coeffs.a1, default.a1));
        assert!(approx_eq(coeffs.a2, default.a2));
    }

    #[test]
    fn test_peaking_eq_near_zero_gain_returns_default() {
        // |gain_db| < 0.001 也应返回 default (恒等滤波器)
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 0.0005, 1.0);
        let default = BiquadCoefficients::default();
        assert!(approx_eq(coeffs.b0, default.b0));
    }

    #[test]
    fn test_peaking_eq_nonzero_gain_not_identity() {
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 6.0, 1.0);
        assert!(
            !approx_eq(coeffs.b0, 1.0),
            "b0 should differ from 1.0 with non-zero gain"
        );
        assert!(
            !approx_eq(coeffs.b1, 0.0),
            "b1 should differ from 0.0 with non-zero gain"
        );
        // peaking EQ 归一化后 a1 == b1
        assert!(approx_eq(coeffs.a1, coeffs.b1));
    }

    #[test]
    fn test_biquad_process_with_identity_coeffs() {
        let mut state = BiquadState::default();
        let coeffs = BiquadCoefficients::default();
        for &x in &[0.5_f32, -0.3, 0.8, -0.1, 0.0, 1.0] {
            let y = state.process(x, &coeffs);
            assert!(approx_eq(y, x), "identity filter output {y} != input {x}");
        }
    }

    #[test]
    fn test_biquad_reset_clears_state() {
        let mut state = BiquadState::default();
        let coeffs = BiquadCoefficients {
            b0: 0.5,
            b1: 0.5,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        };
        // 处理若干样本填充状态
        let _ = state.process(1.0, &coeffs);
        let _ = state.process(0.5, &coeffs);
        // reset 后用恒等系数处理,输出应等于输入
        state.reset();
        let identity = BiquadCoefficients::default();
        let y = state.process(0.7, &identity);
        assert!(
            approx_eq(y, 0.7),
            "after reset, identity output {y} != input 0.7"
        );
    }

    #[test]
    fn test_equalizer_new_defaults() {
        let eq = Equalizer::new(48000, 2);
        let settings = eq.get_settings();
        assert!(!settings.enabled, "default enabled should be false");
        assert!(settings.preamp.abs() < 1e-5, "default preamp should be 0");
        assert_eq!(settings.gains.len(), EQ_BAND_COUNT);
        for &g in &settings.gains {
            assert!(g.abs() < 1e-5, "default gains should be 0");
        }
    }

    #[test]
    fn test_equalizer_disabled_passthrough() {
        let mut eq = Equalizer::new(48000, 2);
        // enabled 默认为 false
        for &x in &[0.5_f32, -0.3, 0.8, 0.0] {
            let y = eq.process_sample(x, 0);
            assert!(approx_eq(y, x), "disabled EQ output {y} != input {x}");
        }
    }

    #[test]
    fn test_equalizer_enabled_changes_signal() {
        let mut eq = Equalizer::new(48000, 2);
        // 提升 1 kHz 频段 (索引 5) +6 dB
        eq.set_band_gain(5, 6.0);
        eq.set_enabled(true);
        let sample_rate = 48000.0_f32;
        let freq = 1000.0_f32;
        let mut input_energy = 0.0_f64;
        let mut output_energy = 0.0_f64;
        for n in 0..480i32 {
            let t = n as f32 / sample_rate;
            let x = (2.0 * PI * freq * t).sin() * 0.5;
            let y = eq.process_sample(x, 0);
            input_energy += f64::from(x) * f64::from(x);
            output_energy += f64::from(y) * f64::from(y);
        }
        assert!(
            (output_energy - input_energy).abs() > 1e-4,
            "enabled EQ should change signal energy: in={input_energy}, out={output_energy}"
        );
    }

    #[test]
    fn test_effective_preamp_compensates_max_boost() {
        let mut settings = EqSettings::default();
        assert!(
            approx_eq(settings.effective_preamp_db(), 0.0),
            "flat EQ needs no compensation"
        );
        settings.gains[5] = 4.0;
        settings.gains[2] = 2.0;
        settings.preamp = -1.0;
        assert!(
            approx_eq(settings.effective_preamp_db(), -5.0),
            "effective preamp should subtract the max boost (4dB), got {}",
            settings.effective_preamp_db()
        );
        // 纯削减不需要补偿
        settings.gains[5] = 0.0;
        settings.gains[2] = -3.0;
        assert!(
            approx_eq(settings.effective_preamp_db(), -1.0),
            "cut-only EQ should not be compensated"
        );
    }

    /// 稳态正弦幅度(丢弃前 warmup 个采样,取峰值)
    fn steady_state_amplitude(
        eq: &mut Equalizer,
        freq: f32,
        amplitude: f32,
        channel: usize,
    ) -> f32 {
        let sample_rate = 48000.0_f32;
        let warmup = 4800;
        let mut peak = 0.0_f32;
        for n in 0..warmup * 2 {
            let t = n as f32 / sample_rate;
            let x = (2.0 * PI * freq * t).sin() * amplitude;
            let y = eq.process_sample(x, channel);
            if n >= warmup {
                peak = peak.max(y.abs());
            }
        }
        peak
    }

    #[test]
    fn test_low_shelf_boosts_bass_region() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(0, 6.0); // 31Hz low shelf
        // 设 preamp = 最大提升量,抵消自动增益补偿,以便单独验证滤波器响应
        eq.set_preamp(6.0);
        eq.set_enabled(true);
        // 8Hz 位于 shelf 通带深处,应获得接近 +6dB (×2) 的增益
        let boosted = steady_state_amplitude(&mut eq, 8.0, 0.25, 0);
        assert!(
            (boosted - 0.5).abs() < 0.06,
            "8Hz should be boosted ~+6dB by 31Hz low shelf, got peak {boosted}"
        );
        // 1kHz 应基本不受影响(shelf 拐点以上的阻带)
        let untouched = steady_state_amplitude(&mut eq, 1000.0, 0.25, 0);
        assert!(
            (untouched - 0.25).abs() < 0.03,
            "1kHz should be nearly unaffected by 31Hz low shelf, got peak {untouched}"
        );
    }

    #[test]
    fn test_high_shelf_boosts_treble_region() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(9, 6.0); // 16kHz high shelf
        eq.set_preamp(6.0);
        eq.set_enabled(true);
        // 22kHz 位于 shelf 通带深处(接近但不越过 Nyquist 24kHz)
        let boosted = steady_state_amplitude(&mut eq, 22000.0, 0.25, 0);
        assert!(
            (boosted - 0.5).abs() < 0.06,
            "22kHz should be boosted ~+6dB by 16kHz high shelf, got peak {boosted}"
        );
        let untouched = steady_state_amplitude(&mut eq, 500.0, 0.25, 0);
        assert!(
            (untouched - 0.25).abs() < 0.03,
            "500Hz should be nearly unaffected by 16kHz high shelf, got peak {untouched}"
        );
    }

    #[test]
    fn test_auto_preamp_prevents_clipping_on_boost() {
        // +8dB boost + 满幅正弦:自动补偿后输出峰值不应超过 soft clip 阈值
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(5, 8.0);
        eq.set_enabled(true);
        let peak = steady_state_amplitude(&mut eq, 1000.0, 0.95, 0);
        assert!(
            peak <= 0.96,
            "auto preamp should keep boosted peak <=0dBFS, got {peak}"
        );
    }

    #[test]
    fn test_set_band_gain_clamps_high() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(0, 100.0);
        let settings = eq.get_settings();
        assert!(
            approx_eq(settings.gains[0], 8.0),
            "gain over +8 should clamp to 8"
        );
    }

    #[test]
    fn test_set_band_gain_clamps_low() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(0, -100.0);
        let settings = eq.get_settings();
        assert!(
            approx_eq(settings.gains[0], -8.0),
            "gain below -8 should clamp to -8"
        );
    }

    #[test]
    fn test_set_band_gain_out_of_range_ignored() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(EQ_BAND_COUNT + 5, 5.0);
        let settings = eq.get_settings();
        for &g in &settings.gains {
            assert!(g.abs() < 1e-5, "out-of-range band gain should be ignored");
        }
    }

    #[test]
    fn test_set_preamp_clamps_high() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_preamp(100.0);
        assert!(approx_eq(eq.get_settings().preamp, 8.0));
    }

    #[test]
    fn test_set_preamp_clamps_low() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_preamp(-100.0);
        assert!(approx_eq(eq.get_settings().preamp, -8.0));
    }

    #[test]
    fn test_eq_preset_flat_is_all_zero() {
        let preset = EqPreset::flat();
        assert_eq!(preset.name, "Flat");
        assert_eq!(preset.gains.len(), EQ_BAND_COUNT);
        for &g in &preset.gains {
            assert!(g.abs() < 1e-5, "flat preset gain should be 0");
        }
    }

    #[test]
    fn test_eq_preset_gains_length() {
        let presets = [
            EqPreset::flat(),
            EqPreset::bass_boost(),
            EqPreset::treble_boost(),
            EqPreset::vocal(),
            EqPreset::rock(),
            EqPreset::pop(),
            EqPreset::jazz(),
            EqPreset::classical(),
            EqPreset::electronic(),
            EqPreset::acoustic(),
        ];
        for preset in &presets {
            assert_eq!(
                preset.gains.len(),
                EQ_BAND_COUNT,
                "preset {} gains length wrong",
                preset.name
            );
        }
    }

    #[test]
    fn test_get_all_presets_count() {
        let presets = get_all_presets();
        assert_eq!(presets.len(), 9, "get_all_presets should return 9 presets");
        for preset in &presets {
            assert_eq!(preset.gains.len(), EQ_BAND_COUNT);
        }
    }

    #[test]
    fn test_soft_clip_below_threshold_passthrough() {
        // |x| <= 0.9 时返回 x (阈值从 0.95 降至 0.9)
        for &x in &[0.0_f32, 0.1, 0.5, 0.89, 0.9, -0.89, -0.9, -0.5, -0.1] {
            let y = soft_clip(x);
            assert!(approx_eq(y, x), "soft_clip({x}) = {y}, expected {x}");
        }
    }

    #[test]
    fn test_soft_clip_above_threshold_compresses() {
        // |x| > 0.9 时压缩,输出幅度减小且符号保持
        // 上限为 0.97 (给 DAC 留 3% 余量)
        for &x in &[
            0.91_f32, 0.95, 1.0, 1.5, 2.0, 10.0, -0.91, -0.95, -1.0, -1.5, -2.0,
        ] {
            let y = soft_clip(x);
            assert!(
                y.abs() < x.abs(),
                "soft_clip should reduce magnitude: |{x}| -> |{y}|"
            );
            assert!(
                y.abs() <= 0.97 + 1e-5,
                "soft_clip output |{y}| should be <= 0.97"
            );
            assert!(x * y >= 0.0, "soft_clip should preserve sign: x={x}, y={y}");
        }
    }

    #[test]
    fn test_soft_clip_ceiling_at_extreme_input() {
        // 极端输入应趋近上限 0.97 而非 1.0
        let y = soft_clip(100.0);
        assert!(
            y < 0.971,
            "soft_clip(100) = {y}, should approach 0.97 ceiling"
        );
        assert!(
            y > 0.969,
            "soft_clip(100) = {y}, should be close to 0.97 ceiling"
        );
    }

    #[test]
    fn test_soft_clip_derivative_continuous_at_threshold() {
        // 验证阈值处导数连续: 阈值附近的小增量应产生近似线性的响应
        let threshold = 0.9_f32;
        let eps = 1e-4;
        let below = soft_clip(threshold - eps);
        let at = soft_clip(threshold);
        let above = soft_clip(threshold + eps);
        // 阈值处函数值连续: 两侧增量应近似 eps
        let diff_below = (below - at).abs();
        let diff_above = (above - at).abs();
        assert!(
            diff_below <= eps * 1.05,
            "should be continuous at threshold from below: diff={diff_below}, eps={eps}"
        );
        assert!(
            diff_above <= eps * 1.05,
            "should be continuous at threshold from above: diff={diff_above}, eps={eps}"
        );
        // 导数连续: 两侧斜率应接近 (tanh 在 over=0 时斜率=1,与线性段匹配)
        let slope_below = diff_below / eps;
        let slope_above = diff_above / eps;
        assert!(
            (slope_below - slope_above).abs() < 0.05,
            "slopes should be close: below={slope_below}, above={slope_above}"
        );
    }

    #[test]
    fn test_process_buffer_disabled_passthrough() {
        let mut eq = Equalizer::new(48000, 2);
        let original = [0.1_f32, -0.2, 0.3, -0.4, 0.5, -0.6];
        let mut buffer = original;
        eq.process_buffer(&mut buffer);
        for (i, (a, b)) in original.iter().zip(buffer.iter()).enumerate() {
            assert!(
                approx_eq(*a, *b),
                "buffer[{i}] changed when disabled: {a} -> {b}"
            );
        }
    }

    #[test]
    fn test_process_buffer_multichannel_interleaved() {
        let mut eq = Equalizer::new(48000, 2);
        eq.set_band_gain(5, 4.0);
        eq.set_enabled(true);

        let n_frames: usize = 480;
        let mut buffer: Vec<f32> = (0..n_frames * 2)
            .map(|i| {
                let frame = i / 2;
                let t = frame as f32 / 48000.0_f32;
                (2.0 * PI * 1000.0 * t).sin() * 0.3
            })
            .collect();
        let original = buffer.clone();
        eq.process_buffer(&mut buffer);

        // 逐样本对比 process_buffer 与 process_sample 结果一致
        let mut eq2 = Equalizer::new(48000, 2);
        eq2.set_band_gain(5, 4.0);
        eq2.set_enabled(true);
        for (i, &s) in original.iter().enumerate() {
            let channel = i % 2;
            let y = eq2.process_sample(s, channel);
            assert!(
                approx_eq(y, buffer[i]),
                "buffer[{i}] (={}) != process_sample (={y})",
                buffer[i]
            );
        }

        // 确认 EQ 启用时 buffer 确实被修改
        let any_changed = original
            .iter()
            .zip(buffer.iter())
            .any(|(a, b)| (a - b).abs() > 1e-4);
        assert!(
            any_changed,
            "process_buffer should modify samples with EQ enabled"
        );
    }
}
