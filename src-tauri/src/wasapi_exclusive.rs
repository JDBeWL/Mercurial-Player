//! WASAPI 独占模式音频播放实现
//!
//! 这个模块实现了真正的 WASAPI 独占模式音频输出。

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Condvar};
use std::thread::{self, JoinHandle};
use std::collections::VecDeque;
use crossbeam_channel::{bounded, Sender, Receiver};

/// 音频线程命令
#[derive(Debug)]
pub enum AudioCommand {
    Initialize { device_name: Option<String> },
    Start,
    Stop,
    Pause,
    Resume,
    SetVolume(f32),
    ClearBuffer,
    Shutdown,
}

/// 音频线程响应
#[derive(Debug)]
pub enum AudioResponse {
    Initialized { sample_rate: u32, channels: u16, device_name: String },
    InitFailed(String),
    Ok,
    Error(String),
}

/// WASAPI 独占模式播放器状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackState {
    Uninitialized,
    Stopped,
    Playing,
    Paused,
}

/// WASAPI 独占模式播放器
pub struct WasapiExclusivePlayback {
    command_tx: Sender<AudioCommand>,
    response_rx: Receiver<AudioResponse>,
    audio_thread: Option<JoinHandle<()>>,
    state: Arc<Mutex<PlaybackState>>,
    sample_rate: AtomicU32,
    channels: AtomicU32,
    volume: Arc<Mutex<f32>>,
    is_running: Arc<AtomicBool>,
    sample_buffer: Arc<(Mutex<VecDeque<f32>>, Condvar)>,
}

impl WasapiExclusivePlayback {
    /// 创建新的 WASAPI 独占模式播放器
    pub fn new() -> Self {
        let (command_tx, command_rx) = bounded::<AudioCommand>(64);
        let (response_tx, response_rx) = bounded::<AudioResponse>(64);
        
        let state = Arc::new(Mutex::new(PlaybackState::Uninitialized));
        let volume = Arc::new(Mutex::new(1.0f32));
        let is_running = Arc::new(AtomicBool::new(true));
        let sample_buffer = Arc::new((Mutex::new(VecDeque::with_capacity(48000 * 2 * 4)), Condvar::new()));
        
        let state_clone = Arc::clone(&state);
        let volume_clone = Arc::clone(&volume);
        let is_running_clone = Arc::clone(&is_running);
        let sample_buffer_clone = Arc::clone(&sample_buffer);
        
        let audio_thread = thread::spawn(move || {
            audio_thread_main(
                command_rx,
                response_tx,
                state_clone,
                volume_clone,
                is_running_clone,
                sample_buffer_clone,
            );
        });
        
        Self {
            command_tx,
            response_rx,
            audio_thread: Some(audio_thread),
            state,
            sample_rate: AtomicU32::new(48000),
            channels: AtomicU32::new(2),
            volume,
            is_running,
            sample_buffer,
        }
    }
    
    /// 初始化设备
    pub fn initialize(&self, device_name: Option<&str>) -> Result<(u32, u16, String), String> {
        self.command_tx
            .send(AudioCommand::Initialize { 
                device_name: device_name.map(|s| s.to_string()) 
            })
            .map_err(|e| format!("Failed to send initialize command: {}", e))?;
        
        match self.response_rx.recv() {
            Ok(AudioResponse::Initialized { sample_rate, channels, device_name }) => {
                self.sample_rate.store(sample_rate, Ordering::SeqCst);
                self.channels.store(channels as u32, Ordering::SeqCst);
                *self.state.lock().unwrap() = PlaybackState::Stopped;
                Ok((sample_rate, channels, device_name))
            }
            Ok(AudioResponse::InitFailed(e)) => Err(e),
            Ok(other) => Err(format!("Unexpected response: {:?}", other)),
            Err(e) => Err(format!("Failed to receive response: {}", e)),
        }
    }
    
    /// 开始播放
    pub fn start(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Start)
            .map_err(|e| format!("Failed to send start command: {}", e))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }
    
    /// 停止播放
    pub fn stop(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Stop)
            .map_err(|e| format!("Failed to send stop command: {}", e))?;
        *self.state.lock().unwrap() = PlaybackState::Stopped;
        Ok(())
    }
    
    /// 暂停播放
    pub fn pause(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Pause)
            .map_err(|e| format!("Failed to send pause command: {}", e))?;
        *self.state.lock().unwrap() = PlaybackState::Paused;
        Ok(())
    }
    
    /// 恢复播放
    pub fn resume(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Resume)
            .map_err(|e| format!("Failed to send resume command: {}", e))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }
    
    /// 设置音量
    pub fn set_volume(&self, vol: f32) -> Result<(), String> {
        let vol = vol.clamp(0.0, 1.0);
        *self.volume.lock().unwrap() = vol;
        self.command_tx
            .send(AudioCommand::SetVolume(vol))
            .map_err(|e| format!("Failed to send volume command: {}", e))
    }
    
    /// 添加音频样本
    pub fn push_samples(&self, samples: Vec<f32>) -> Result<(), String> {
        let (buffer, cvar) = &*self.sample_buffer;
        let mut buf = buffer.lock().unwrap();
        buf.extend(samples);
        cvar.notify_one();
        Ok(())
    }
    
    /// 清空缓冲区
    pub fn clear_buffer(&self) -> Result<(), String> {
        let (buffer, _) = &*self.sample_buffer;
        let mut buf = buffer.lock().unwrap();
        buf.clear();
        Ok(())
    }
    
    /// 获取当前状态
    pub fn get_state(&self) -> PlaybackState {
        *self.state.lock().unwrap()
    }
    
    /// 获取采样率
    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::SeqCst)
    }
    
    /// 获取声道数
    pub fn get_channels(&self) -> u16 {
        self.channels.load(Ordering::SeqCst) as u16
    }
    
    /// 获取音量
    pub fn get_volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }
    
    /// 获取缓冲区大小
    pub fn get_buffer_size(&self) -> usize {
        let (buffer, _) = &*self.sample_buffer;
        buffer.lock().unwrap().len()
    }
}

impl Drop for WasapiExclusivePlayback {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        let _ = self.command_tx.send(AudioCommand::Shutdown);
        
        let (_, cvar) = &*self.sample_buffer;
        cvar.notify_all();
        
        if let Some(thread) = self.audio_thread.take() {
            let _ = thread.join();
        }
    }
}

/// 音频线程主函数
fn audio_thread_main(
    command_rx: Receiver<AudioCommand>,
    response_tx: Sender<AudioResponse>,
    state: Arc<Mutex<PlaybackState>>,
    _volume: Arc<Mutex<f32>>,
    is_running: Arc<AtomicBool>,
    sample_buffer: Arc<(Mutex<VecDeque<f32>>, Condvar)>,
) {
    // 初始化 COM
    let _ = wasapi::initialize_mta();
    
    let mut audio_client: Option<wasapi::AudioClient> = None;
    let mut render_client: Option<wasapi::AudioRenderClient> = None;
    let mut event_handle: Option<wasapi::Handle> = None;
    let mut current_channels: u16 = 2;
    let mut current_bits: u16 = 32;
    let mut current_sample_type_is_float: bool = true;
    let mut is_playing = false;
    let mut current_volume = 1.0f32;
    
    println!("WASAPI audio thread started");
    
    while is_running.load(Ordering::SeqCst) {
        // 处理命令（非阻塞）
        match command_rx.try_recv() {
            Ok(AudioCommand::Initialize { device_name }) => {
                match initialize_exclusive_device(device_name.as_deref()) {
                    Ok((client, format_info)) => {
                        let (sr, ch, name, bits, is_float) = format_info;
                        current_channels = ch;
                        current_bits = bits;
                        current_sample_type_is_float = is_float;
                        
                        println!(
                            "Audio format: {}Hz, {} channels, {} bits, float: {}",
                            sr, ch, bits, is_float
                        );
                        
                        match client.get_audiorenderclient() {
                            Ok(rc) => {
                                match client.set_get_eventhandle() {
                                    Ok(eh) => {
                                        render_client = Some(rc);
                                        event_handle = Some(eh);
                                        audio_client = Some(client);
                                        let _ = response_tx.send(AudioResponse::Initialized {
                                            sample_rate: sr,
                                            channels: ch,
                                            device_name: name,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = response_tx.send(AudioResponse::InitFailed(
                                            format!("Failed to get event handle: {:?}", e)
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = response_tx.send(AudioResponse::InitFailed(
                                    format!("Failed to get render client: {:?}", e)
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = response_tx.send(AudioResponse::InitFailed(e));
                    }
                }
            }
            Ok(AudioCommand::Start) => {
                if let Some(ref client) = audio_client {
                    if let Err(e) = client.start_stream() {
                        eprintln!("Failed to start audio client: {:?}", e);
                    } else {
                        is_playing = true;
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::Stop) => {
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    *state.lock().unwrap() = PlaybackState::Stopped;
                    
                    let (buffer, _) = &*sample_buffer;
                    buffer.lock().unwrap().clear();
                }
            }
            Ok(AudioCommand::Pause) => {
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    *state.lock().unwrap() = PlaybackState::Paused;
                }
            }
            Ok(AudioCommand::Resume) => {
                if let Some(ref client) = audio_client {
                    if let Err(e) = client.start_stream() {
                        eprintln!("Failed to resume audio client: {:?}", e);
                    } else {
                        is_playing = true;
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::SetVolume(vol)) => {
                current_volume = vol;
            }
            Ok(AudioCommand::ClearBuffer) => {
                let (buffer, _) = &*sample_buffer;
                buffer.lock().unwrap().clear();
            }
            Ok(AudioCommand::Shutdown) => {
                break;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                break;
            }
        }
        
        // 处理音频输出
        if is_playing {
            if let (Some(ref client), Some(ref rc), Some(ref eh)) = 
                (&audio_client, &render_client, &event_handle) 
            {
                // 等待缓冲区事件
                match eh.wait_for_event(10) {
                    Ok(_) => {
                        match client.get_available_space_in_frames() {
                            Ok(frames_available) if frames_available > 0 => {
                                let samples_needed = frames_available as usize * current_channels as usize;
                                
                                let (buffer, _) = &*sample_buffer;
                                let mut buf = buffer.lock().unwrap();
                                
                                let mut output_samples = Vec::with_capacity(samples_needed);
                                
                                for _ in 0..samples_needed {
                                    if let Some(sample) = buf.pop_front() {
                                        output_samples.push(sample * current_volume);
                                    } else {
                                        output_samples.push(0.0);
                                    }
                                }
                                
                                drop(buf);
                                
                                // 根据位深度和采样类型转换数据
                                let output_bytes = convert_samples_to_bytes(
                                    &output_samples,
                                    current_bits,
                                    current_sample_type_is_float,
                                );
                                
                                if let Err(e) = rc.write_to_device(
                                    frames_available as usize,
                                    &output_bytes,
                                    None,
                                ) {
                                    eprintln!("Failed to write to device: {:?}", e);
                                    // 如果写入失败，停止播放
                                    is_playing = false;
                                    *state.lock().unwrap() = PlaybackState::Stopped;
                                }
                            }
                            Ok(_) => {} // frames_available == 0, do nothing
                            Err(e) => {
                                eprintln!("Failed to get available space: {:?}", e);
                            }
                        }
                    }
                    Err(_) => {
                        // 等待超时，继续循环
                    }
                }
            }
        } else {
            thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    
    // 清理
    if let Some(ref client) = audio_client {
        let _ = client.stop_stream();
    }
    
    println!("WASAPI audio thread stopped");
}

/// 初始化独占模式设备
fn initialize_exclusive_device(device_name: Option<&str>) -> Result<(wasapi::AudioClient, (u32, u16, String, u16, bool)), String> {
    use wasapi::{
        DeviceEnumerator, Direction, 
        SampleType, ShareMode, StreamMode, WaveFormat,
    };
    
    let enumerator = DeviceEnumerator::new()
        .map_err(|e| format!("Failed to create device enumerator: {:?}", e))?;
    
    let device = if let Some(name) = device_name {
        let collection = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|e| format!("Failed to get device collection: {:?}", e))?;
        
        let mut found = None;
        for device in collection.into_iter().flatten() {
            if let Ok(friendly_name) = device.get_friendlyname() {
                if friendly_name == name {
                    found = Some(device);
                    break;
                }
            }
        }
        found.ok_or_else(|| format!("Device not found: {}", name))?
    } else {
        enumerator
            .get_default_device(&Direction::Render)
            .map_err(|e| format!("Failed to get default device: {:?}", e))?
    };
    
    let device_name = device.get_friendlyname().unwrap_or_else(|_| "Unknown".to_string());
    
    let mut audio_client = device
        .get_iaudioclient()
        .map_err(|e| format!("Failed to get audio client: {:?}", e))?;
    
    // 首先获取设备的默认格式（Windows 中用户设置的采样率）
    let default_format = audio_client.get_mixformat()
        .map_err(|e| format!("Failed to get mix format: {:?}", e))?;
    
    let default_sample_rate = default_format.get_samplespersec() as usize;
    let default_channels = default_format.get_nchannels() as usize;
    
    println!(
        "Device default format: {}Hz, {} channels",
        default_sample_rate, default_channels
    );
    
    // 尝试使用用户设置的采样率，优先尝试不同的位深度
    // 如果用户设置的采样率不支持独占模式，则尝试其他常见采样率
    let sample_rates_to_try: [usize; 12] = [
        default_sample_rate,  // 首先尝试用户设置的采样率
        384000,
        352800,
        192000,
        176400,
        96000,
        88200,
        48000,
        44100,
        32000,
        22050,
        16000,
    ];
    
    let bit_depths: [(usize, bool); 4] = [
        (32, true),   // 32-bit float
        (32, false),  // 32-bit int
        (24, false),  // 24-bit int
        (16, false),  // 16-bit int
    ];
    
    let channels_to_try: [usize; 2] = [
        default_channels,  // 首先尝试用户设置的声道数
        2,                 // 然后尝试立体声
    ];
    
    let mut found_format = None;
    
    // 优先使用用户设置的采样率
    'outer: for &sample_rate in &sample_rates_to_try {
        for &channels in &channels_to_try {
            for &(bits, is_float) in &bit_depths {
                let sample_type = if is_float {
                    SampleType::Float
                } else {
                    SampleType::Int
                };
                
                let wave_format = WaveFormat::new(bits, bits, &sample_type, sample_rate, channels, None);
                
                if audio_client.is_supported(&wave_format, &ShareMode::Exclusive).is_ok() {
                    found_format = Some((wave_format, sample_rate as u32, channels as u16, bits as u16, is_float));
                    break 'outer;
                }
            }
        }
    }
    
    let (wave_format, sample_rate, channels, bits, is_float) = found_format
        .ok_or_else(|| "No supported exclusive format found".to_string())?;
    
    // 获取设备周期
    let (_default_period, min_period) = audio_client
        .get_device_period()
        .map_err(|e| format!("Failed to get device period: {:?}", e))?;
    
    // 初始化独占模式
    let stream_mode = StreamMode::EventsExclusive {
        period_hns: min_period,
    };
    
    audio_client
        .initialize_client(&wave_format, &Direction::Render, &stream_mode)
        .map_err(|e| format!("Failed to initialize exclusive mode: {:?}", e))?;
    
    println!(
        "WASAPI Exclusive Mode initialized: {} @ {}Hz, {} channels, {} bits, float: {}",
        device_name, sample_rate, channels, bits, is_float
    );
    
    Ok((audio_client, (sample_rate, channels, device_name, bits, is_float)))
}

/// 将 f32 样本转换为设备期望的字节格式
fn convert_samples_to_bytes(samples: &[f32], bits: u16, is_float: bool) -> Vec<u8> {
    match (bits, is_float) {
        (32, true) => {
            // 32-bit float
            samples.iter().flat_map(|&s| s.to_le_bytes()).collect()
        }
        (32, false) => {
            // 32-bit int
            samples
                .iter()
                .flat_map(|&s| {
                    let clamped = s.clamp(-1.0, 1.0);
                    let int_val = (clamped * i32::MAX as f32) as i32;
                    int_val.to_le_bytes()
                })
                .collect()
        }
        (24, _) => {
            // 24-bit int (stored in 3 bytes)
            samples
                .iter()
                .flat_map(|&s| {
                    let clamped = s.clamp(-1.0, 1.0);
                    // 24-bit range: -8388608 to 8388607
                    let int_val = (clamped * 8388607.0) as i32;
                    // 取低 24 位，以小端序存储
                    let bytes = int_val.to_le_bytes();
                    [bytes[0], bytes[1], bytes[2]]
                })
                .collect()
        }
        (16, _) => {
            // 16-bit int
            samples
                .iter()
                .flat_map(|&s| {
                    let clamped = s.clamp(-1.0, 1.0);
                    let int_val = (clamped * i16::MAX as f32) as i16;
                    int_val.to_le_bytes()
                })
                .collect()
        }
        _ => {
            // 默认使用 32-bit float
            samples.iter().flat_map(|&s| s.to_le_bytes()).collect()
        }
    }
}
