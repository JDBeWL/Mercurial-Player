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

fn emit_spectrum_update(app: &AppHandle, data: &[f32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    app.emit("spectrum-update", SpectrumUpdateEvent {
        data: data.to_vec(),
        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64,
    })?;
    Ok(())
}

pub struct VisualizationSource<I: Source<Item = f32> + Send> {
    input: I,
    waveform_data: Arc<Mutex<Vec<f32>>>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
    app_handle: Option<AppHandle>,
    last_emit_time: std::sync::atomic::AtomicU64,
    eq_settings: Arc<RwLock<EqSettings>>,
    eq_processor: EqProcessor,
    // 缓存的 EQ 设置，减少锁读取频率
    cached_eq_enabled: bool,
    eq_update_counter: u32,
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

impl<I: Source<Item = f32> + Send> VisualizationSource<I> {
    pub fn new(input: I, waveform_data: Arc<Mutex<Vec<f32>>>, spectrum_data: Arc<Mutex<Vec<f32>>>, app_handle: Option<AppHandle>) -> Self {
        let (sr, ch) = (input.sample_rate(), input.channels());
        Self {
            input,
            waveform_data,
            spectrum_data,
            buffer: Vec::with_capacity(1024),
            prev_spectrum: vec![0.0; 128],
            app_handle,
            last_emit_time: std::sync::atomic::AtomicU64::new(0),
            eq_settings: Arc::new(RwLock::new(EqSettings::default())),
            eq_processor: EqProcessor::new(sr, ch),
            cached_eq_enabled: false,
            eq_update_counter: 0,
        }
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
        if self.buffer.len() >= 1024 {
            if let Ok(mut spec) = self.spectrum_data.try_lock() {
                let hann = hann_window(&self.buffer);
                if let Ok(spectrum) = samples_fft_to_spectrum(&hann, self.input.sample_rate(), FrequencyLimit::Range(20.0, 20000.0), Some(&divide_by_N_sqrt)) {
                    let mut new_spec = vec![0.0; 128];
                    let (log_min, log_max) = (180.0_f32.log10(), 20000.0_f32.log10());
                    let log_step = (log_max - log_min) / 128.0;
                    for (freq, value) in spectrum.data() {
                        let f = freq.val();
                        if (180.0..=20000.0).contains(&f) {
                            let bin = ((f.log10() - log_min) / log_step).floor() as usize;
                            if bin < 128 && value.val() > new_spec[bin] { new_spec[bin] = value.val(); }
                        }
                    }
                    for i in 0..128 { self.prev_spectrum[i] = self.prev_spectrum[i] * 0.5 + new_spec[i] * 0.5; }
                    *spec = self.prev_spectrum.clone();
                }
            }
            if let Ok(mut wave) = self.waveform_data.try_lock() { *wave = self.buffer.clone(); }
            if let Some(ref app) = self.app_handle {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
                let last = self.last_emit_time.load(Ordering::Relaxed);
                if now - last >= 16 && self.spectrum_data.try_lock().is_ok() {
                    let _ = emit_spectrum_update(app, &self.prev_spectrum);
                    self.last_emit_time.store(now, Ordering::Relaxed);
                }
            }
            self.buffer.clear();
        }
        Some(processed)
    }
}

impl<I: Source<Item = f32> + Send> Source for VisualizationSource<I> {
    fn current_frame_len(&self) -> Option<usize> { self.input.current_frame_len() }
    fn channels(&self) -> u16 { self.input.channels() }
    fn sample_rate(&self) -> u32 { self.input.sample_rate() }
    fn total_duration(&self) -> Option<Duration> { self.input.total_duration() }
}

/// 播放音轨（共享模式）
pub fn play_track_shared(app: &AppHandle, state: &State<AppState>, path: &str, position: Option<f32>) -> Result<(), String> {
    let player = &state.player;
    {
        let sink = player.sink.lock().unwrap();
        // 先淡出当前音频，避免爆音
        let current_vol = sink.volume();
        if current_vol > 0.0 && !sink.empty() {
            // 快速淡出
            for i in (0..10).rev() {
                sink.set_volume(current_vol * (i as f32 / 10.0));
                std::thread::sleep(Duration::from_millis(5));
            }
        }
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
            if let Some(t) = position { let _ = dec.seek(Duration::from_secs_f32(t)); }
            let _ = dec.prefill_buffer();
            println!("使用Symphonia解码器: {path}");
            // 添加淡入效果
            Box::new(
                VisualizationSource::new(LockFreeSymphoniaSource::new(dec), waveform, spectrum, Some(app.clone()))
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(50))
            )
        }
        Err(e) => {
            println!("Symphonia解码失败，回退到rodio: {e}");
            let file = File::open(path).map_err(|e| e.to_string())?;
            Box::new(
                VisualizationSource::new(rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?.convert_samples::<f32>(), waveform, spectrum, Some(app.clone()))
                    .with_eq_settings(eq_settings)
                    .fade_in(Duration::from_millis(50))
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
    std::thread::sleep(Duration::from_millis(100));
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

    let mut wait = 0;
    while !thread_started.load(Ordering::SeqCst) && wait < 50 {
        std::thread::sleep(Duration::from_millis(10));
        wait += 1;
    }
    {
        if let Some(ref wasapi) = *player.wasapi_player.lock().unwrap() {
            std::thread::sleep(Duration::from_millis(150));
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

#[cfg(windows)]
fn decode_and_push_to_wasapi(
    mut source: LockFreeSymphoniaSource,
    wasapi: Arc<Mutex<Option<super::wasapi::WasapiExclusivePlayback>>>,
    _waveform: Arc<Mutex<Vec<f32>>>,
    _spectrum: Arc<Mutex<Vec<f32>>>,
    _app: AppHandle,
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
    let chunk_size = 1024;
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
            }
            break;
        }
        std::thread::sleep(Duration::from_micros(500));
    }
}

fn convert_channels(samples: &[f32], src_ch: u16, target_ch: u16) -> Vec<f32> {
    if src_ch == target_ch { return samples.to_vec(); }
    let (src, tgt) = (src_ch as usize, target_ch as usize);
    let frames = samples.len() / src;
    let mut out = Vec::with_capacity(frames * tgt);
    for f in 0..frames {
        let start = f * src;
        match (src, tgt) {
            (1, 2) => { let s = samples[start]; out.push(s); out.push(s); }
            (2, 1) => out.push((samples[start] + samples[start + 1]) / 2.0),
            _ => {
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
        ).with_eq_settings(eq_settings)
        .fade_in(Duration::from_millis(30)) // seek 时使用更短的淡入
    );
    {
        let sink = player.sink.lock().unwrap();
        // 快速淡出避免爆音
        let current_vol = sink.volume();
        if current_vol > 0.0 && !sink.empty() {
            for i in (0..5).rev() {
                sink.set_volume(current_vol * (i as f32 / 5.0));
                std::thread::sleep(Duration::from_millis(3));
            }
        }
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
    let volume = *state.player.target_volume.lock().unwrap();
    let is_playing = {
        let exclusive_mode = *state.player.exclusive_mode.lock().unwrap();
        if exclusive_mode {
            #[cfg(windows)]
            {
                state.player.wasapi_player.lock().unwrap().as_ref().map_or(false, |wasapi| wasapi.get_state() == PlaybackState::Playing)
            }
            #[cfg(not(windows))]
            {
                false
            }
        } else {
            !state.player.sink.lock().unwrap().is_paused()
        }
    };
    Ok(PlaybackStatus::new(is_playing, 0.0, volume))
}

/// 检查音轨是否播放完毕
pub fn check_track_finished(state: &State<AppState>) -> Result<bool, String> {
    let exclusive_mode = *state.player.exclusive_mode.lock().unwrap();
    if exclusive_mode {
        #[cfg(windows)]
        {
            Ok(state.player.wasapi_player.lock().unwrap().as_ref().map_or(true, |wasapi| wasapi.get_state() == PlaybackState::Stopped))
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    } else {
        let sink = state.player.sink.lock().unwrap();
        Ok(sink.empty() && !sink.is_paused())
    }
}
