use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// 音频解码器状态
#[derive(Debug, PartialEq)]
enum DecoderState {
    Uninitialized,
    Ready,
    EndOfStream,
    Error(String),
}

/// 计算基于时间的缓冲区大小
#[must_use]
const fn calculate_buffer_size(sample_rate: u32, channels: u16, duration_ms: u32) -> usize {
    let samples_per_channel = (sample_rate as u64 * duration_ms as u64) / 1000;
    (samples_per_channel * channels as u64) as usize
}

/// 音频缓冲区管理器
struct AudioBuffer {
    samples: Vec<f32>,
    position: usize,
    capacity: usize,
    sample_rate: u32,
    channels: u16,
    refill_threshold_ms: u32,
}

impl AudioBuffer {
    fn new(capacity: usize, sample_rate: u32, channels: u16) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            position: 0,
            capacity,
            sample_rate,
            channels,
            refill_threshold_ms: 50, // 默认50ms的缓冲时间阈值
        }
    }
    
    #[inline]
    fn is_empty(&self) -> bool {
        self.position >= self.samples.len()
    }
    
    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.is_empty() {
            return None;
        }
        
        let sample = self.samples[self.position];
        self.position += 1;
        Some(sample)
    }
    
    fn clear(&mut self) {
        self.samples.clear();
        self.position = 0;
    }
    
    fn append(&mut self, samples: &[f32]) {
        self.samples.extend_from_slice(samples);
    }
    
    #[inline]
    fn remaining(&self) -> usize {
        self.samples.len() - self.position
    }
    
    /// 基于时间的填充需求检测
    fn needs_refill(&self) -> bool {
        // 计算剩余样本可播放的毫秒数
        let remaining_ms = (self.remaining() as u64 * 1000) / (self.sample_rate as u64 * self.channels as u64);
        remaining_ms < self.refill_threshold_ms as u64
    }
    
    /// 设置填充阈值
    fn set_refill_threshold(&mut self, threshold_ms: u32) {
        self.refill_threshold_ms = threshold_ms;
    }
    
    /// 获取当前缓冲区大小
    fn len(&self) -> usize {
        self.samples.len() - self.position
    }
}

/// Symphonia解码器的简单包装器，直接实现Source
pub struct SymphoniaSource {
    decoder: Arc<Mutex<SymphoniaDecoder>>,
}

impl SymphoniaSource {
    pub fn new(decoder: SymphoniaDecoder) -> Self {
        SymphoniaSource { 
            decoder: Arc::new(Mutex::new(decoder)) 
        }
    }
}

impl Iterator for SymphoniaSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if let Ok(mut decoder) = self.decoder.lock() {
            decoder.next()
        } else {
            None
        }
    }
}

impl Source for SymphoniaSource {
    fn current_frame_len(&self) -> Option<usize> {
        if let Ok(decoder) = self.decoder.lock() {
            decoder.current_frame_len()
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        // 返回目标声道数，而不是原始声道数
        if let Ok(decoder) = self.decoder.lock() {
            decoder.target_channels()
        } else {
            0
        }
    }

    fn sample_rate(&self) -> u32 {
        if let Ok(decoder) = self.decoder.lock() {
            decoder.sample_rate()
        } else {
            0
        }
    }

    fn total_duration(&self) -> Option<Duration> {
        if let Ok(decoder) = self.decoder.lock() {
            decoder.total_duration()
        } else {
            None
        }
    }
}

/// Symphonia解码器实现
pub struct SymphoniaDecoder {
    path: String,
    sample_rate: u32,
    total_duration: Option<Duration>,
    state: DecoderState,
    buffer: AudioBuffer,
    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    format: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    track_id: Option<u32>,
    current_sample: u64,
    // 多声道处理配置
    target_channels: u16,  // 目标声道数（通常是2，用于立体声）
    source_channels: u16, // 原始声道数，用于计算缓冲区
    channel_map: Option<Vec<usize>>,  // 声道映射表
}

impl SymphoniaDecoder {
    /// 创建新的解码器实例
    pub fn new(path: &str) -> Result<Self, String> {
        Self::new_with_buffer_duration(path, None)
    }
    
    /// 创建带有自定义缓冲区时长的解码器
    pub fn new_with_buffer_duration(path: &str, buffer_duration_ms: Option<u32>) -> Result<Self, String> {
        // 获取基本信息
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(file.try_clone().map_err(|e| e.to_string())?), Default::default());
        
        // 创建格式提示
        let mut hint = Hint::new();
        if let Some(extension) = Path::new(path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        
        // 探测格式
        let meta_opts: MetadataOptions = Default::default();
        let mut fmt_opts: FormatOptions = Default::default();
        // 启用无缝播放
        fmt_opts.enable_gapless = true;
        
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| format!("Failed to probe format: {}", e))?;
        
        let format = probed.format;
        
        // 查找第一个音轨
        let track = format.tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;
        
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let source_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2) as u16;
        
        let total_duration = if let (Some(n_frames), Some(sample_rate)) = (
            track.codec_params.n_frames,
            track.codec_params.sample_rate
        ) {
            Some(Duration::from_secs_f64(n_frames as f64 / sample_rate as f64))
        } else {
            None
        };
        
        // 使用基于时间的缓冲区大小计算
        let buffer_duration_ms = buffer_duration_ms.unwrap_or_else(|| {
            match sample_rate {
                0..=22050 => 200,        // 200ms for low quality
                22051..=44100 => 150,    // 150ms for standard quality
                44101..=48000 => 125,    // 125ms for medium quality
                48001..=96000 => 100,    // 100ms for high quality
                _ => 150,                // 150ms default
            }
        });
        
        let target_channels = 2u16; // 总是输出立体声
        let buffer_size = calculate_buffer_size(sample_rate, target_channels, buffer_duration_ms);
        
        // 创建声道映射表
        let channel_map = Self::create_channel_mapping(source_channels);
        
        Ok(SymphoniaDecoder {
            path: path.to_string(),
            sample_rate,
            source_channels,
            total_duration,
            state: DecoderState::Uninitialized,
            buffer: AudioBuffer::new(buffer_size, sample_rate, target_channels),
            decoder: None,
            format: None,
            track_id: None,
            current_sample: 0,
            target_channels,
            channel_map,
        })
    }
    
    /// 创建声道映射表，用于将多声道降混到立体声
    fn create_channel_mapping(channels: u16) -> Option<Vec<usize>> {
        match channels {
            // 5.1声道到立体声的映射：L, R, C, LFE, SL, SR -> L, R
            6 => Some(vec![0, 1]), // 只使用左右声道，忽略中置、低频增强和环绕声道
            
            // 7.1声道到立体声的映射：L, R, C, LFE, SL, SR, BL, BR -> L, R
            8 => Some(vec![0, 1]), // 只使用左右声道
            
            // 其他多声道配置的通用处理
            n if n > 2 => Some(vec![0, 1]), // 使用前两个声道作为左右声道
            
            _ => None, // 单声道或立体声不需要映射
        }
    }
    
    /// 在运行时调整缓冲区参数
    pub fn adjust_buffer_settings(&mut self, buffer_duration_ms: u32, refill_threshold_ms: u32) {
        let new_buffer_size = calculate_buffer_size(self.sample_rate, self.target_channels, buffer_duration_ms);
        self.buffer = AudioBuffer::new(new_buffer_size, self.sample_rate, self.target_channels);
        self.buffer.set_refill_threshold(refill_threshold_ms);
    }
    
    /// 获取当前缓冲区设置
    pub fn get_buffer_info(&self) -> (u32, u32, usize) {
        let refill_threshold_ms = self.buffer.refill_threshold_ms;
        let buffer_duration_ms = ((self.buffer.capacity as u64 * 1000) / 
                               (self.sample_rate as u64 * self.target_channels as u64)) as u32;
        (buffer_duration_ms, refill_threshold_ms, self.buffer.capacity)
    }
    
    /// 获取目标声道数
    pub fn target_channels(&self) -> u16 {
        self.target_channels
    }
    
    /// 获取源声道数
    pub fn source_channels(&self) -> u16 {
        self.source_channels
    }
    
    /// 跳转到指定时间点
    pub fn seek(&mut self, time: Duration) -> Result<(), String> {
        let target_seconds = time.as_secs_f64();
        let target_ts = (target_seconds * self.sample_rate as f64) as u64;
        
        // 更新当前采样位置
        self.current_sample = target_ts;
        
        // 清空缓冲区
        self.buffer.clear();
        
        // 如果解码器已初始化，执行seek
        if let (Some(format), Some(decoder)) = (&mut self.format, &mut self.decoder) {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp {
                ts: target_ts,
                track_id: self.track_id.unwrap(),
            };
            
            match format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to) {
                Ok(_) => {
                    decoder.reset();
                    self.state = DecoderState::Ready;
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Symphonia seek failed: {:?}", e);
                    // Seek失败，将current_sample重置为0
                    self.current_sample = 0;
                    // 重新初始化解码器以确保从头开始
                    self.state = DecoderState::Uninitialized;
                    return Err(format!("Seek failed: {:?}", e));
                }
            }
        } else {
            // 如果解码器未初始化，在下次调用next()时会自动初始化
            self.state = DecoderState::Uninitialized;
            Ok(())
        }
    }
    
    /// 初始化解码器
    fn initialize_decoder(&mut self) -> Result<(), String> {
        // 打开文件
        let file = File::open(&self.path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(file.try_clone().map_err(|e| e.to_string())?), Default::default());
        
        // 创建格式提示
        let mut hint = Hint::new();
        if let Some(extension) = Path::new(&self.path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        
        // 探测格式
        let meta_opts: MetadataOptions = Default::default();
        let mut fmt_opts: FormatOptions = Default::default();
        // 启用无缝播放
        fmt_opts.enable_gapless = true;
        
        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| format!("Failed to probe format: {}", e))?;
        
        let mut format = probed.format;
        
        // 查找第一个音轨
        let track = format.tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;
        
        let track_id = track.id;
        
        // 创建解码器
        let dec_opts: DecoderOptions = Default::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &dec_opts)
            .map_err(|e| format!("Failed to create decoder: {}", e))?;
        
        // 如果需要从特定位置开始，先seek
        if self.current_sample > 0 {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp {
                ts: self.current_sample,
                track_id,
            };
            
            match format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to) {
                Ok(_) => {
                    decoder.reset();
                }
                Err(_) => {
                    // Seek失败，从头开始
                    self.current_sample = 0;
                }
            }
        }
        
        self.format = Some(format);
        self.decoder = Some(decoder);
        self.track_id = Some(track_id);
        self.state = DecoderState::Ready;
        
        Ok(())
    }
    
    /// 填充缓冲区
    fn fill_buffer(&mut self) -> Result<(), String> {
        // 如果未初始化，先初始化
        if self.state == DecoderState::Uninitialized {
            if let Err(e) = self.initialize_decoder() {
                self.state = DecoderState::Error(e.clone());
                return Err(e);
            }
        }
        
        // 使用模式匹配检查状态，而不是使用 == 比较
        match &self.state {
            DecoderState::Error(_) | DecoderState::EndOfStream => {
                return Ok(());
            }
            _ => {}
        }
        
        let (format, decoder) = match (&mut self.format, &mut self.decoder) {
            (Some(format), Some(decoder)) => (format, decoder),
            _ => {
                self.state = DecoderState::Error("Decoder not initialized".to_string());
                return Err("Decoder not initialized".to_string());
            }
        };
        
        let track_id = self.track_id.unwrap();
        
        // 解码数据直到缓冲区填满或到达文件末尾
        let mut decoded_packets = 0;
        let max_packets = 20; // 适当增加最大解码包数
        
        while self.buffer.remaining() < self.buffer.capacity && decoded_packets < max_packets {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(Error::ResetRequired) => {
                    // 解码器需要重置，继续
                    decoder.reset();
                    continue;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // 到达文件末尾
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(e) => {
                    self.state = DecoderState::Error(format!("Failed to read packet: {}", e));
                    return Err(format!("Failed to read packet: {}", e));
                }
            };
            
            // 只有匹配的track_id的包才进行解码
            if packet.track_id() != track_id {
                continue;
            }
            
            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let mut samples = Vec::new();
                    Self::convert_audio_buffer(decoded, &mut samples, &self.channel_map);
                    self.buffer.append(&samples);
                    decoded_packets += 1;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(Error::DecodeError(_)) => {
                    // 跳过解码错误
                    continue;
                }
                Err(e) => {
                    self.state = DecoderState::Error(format!("Failed to decode: {}", e));
                    return Err(format!("Failed to decode: {}", e));
                }
            }
        }
        
        Ok(())
    }
    
    /// 将音频缓冲区转换为f32样本，支持多声道降混
    fn convert_audio_buffer(audio_buf: AudioBufferRef, samples: &mut Vec<f32>, channel_map: &Option<Vec<usize>>) {
        match audio_buf {
            AudioBufferRef::F32(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i].max(-1.0).min(1.0),
                samples,
                channel_map
            ),
            AudioBufferRef::U16(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 32767.5 - 1.0,
                samples,
                channel_map
            ),
            AudioBufferRef::S16(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 32768.0,
                samples,
                channel_map
            ),
            AudioBufferRef::U24(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| {
                    let u24_val = buf.chan(ch)[i];
                    u24_val.inner() as f32 / 8388607.5 - 1.0
                },
                samples,
                channel_map
            ),
            AudioBufferRef::S24(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| {
                    let i24_val = buf.chan(ch)[i];
                    i24_val.inner() as f32 / 8388608.0
                },
                samples,
                channel_map
            ),
            AudioBufferRef::U32(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 2147483647.5 - 1.0,
                samples,
                channel_map
            ),
            AudioBufferRef::S32(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 2147483648.0,
                samples,
                channel_map
            ),
            AudioBufferRef::U8(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 127.5 - 1.0,
                samples,
                channel_map
            ),
            AudioBufferRef::S8(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| buf.chan(ch)[i] as f32 / 128.0,
                samples,
                channel_map
            ),
            AudioBufferRef::F64(buf) => Self::process_audio_buffer(
                buf.frames(),
                buf.spec().channels.count(),
                |ch, i| {
                    let sample = buf.chan(ch)[i] as f32;
                    sample.max(-1.0).min(1.0)
                },
                samples,
                channel_map
            ),
        }
    }
    
    /// 处理音频缓冲区的通用函数，减少重复代码
    fn process_audio_buffer<F>(
        frames: usize,
        channels: usize,
        sample_fn: F,
        samples: &mut Vec<f32>,
        channel_map: &Option<Vec<usize>>
    )
    where
        F: Fn(usize, usize) -> f32,
    {
        match channel_map {
            Some(map) => {
                // 多声道降混到立体声
                for i in 0..frames {
                    for &ch_idx in map.iter() {
                        if ch_idx < channels {
                            samples.push(sample_fn(ch_idx, i));
                        }
                    }
                }
            }
            None => {
                // 直接输出所有声道（单声道或立体声）
                for i in 0..frames {
                    for ch in 0..channels {
                        samples.push(sample_fn(ch, i));
                    }
                }
            }
        }
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // 如果缓冲区为空或需要填充，尝试填充
        if self.buffer.is_empty() || self.buffer.needs_refill() {
            if let Err(e) = self.fill_buffer() {
                eprintln!("Error filling buffer: {}", e);
                return None;
            }
            
            // 如果填充后仍然为空，说明已到达文件末尾
            if self.buffer.is_empty() {
                return None;
            }
        }
        
        let sample = self.buffer.next();
        if sample.is_some() {
            self.current_sample += 1;
        }
        
        sample
    }
}

impl Source for SymphoniaDecoder {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        // 返回目标声道数，而不是原始声道数
        self.target_channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }
}

impl Drop for SymphoniaDecoder {
    fn drop(&mut self) {
        // 清理资源
    }
}