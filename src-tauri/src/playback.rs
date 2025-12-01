//! 音频播放相关的 Tauri 命令
//!
//! 这个模块包含所有与音频播放控制相关的功能，包括播放、暂停、恢复、音量控制等

use super::audio_decoder::SymphoniaDecoder;
use super::AppState;
use rodio::Source;
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use spectrum_analyzer::windows::hann_window;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{command, State};

/// 表示当前音频播放器的状态
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    #[serde(rename = "isPlaying")]
    pub is_playing: bool,
    #[serde(rename = "positionSecs")]
    pub position_secs: f32,
    #[serde(rename = "volume")]
    pub volume: f32,
}

impl PlaybackStatus {
    pub const fn new(is_playing: bool, position_secs: f32, volume: f32) -> Self {
        Self {
            is_playing,
            position_secs,
            volume,
        }
    }
}

/// 用于可视化的音频源包装器
pub struct VisualizationSource<I>
where
    I: Source<Item = f32> + Send,
{
    input: I,
    waveform_data: Arc<Mutex<Vec<f32>>>,
    spectrum_data: Arc<Mutex<Vec<f32>>>,
    buffer: Vec<f32>,
    prev_spectrum: Vec<f32>,
}

impl<I> VisualizationSource<I>
where
    I: Source<Item = f32> + Send,
{
    pub fn new(
        input: I,
        waveform_data: Arc<Mutex<Vec<f32>>>,
        spectrum_data: Arc<Mutex<Vec<f32>>>,
    ) -> Self {
        Self {
            input,
            waveform_data,
            spectrum_data,
            buffer: Vec::with_capacity(1024),
            prev_spectrum: vec![0.0; 128],
        }
    }
}

impl<I> Iterator for VisualizationSource<I>
where
    I: Source<Item = f32> + Send,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next();
        if let Some(s) = sample {
            // 存入本地缓冲区用于FFT
            self.buffer.push(s);

            if self.buffer.len() >= 1024 {
                // 计算FFT
                let hann_window = hann_window(&self.buffer);
                // 44100Hz 采样率 (简化假设，实际应从 input.sample_rate() 获取)
                let spectrum_result = samples_fft_to_spectrum(
                    &hann_window,
                    44100,
                    FrequencyLimit::Range(20.0, 20000.0),
                    Some(&divide_by_N_sqrt),
                );

                if let Ok(spectrum) = spectrum_result {
                    // 对数频段映射 (128 bands)
                    let mut new_spectrum = vec![0.0; 128];
                    let data = spectrum.data();

                    // 频率范围 20Hz - 20000Hz
                    let min_freq = 20.0f32;
                    let max_freq = 20000.0f32;
                    let log_min = min_freq.log10();
                    let log_max = max_freq.log10();
                    let log_step = (log_max - log_min) / 128.0;

                    for (freq, value) in data {
                        let freq_val = freq.val();
                        if freq_val < min_freq || freq_val > max_freq {
                            continue;
                        }

                        // 计算当前频率属于哪个 bin
                        let bin_index = ((freq_val.log10() - log_min) / log_step).floor() as usize;

                        if bin_index < 128 {
                            // 取最大值
                            if value.val() > new_spectrum[bin_index] {
                                new_spectrum[bin_index] = value.val();
                            }
                        }
                    }

                    // 平滑处理 (EMA)
                    for i in 0..128 {
                        self.prev_spectrum[i] = self.prev_spectrum[i] * 0.5 + new_spectrum[i] * 0.5;
                    }

                    // 更新共享状态
                    if let Ok(mut spec) = self.spectrum_data.try_lock() {
                        *spec = self.prev_spectrum.clone();
                    }
                }

                // 更新波形数据用于调试或备用
                if let Ok(mut wave) = self.waveform_data.try_lock() {
                    *wave = self.buffer.clone();
                }

                self.buffer.clear();
            }
        }
        sample
    }
}

impl<I> Source for VisualizationSource<I>
where
    I: Source<Item = f32> + Send,
{
    fn current_frame_len(&self) -> Option<usize> {
        self.input.current_frame_len()
    }

    fn channels(&self) -> u16 {
        self.input.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }
}

/// 获取当前波形数据
#[command]
pub fn get_waveform_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    let data = state
        .player
        .waveform_data
        .lock()
        .map_err(|e| format!("Failed to acquire lock on waveform data: {}", e))?;
    Ok(data.clone())
}

/// 获取当前频谱数据
#[command]
pub fn get_spectrum_data(state: State<AppState>) -> Result<Vec<f32>, String> {
    let data = state
        .player
        .spectrum_data
        .lock()
        .map_err(|e| format!("Failed to acquire lock on spectrum data: {}", e))?;
    Ok(data.clone())
}

/// 播放指定的音轨
#[command]
pub fn play_track(
    state: State<AppState>,
    path: String,
    position: Option<f32>,
) -> Result<(), String> {
    let player_state = &state.player;

    // 停止当前音轨并清除sink
    {
        let sink = player_state.sink.lock().unwrap();
        sink.stop();
        // 播放新音轨前恢复音量
        let target_volume = *player_state.target_volume.lock().unwrap();
        sink.set_volume(target_volume);
    }

    // 更新当前音轨信息
    *player_state.current_path.lock().unwrap() = Some(path.clone());
    *player_state.current_source.lock().unwrap() = None;

    let waveform_data = Arc::clone(&player_state.waveform_data);
    let spectrum_data = Arc::clone(&player_state.spectrum_data);

    let source: Box<dyn Source<Item = f32> + Send> = match SymphoniaDecoder::new(&path) {
        Ok(mut symphonia_decoder) => {
            if let Some(time) = position {
                if let Err(e) = symphonia_decoder.seek(Duration::from_secs_f32(time)) {
                    eprintln!("Symphonia seek failed: {}", e);
                }
            }
            println!("使用Symphonia解码器: {}", path);
            Box::new(VisualizationSource::new(
                crate::audio_decoder::SymphoniaSource::new(symphonia_decoder),
                waveform_data,
                spectrum_data,
            ))
        }
        Err(e) => {
            println!("Symphonia解码失败，回退到rodio解码器: {}", e);
            let file = File::open(&path).map_err(|e| e.to_string())?;
            let reader = BufReader::new(file);
            let rodio_source = rodio::Decoder::new(reader).map_err(|e| e.to_string())?;
            Box::new(VisualizationSource::new(
                rodio_source.convert_samples::<f32>(),
                waveform_data,
                spectrum_data,
            ))
        }
    };

    let sink = player_state.sink.lock().unwrap();
    sink.append(source);
    sink.play();

    Ok(())
}

/// 暂停当前播放的音轨
#[command]
pub fn pause_track(state: State<AppState>) -> Result<(), String> {
    let player_state = &state.player;
    let sink = Arc::clone(&player_state.sink);
    sink.lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?
        .pause();
    Ok(())
}

/// 恢复播放暂停的音轨
#[command]
pub fn resume_track(state: State<AppState>) -> Result<(), String> {
    let sink = state
        .player
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    sink.play();
    Ok(())
}

/// 设置播放音量
#[command]
pub fn set_volume(state: State<AppState>, volume: f32) -> Result<(), String> {
    // 验证音量范围
    if !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between 0.0 and 1.0".to_string());
    }

    let player_state = &state.player;
    *player_state
        .target_volume
        .lock()
        .map_err(|e| format!("Failed to acquire lock on target volume: {}", e))? = volume;

    let sink = player_state
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    sink.set_volume(volume);
    Ok(())
}

/// 获取当前播放状态
#[command]
pub fn get_playback_status(state: State<AppState>) -> Result<PlaybackStatus, String> {
    let player_state = &state.player;
    let sink = player_state.sink.lock().unwrap();

    let is_playing = !sink.is_paused();
    let volume = *player_state.target_volume.lock().unwrap();

    // 计算播放位置（这里简化处理，实际实现可能需要更复杂的逻辑）
    let position_secs = 0.0; // 占位符，实际实现需要从解码器获取

    Ok(PlaybackStatus::new(is_playing, position_secs, volume))
}

/// 检查当前音轨是否播放完毕
#[command]
pub fn is_track_finished(state: State<AppState>) -> Result<bool, String> {
    let sink = state
        .player
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    // 检查sink是否为空且当前没有暂停状态
    Ok(sink.empty() && !sink.is_paused())
}

/// 跳转到音轨的指定位置
#[command]
pub fn seek_track(state: State<AppState>, time: f32) -> Result<(), String> {
    let player_state = &state.player;
    let duration = Duration::from_secs_f32(time);

    if let Some(path) = player_state.current_path.lock().unwrap().clone() {
        // 统一使用Symphonia进行seek
        match SymphoniaDecoder::new(&path) {
            Ok(mut decoder) => {
                println!("使用Symphonia seek for: {}", path);
                match decoder.seek(duration) {
                    Ok(_) => {
                        let source: Box<dyn Source<Item = f32> + Send> =
                            Box::new(VisualizationSource::new(
                                crate::audio_decoder::SymphoniaSource::new(decoder),
                                Arc::clone(&player_state.waveform_data),
                                Arc::clone(&player_state.spectrum_data),
                            ));

                        // 停止当前播放并替换为新源
                        {
                            let sink = player_state.sink.lock().unwrap();
                            sink.stop();
                            let target_volume = *player_state.target_volume.lock().unwrap();
                            sink.set_volume(target_volume);
                        }

                        let sink = player_state.sink.lock().unwrap();
                        sink.append(source);
                        sink.play();
                        Ok(())
                    }
                    Err(e) => Err(format!("Failed to seek track with Symphonia: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to create decoder for seeking: {}", e)),
        }
    } else {
        Err("No track currently loaded".to_string())
    }
}
