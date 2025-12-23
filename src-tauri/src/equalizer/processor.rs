//! EQ 均衡器模块
//!
//! 实现 10 段参数均衡器，支持实时调节。

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;
use std::sync::{Arc, RwLock};

pub const EQ_BAND_COUNT: usize = 10;
pub const EQ_FREQUENCIES: [f32; EQ_BAND_COUNT] = [31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];
pub const EQ_Q_VALUES: [f32; EQ_BAND_COUNT] = [0.7, 0.7, 0.8, 0.9, 1.0, 1.0, 1.1, 1.2, 1.3, 1.4];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct EqBand {
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        Self { frequency: 1000.0, gain: 0.0, q: 1.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPreset {
    pub name: String,
    pub gains: [f32; EQ_BAND_COUNT],
}

impl EqPreset {
    #[must_use] pub fn flat() -> Self { Self { name: "Flat".to_string(), gains: [0.0; EQ_BAND_COUNT] } }
    #[must_use] pub fn bass_boost() -> Self { Self { name: "Bass Boost".to_string(), gains: [4.0, 3.5, 2.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] } }
    #[must_use] pub fn treble_boost() -> Self { Self { name: "Treble Boost".to_string(), gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 2.5, 3.5, 4.0] } }
    #[must_use] pub fn vocal() -> Self { Self { name: "Vocal".to_string(), gains: [-1.5, -1.0, 0.0, 1.5, 2.5, 2.5, 2.0, 0.5, 0.0, -0.5] } }
    #[must_use] pub fn rock() -> Self { Self { name: "Rock".to_string(), gains: [3.5, 2.5, 1.5, 0.0, -0.5, 0.0, 1.5, 2.0, 2.5, 3.0] } }
    #[must_use] pub fn pop() -> Self { Self { name: "Pop".to_string(), gains: [-0.5, 0.0, 1.5, 2.0, 2.5, 2.0, 0.5, 0.0, -0.5, -1.0] } }
    #[must_use] pub fn jazz() -> Self { Self { name: "Jazz".to_string(), gains: [2.0, 1.5, 0.5, 1.0, -1.0, -1.0, 0.0, 1.5, 2.0, 2.5] } }
    #[must_use] pub fn classical() -> Self { Self { name: "Classical".to_string(), gains: [2.5, 2.0, 1.5, 0.5, -0.5, -0.5, 0.0, 1.5, 2.0, 2.5] } }
    #[must_use] pub fn electronic() -> Self { Self { name: "Electronic".to_string(), gains: [3.5, 3.0, 0.5, 0.0, -1.0, 1.0, 0.5, 2.0, 2.5, 3.5] } }
    #[must_use] pub fn acoustic() -> Self { Self { name: "Acoustic".to_string(), gains: [2.0, 1.5, 0.5, 0.5, 1.5, 1.5, 1.5, 2.0, 1.5, 0.5] } }
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
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 }
    }
}

impl BiquadCoefficients {
    #[must_use]
    pub fn peaking_eq(sample_rate: f32, frequency: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < 0.001 { return Self::default(); }
        let a = 10.0_f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * frequency / sample_rate;
        let (sin_omega, cos_omega) = (omega.sin(), omega.cos());
        let alpha = sin_omega / (2.0 * q);
        let (b0, b1, b2) = (1.0 + alpha * a, -2.0 * cos_omega, 1.0 - alpha * a);
        let (a0, a1, a2) = (1.0 + alpha / a, -2.0 * cos_omega, 1.0 - alpha / a);
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
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
        let output = coeffs.b0 * input + coeffs.b1 * self.x1 + coeffs.b2 * self.x2 - coeffs.a1 * self.y1 - coeffs.a2 * self.y2;
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
        Self { enabled: false, gains: [0.0; EQ_BAND_COUNT], preamp: 0.0 }
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
        let settings = self.settings.read().unwrap();
        for (i, &freq) in EQ_FREQUENCIES.iter().enumerate() {
            self.coefficients[i] = BiquadCoefficients::peaking_eq(self.sample_rate, freq, settings.gains[i], EQ_Q_VALUES[i]);
        }
    }

    pub fn process_sample(&mut self, input: f32, channel: usize) -> f32 {
        let settings = self.settings.read().unwrap();
        if !settings.enabled { return input; }
        let preamp_gain = 10.0_f32.powf(settings.preamp / 20.0);
        let mut sample = input * preamp_gain;
        drop(settings);
        for (band, coeffs) in self.coefficients.iter().enumerate() {
            sample = self.states[band][channel].process(sample, coeffs);
        }
        soft_clip(sample)
    }

    pub fn process_buffer(&mut self, buffer: &mut [f32]) {
        let settings = self.settings.read().unwrap();
        if !settings.enabled { return; }
        let preamp_gain = 10.0_f32.powf(settings.preamp / 20.0);
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
        self.settings.write().unwrap().gains = gains;
        self.update_coefficients();
    }

    pub fn set_band_gain(&mut self, band: usize, gain: f32) {
        if band < EQ_BAND_COUNT {
            self.settings.write().unwrap().gains[band] = gain.clamp(-8.0, 8.0);
            self.update_coefficients();
        }
    }

    pub fn set_preamp(&mut self, preamp: f32) {
        self.settings.write().unwrap().preamp = preamp.clamp(-8.0, 8.0);
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.settings.write().unwrap().enabled = enabled;
    }

    #[must_use]
    pub fn get_settings(&self) -> EqSettings {
        self.settings.read().unwrap().clone()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate as f32;
        self.update_coefficients();
    }
}

fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.9 { x }
    else if x > 0.0 { 0.9 + 0.1 * ((x - 0.9) / 0.1).tanh() }
    else { -0.9 - 0.1 * ((-x - 0.9) / 0.1).tanh() }
}

pub struct GlobalEqualizer {
    settings: Arc<RwLock<EqSettings>>,
}

impl GlobalEqualizer {
    #[must_use]
    pub fn new() -> Self {
        Self { settings: Arc::new(RwLock::new(EqSettings::default())) }
    }

    #[must_use]
    pub fn get_settings_handle(&self) -> Arc<RwLock<EqSettings>> {
        Arc::clone(&self.settings)
    }

    #[must_use]
    pub fn get_settings(&self) -> EqSettings {
        self.settings.read().unwrap().clone()
    }

    pub fn set_settings(&self, settings: EqSettings) {
        *self.settings.write().unwrap() = settings;
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.settings.write().unwrap().enabled = enabled;
    }

    pub fn set_gains(&self, gains: [f32; EQ_BAND_COUNT]) {
        self.settings.write().unwrap().gains = gains;
    }

    pub fn set_band_gain(&self, band: usize, gain: f32) {
        if band < EQ_BAND_COUNT {
            self.settings.write().unwrap().gains[band] = gain.clamp(-8.0, 8.0);
        }
    }

    pub fn set_preamp(&self, preamp: f32) {
        self.settings.write().unwrap().preamp = preamp.clamp(-8.0, 8.0);
    }
}

impl Default for GlobalEqualizer {
    fn default() -> Self {
        Self::new()
    }
}
