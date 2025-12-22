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
use tauri::{command, State, AppHandle, Emitter};

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

/// 频谱数据更新事件
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SpectrumUpdateEvent {
    pub data: Vec<f32>,
    pub timestamp: u64,
}

/// 发送频谱数据更新事件
fn emit_spectrum_update(app: &AppHandle, spectrum_data: &[f32]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let event = SpectrumUpdateEvent {
        data: spectrum_data.to_vec(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis() as u64,
    };
    
    app.emit("spectrum-update", event)?;
    Ok(())
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
    app_handle: Option<AppHandle>,
    last_emit_time: std::sync::atomic::AtomicU64,
}

impl<I> VisualizationSource<I>
where
    I: Source<Item = f32> + Send,
{
    pub fn new(
        input: I,
        waveform_data: Arc<Mutex<Vec<f32>>>,
        spectrum_data: Arc<Mutex<Vec<f32>>>,
        app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            input,
            waveform_data,
            spectrum_data,
            buffer: Vec::with_capacity(1024),
            prev_spectrum: vec![0.0; 128],
            app_handle,
            last_emit_time: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

// 别管这个纯娱乐的可视化功能，maybe it could be useful one day
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

            // 使用 try_lock 避免阻塞音频流，仅在锁可用时才更新可视化数据
            // 这样可以确保 FFT 计算不会阻塞音频播放
            if self.buffer.len() >= 1024 {
                // 快速路径：仅在锁可用时执行 FFT 计算
                // 如果锁被占用，跳过本次更新，下次再尝试
                if let Ok(mut spec) = self.spectrum_data.try_lock() {
                    // 计算FFT（仅当锁可用时）
                    let hann_window = hann_window(&self.buffer);
                    // 使用实际的采样率（如果可用）
                    let sample_rate = self.input.sample_rate();
                    let spectrum_result = samples_fft_to_spectrum(
                        &hann_window,
                        sample_rate,
                        FrequencyLimit::Range(20.0, 20000.0),
                        Some(&divide_by_N_sqrt),
                    );

                    if let Ok(spectrum) = spectrum_result {
                        // 对数频段映射 (128 bands)
                        let mut new_spectrum = vec![0.0; 128];
                        let data = spectrum.data();

                        // 频率范围 180Hz - 20000Hz
                        let min_freq = 180.0f32;
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

                        // 更新共享状态（锁已经持有）
                        *spec = self.prev_spectrum.clone();
                    }
                }

                // 更新波形数据（也使用 try_lock）
                if let Ok(mut wave) = self.waveform_data.try_lock() {
                    *wave = self.buffer.clone();
                }

                // 发送频谱更新事件（限制推送频率为60fps，约16ms间隔）
                // 使用 try_lock 的结果来决定是否发送，避免阻塞
                if let Some(ref app) = self.app_handle {
                    let current_time = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    
                    let last_time = self.last_emit_time.load(std::sync::atomic::Ordering::Relaxed);
                    
                    if current_time - last_time >= 16 { // 限制推送频率约60fps
                        // 仅在锁可用时发送事件，避免阻塞
                        if self.spectrum_data.try_lock().is_ok() {
                            if let Err(e) = emit_spectrum_update(app, &self.prev_spectrum) {
                                eprintln!("Failed to emit spectrum update: {}", e);
                            } else {
                                self.last_emit_time.store(current_time, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                    }
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
    app: AppHandle,
    state: State<AppState>,
    path: String,
    position: Option<f32>,
) -> Result<(), String> {
    let player_state = &state.player;
    
    // 检查是否使用独占模式
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        play_track_exclusive(&app, &state, &path, position)
    } else {
        play_track_shared(&app, &state, &path, position)
    }
}

/// 使用共享模式播放音轨
fn play_track_shared(
    app: &AppHandle,
    state: &State<AppState>,
    path: &str,
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
    *player_state.current_path.lock().unwrap() = Some(path.to_string());
    *player_state.current_source.lock().unwrap() = None;

    let waveform_data = Arc::clone(&player_state.waveform_data);
    let spectrum_data = Arc::clone(&player_state.spectrum_data);

    let source: Box<dyn Source<Item = f32> + Send> = match SymphoniaDecoder::new(path) {
        Ok(mut symphonia_decoder) => {
            if let Some(time) = position {
                if let Err(e) = symphonia_decoder.seek(Duration::from_secs_f32(time)) {
                    eprintln!("Symphonia seek failed: {}", e);
                }
            }
            
            // 预填充缓冲区，确保在播放开始前有足够的数据
            // 这可以减少播放开始时的中断概率
            if let Err(e) = symphonia_decoder.prefill_buffer() {
                eprintln!("警告: 缓冲区预填充失败，可能会影响播放流畅度: {}", e);
                // 不阻止播放，继续执行
            }
            
            println!("使用Symphonia解码器（无锁版本）: {}", path);
            // 使用无锁版本的解码器，完全消除锁竞争
            Box::new(VisualizationSource::new(
                crate::audio_decoder::LockFreeSymphoniaSource::new(symphonia_decoder),
                waveform_data,
                spectrum_data,
                Some(app.clone()),
            ))
        }
        Err(e) => {
            println!("Symphonia解码失败，回退到rodio解码器: {}", e);
            let file = File::open(path).map_err(|e| e.to_string())?;
            let reader = BufReader::new(file);
            let rodio_source = rodio::Decoder::new(reader).map_err(|e| e.to_string())?;
            Box::new(VisualizationSource::new(
                rodio_source.convert_samples::<f32>(),
                waveform_data,
                spectrum_data,
                Some(app.clone()),
            ))
        }
    };

    let sink = player_state.sink.lock().unwrap();
    sink.append(source);
    sink.play();

    Ok(())
}

/// 使用 WASAPI 独占模式播放音轨
fn play_track_exclusive(
    app: &AppHandle,
    state: &State<AppState>,
    path: &str,
    position: Option<f32>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    
    let player_state = &state.player;
    
    // 1. 先停止之前的解码线程
    player_state.decode_thread_stop.store(true, Ordering::SeqCst);
    
    // 生成新的线程 ID
    let new_thread_id = player_state.decode_thread_id.fetch_add(1, Ordering::SeqCst) + 1;
    
    // 2. 停止 WASAPI 播放并清空缓冲区
    {
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            let _ = player.stop();
            let _ = player.clear_buffer();
        }
    }
    
    // 等待一小段时间让之前的线程有机会退出
    std::thread::sleep(Duration::from_millis(100));
    
    // 3. 重置停止标志 - 必须在启动新线程之前
    player_state.decode_thread_stop.store(false, Ordering::SeqCst);
    
    // 添加内存屏障确保标志已经被重置
    std::sync::atomic::fence(Ordering::SeqCst);
    
    // 4. 获取 WASAPI 播放器信息
    let (target_sample_rate, target_channels) = {
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        let wasapi_player = wasapi_guard.as_ref()
            .ok_or_else(|| "WASAPI exclusive player not initialized".to_string())?;
        (wasapi_player.get_sample_rate(), wasapi_player.get_channels())
    };
    
    // 更新当前音轨信息（仅在非 seek 操作时更新）
    // 对于 seek 操作，current_path 已经是正确的值，不需要重复设置
    if position.is_none() {
        *player_state.current_path.lock().unwrap() = Some(path.to_string());
    }
    
    println!(
        "WASAPI Exclusive playback: {} @ {}Hz, {} channels",
        path, target_sample_rate, target_channels
    );
    
    // 5. 创建解码器
    let mut decoder = SymphoniaDecoder::new(path)
        .map_err(|e| format!("Failed to create decoder: {}", e))?;
    
    // 如果有指定位置，先 seek
    if let Some(time) = position {
        if let Err(e) = decoder.seek(Duration::from_secs_f32(time)) {
            eprintln!("Seek failed: {}", e);
        }
    }
    
    // 预填充缓冲区
    if let Err(e) = decoder.prefill_buffer() {
        eprintln!("警告: 缓冲区预填充失败: {}", e);
    }
    
    // 获取解码器的采样率和声道数
    let source_sample_rate = decoder.sample_rate();
    let source_channels = decoder.channels();
    
    println!(
        "Source format: {}Hz, {} channels -> Target: {}Hz, {} channels",
        source_sample_rate, source_channels, target_sample_rate, target_channels
    );
    
    // 创建无锁源
    let source = crate::audio_decoder::LockFreeSymphoniaSource::new(decoder);
    
    // 6. 启动解码线程
    let wasapi_player_clone = Arc::clone(&player_state.wasapi_player);
    let waveform_data = Arc::clone(&player_state.waveform_data);
    let spectrum_data = Arc::clone(&player_state.spectrum_data);
    let app_clone = app.clone();
    let stop_flag = Arc::clone(&player_state.decode_thread_stop);
    let thread_id = Arc::clone(&player_state.decode_thread_id);
    
    // 用于通知主线程解码线程已经开始
    let thread_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_started_clone = Arc::clone(&thread_started);
    
    std::thread::spawn(move || {
        // 通知主线程解码线程已经开始
        thread_started_clone.store(true, Ordering::SeqCst);
        
        // 使用 catch_unwind 捕获 panic
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_and_push_to_wasapi(
                source,
                wasapi_player_clone,
                waveform_data,
                spectrum_data,
                app_clone,
                stop_flag,
                thread_id,
                new_thread_id,
                source_sample_rate,
                source_channels,
                target_sample_rate,
                target_channels,
            );
        }));
        
        if let Err(e) = result {
            eprintln!("Decode thread panicked: {:?}", e);
        }
    });
    
    // 等待解码线程启动
    let mut wait_count = 0;
    while !thread_started.load(Ordering::SeqCst) && wait_count < 50 {
        std::thread::sleep(Duration::from_millis(10));
        wait_count += 1;
    }
    
    // 7. 启动 WASAPI 播放
    {
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            // 等待一小段时间让解码线程开始填充缓冲区
            std::thread::sleep(Duration::from_millis(150));
            
            if let Err(e) = player.start() {
                eprintln!("Failed to start WASAPI playback: {:?}", e);
                return Err(format!("Failed to start WASAPI playback: {:?}", e));
            }
        }
    }
    
    Ok(())
}

/// 解码音频并推送到 WASAPI 播放器
fn decode_and_push_to_wasapi(
    mut source: crate::audio_decoder::LockFreeSymphoniaSource,
    wasapi_player: Arc<Mutex<Option<crate::wasapi_exclusive::WasapiExclusivePlayback>>>,
    _waveform_data: Arc<Mutex<Vec<f32>>>,
    _spectrum_data: Arc<Mutex<Vec<f32>>>,
    _app: AppHandle,
    stop_flag: Arc<std::sync::atomic::AtomicBool>,
    thread_id_ref: Arc<std::sync::atomic::AtomicU64>,
    my_thread_id: u64,
    source_sample_rate: u32,
    source_channels: u16,
    target_sample_rate: u32,
    target_channels: u16,
) {
    use std::sync::atomic::Ordering;
    use rubato::{Resampler, SincFixedIn, SincInterpolationType, SincInterpolationParameters, WindowFunction};
    
    // 如果停止标志已经被设置，或者线程 ID 不匹配，立即退出
    if stop_flag.load(Ordering::SeqCst) {
        return;
    }
    
    if thread_id_ref.load(Ordering::SeqCst) != my_thread_id {
        return;
    }
    
    // 检查是否需要重采样
    let need_resample = source_sample_rate != target_sample_rate;
    let resample_ratio = target_sample_rate as f64 / source_sample_rate as f64;
    
    // 创建重采样器（如果需要）
    let chunk_size = 1024;
    let mut resampler: Option<SincFixedIn<f32>> = if need_resample {
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.925,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        
        match SincFixedIn::<f32>::new(
            resample_ratio,
            2.0,
            params,
            chunk_size,
            source_channels as usize,
        ) {
            Ok(r) => Some(r),
            Err(e) => {
                eprintln!("Failed to create resampler: {:?}", e);
                None
            }
        }
    } else {
        None
    };
    
    // 用于重采样的缓冲区
    let mut input_frames: Vec<Vec<f32>> = vec![Vec::with_capacity(chunk_size * 2); source_channels as usize];
    let mut output_buffer: Vec<f32> = Vec::with_capacity(chunk_size * target_channels as usize * 4);
    
    loop {
        // 检查是否应该停止
        if stop_flag.load(Ordering::SeqCst) {
            break;
        }
        
        // 检查是否是当前活动的线程
        if thread_id_ref.load(Ordering::SeqCst) != my_thread_id {
            break;
        }
        
        // 检查 WASAPI 播放器是否仍然存在
        let player_exists = {
            let player_guard = wasapi_player.lock().unwrap();
            player_guard.is_some()
        };
        
        if !player_exists {
            break;
        }
        
        // 清空输入缓冲区
        for ch in &mut input_frames {
            ch.clear();
        }
        
        // 解码一批样本（交错格式）
        let samples_needed = chunk_size * source_channels as usize;
        let mut interleaved_buffer = Vec::with_capacity(samples_needed);
        let mut eof = false;
        
        for _ in 0..samples_needed {
            if let Some(sample) = source.next() {
                interleaved_buffer.push(sample);
            } else {
                eof = true;
                break;
            }
        }
        
        if interleaved_buffer.is_empty() {
            break;
        }
        
        // 将交错格式转换为分离的声道格式（用于重采样）
        for (i, sample) in interleaved_buffer.iter().enumerate() {
            let ch = i % source_channels as usize;
            input_frames[ch].push(*sample);
        }
        
        // 独占模式下不计算频谱，以保证音频播放的流畅性
        
        // 重采样（如果需要）
        let output_frames: Vec<Vec<f32>> = if let Some(ref mut resampler) = resampler {
            let actual_frames = input_frames[0].len();
            
            // 如果帧数不足，填充到 chunk_size
            if actual_frames < chunk_size {
                for ch in &mut input_frames {
                    ch.resize(chunk_size, 0.0);
                }
            }
            
            match resampler.process(&input_frames, None) {
                Ok(output) => output,
                Err(e) => {
                    eprintln!("Resampling error: {:?}", e);
                    // 如果重采样失败，直接使用原始数据
                    input_frames.clone()
                }
            }
        } else {
            input_frames.clone()
        };
        
        // 将分离的声道格式转换回交错格式
        output_buffer.clear();
        let output_len = output_frames.first().map(|v| v.len()).unwrap_or(0);
        let output_ch_count = output_frames.len();
        
        for i in 0..output_len {
            for ch in 0..output_ch_count {
                output_buffer.push(output_frames[ch].get(i).copied().unwrap_or(0.0));
            }
        }
        
        // 声道转换（如果需要）
        let final_output: Vec<f32> = if source_channels != target_channels {
            convert_channels(&output_buffer, source_channels, target_channels)
        } else {
            output_buffer.clone()
        };
        
        // 推送样本到 WASAPI
        if !final_output.is_empty() {
            let player_guard = wasapi_player.lock().unwrap();
            if let Some(ref player) = *player_guard {
                if let Err(e) = player.push_samples(final_output) {
                    eprintln!("Failed to push samples: {}", e);
                    break;
                }
            }
        }
        
        // 如果到达文件末尾，退出循环
        if eof && interleaved_buffer.len() < samples_needed {
            // 等待 WASAPI 缓冲区播放完毕
            loop {
                // 检查是否被停止
                if stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                if thread_id_ref.load(Ordering::SeqCst) != my_thread_id {
                    break;
                }
                
                // 检查缓冲区是否为空
                let buffer_size = {
                    let player_guard = wasapi_player.lock().unwrap();
                    if let Some(ref player) = *player_guard {
                        player.get_buffer_size()
                    } else {
                        0
                    }
                };
                
                if buffer_size == 0 {
                    break;
                }
                
                std::thread::sleep(Duration::from_millis(50));
            }
            
            // 只有在没有被中断的情况下才停止播放器
            if !stop_flag.load(Ordering::SeqCst) && thread_id_ref.load(Ordering::SeqCst) == my_thread_id {
                // 停止 WASAPI 播放器，这样 is_track_finished 会返回 true
                let player_guard = wasapi_player.lock().unwrap();
                if let Some(ref player) = *player_guard {
                    let _ = player.stop();
                }
            }
            
            break;
        }
        
        // 动态调整延迟：检查缓冲区状态
        std::thread::sleep(Duration::from_micros(500));
    }
}

/// 声道转换
fn convert_channels(samples: &[f32], source_channels: u16, target_channels: u16) -> Vec<f32> {
    if source_channels == target_channels {
        return samples.to_vec();
    }
    
    let source_ch = source_channels as usize;
    let target_ch = target_channels as usize;
    let frame_count = samples.len() / source_ch;
    let mut output = Vec::with_capacity(frame_count * target_ch);
    
    for frame in 0..frame_count {
        let frame_start = frame * source_ch;
        
        if source_ch == 1 && target_ch == 2 {
            // 单声道转立体声
            let sample = samples[frame_start];
            output.push(sample);
            output.push(sample);
        } else if source_ch == 2 && target_ch == 1 {
            // 立体声转单声道
            let left = samples[frame_start];
            let right = samples[frame_start + 1];
            output.push((left + right) / 2.0);
        } else {
            // 其他情况：简单复制或截断
            for ch in 0..target_ch {
                if ch < source_ch {
                    output.push(samples[frame_start + ch]);
                } else {
                    output.push(samples[frame_start]);
                }
            }
        }
    }
    
    output
}

/// 暂停当前播放的音轨
#[command]
pub fn pause_track(state: State<AppState>) -> Result<(), String> {
    let player_state = &state.player;
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        // WASAPI 独占模式
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            player.pause()?;
        }
    } else {
        // 共享模式
        let sink = Arc::clone(&player_state.sink);
        sink.lock()
            .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?
            .pause();
    }
    Ok(())
}

/// 恢复播放暂停的音轨
#[command]
pub fn resume_track(state: State<AppState>) -> Result<(), String> {
    let player_state = &state.player;
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        // WASAPI 独占模式
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            player.resume()?;
        }
    } else {
        // 共享模式
        let sink = state
            .player
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
        sink.play();
    }
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

    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        // WASAPI 独占模式
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            player.set_volume(volume)?;
        }
    } else {
        // 共享模式
        let sink = player_state
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
        sink.set_volume(volume);
    }
    Ok(())
}

/// 获取当前播放状态
#[command]
pub fn get_playback_status(state: State<AppState>) -> Result<PlaybackStatus, String> {
    let player_state = &state.player;
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    let volume = *player_state.target_volume.lock().unwrap();
    
    let is_playing = if exclusive_mode {
        // WASAPI 独占模式
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            player.get_state() == crate::wasapi_exclusive::PlaybackState::Playing
        } else {
            false
        }
    } else {
        // 共享模式
        let sink = player_state.sink.lock().unwrap();
        !sink.is_paused()
    };

    // 计算播放位置（这里简化处理，实际实现可能需要更复杂的逻辑）
    let position_secs = 0.0; // 占位符，实际实现需要从解码器获取

    Ok(PlaybackStatus::new(is_playing, position_secs, volume))
}

/// 检查当前音轨是否播放完毕
#[command]
pub fn is_track_finished(state: State<AppState>) -> Result<bool, String> {
    let player_state = &state.player;
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if exclusive_mode {
        // WASAPI 独占模式
        let wasapi_guard = player_state.wasapi_player.lock().unwrap();
        if let Some(ref player) = *wasapi_guard {
            Ok(player.get_state() == crate::wasapi_exclusive::PlaybackState::Stopped)
        } else {
            Ok(true)
        }
    } else {
        // 共享模式
        let sink = state
            .player
            .sink
            .lock()
            .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
        // 检查sink是否为空且当前没有暂停状态
        Ok(sink.empty() && !sink.is_paused())
    }
}

/// 跳转到音轨的指定位置
#[command]
pub fn seek_track(app: AppHandle, state: State<AppState>, time: f32) -> Result<(), String> {
    let player_state = &state.player;
    let exclusive_mode = *player_state.exclusive_mode.lock().unwrap();
    
    if let Some(path) = player_state.current_path.lock().unwrap().clone() {
        if exclusive_mode {
            // WASAPI 独占模式：重新播放并跳转到指定位置
            play_track_exclusive(&app, &state, &path, Some(time))
        } else {
            // 共享模式：使用 Symphonia 进行 seek
            seek_track_shared(&app, &state, &path, time)
        }
    } else {
        Err("No track currently loaded".to_string())
    }
}

/// 共享模式下的 seek 实现
fn seek_track_shared(
    app: &AppHandle,
    state: &State<AppState>,
    path: &str,
    time: f32,
) -> Result<(), String> {
    let player_state = &state.player;
    let duration = Duration::from_secs_f32(time);

    // 统一使用Symphonia进行seek
    match SymphoniaDecoder::new(path) {
        Ok(mut decoder) => {
            println!("使用Symphonia seek for: {}", path);
            match decoder.seek(duration) {
                Ok(_) => {
                    // 预填充缓冲区，确保跳转后的播放流畅
                    if let Err(e) = decoder.prefill_buffer() {
                        eprintln!("警告: 跳转后缓冲区预填充失败: {}", e);
                        // 不阻止播放，继续执行
                    }
                    
                    let source: Box<dyn Source<Item = f32> + Send> =
                        Box::new(VisualizationSource::new(
                            crate::audio_decoder::LockFreeSymphoniaSource::new(decoder),
                            Arc::clone(&player_state.waveform_data),
                            Arc::clone(&player_state.spectrum_data),
                            Some(app.clone()),
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
}
