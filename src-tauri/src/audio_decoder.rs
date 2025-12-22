use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use std::thread;
use crossbeam_channel::{unbounded, Receiver};
use symphonia::core::audio::AudioBufferRef;
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
/// 作为一个简单的 FIFO 队列使用
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
            // 针对IO延迟优化：降低阈值到 100ms，更早触发填充
            // 配合更大的缓冲区，可以在IO繁忙时提前填充，避免中断
            refill_threshold_ms: 100,
        }
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.position >= self.samples.len()
    }

    #[inline(always)]
    fn next(&mut self) -> Option<f32> {
        if self.position < self.samples.len() {
            let sample = self.samples[self.position];
            self.position += 1;
            Some(sample)
        } else {
            None
        }
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
        let remaining_samples = self.remaining();
        // 避免除法，转换为乘法比较: remaining * 1000 < rate * channels * threshold
        (remaining_samples as u64 * 1000)
            < (self.sample_rate as u64 * self.channels as u64 * self.refill_threshold_ms as u64)
    }

    fn set_refill_threshold(&mut self, threshold_ms: u32) {
        self.refill_threshold_ms = threshold_ms;
    }
}

/// 无锁版本的 Symphonia 解码器源
/// 使用后台线程解码，通过无锁通道传递数据，完全消除锁竞争
pub struct LockFreeSymphoniaSource {
    // 无锁通道接收端，从后台解码线程接收音频样本
    receiver: Receiver<f32>,
    // 后台解码线程的句柄
    _decoder_thread: thread::JoinHandle<()>,
    // 停止标志（原子操作，无锁）
    stop_flag: Arc<AtomicBool>,
    // 缓存 Source trait 方法的返回值
    cached_channels: u16,
    cached_sample_rate: u32,
    cached_total_duration: Option<Duration>,
    // 本地缓存，用于批量接收数据
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
}

/// Symphonia解码器的包装器，实现了 rodio::Source
///
/// 优化说明：引入了本地缓存 (chunk_buffer) 以减少锁争用。
/// 针对锁竞争优化：缓存 Source trait 方法的返回值，避免频繁加锁。
/// 
/// 注意：保留旧版本作为备用，新版本使用无锁架构
pub struct SymphoniaSource {
    decoder: Arc<Mutex<SymphoniaDecoder>>,
    // 本地缓存，用于批量获取数据，避免每次 next() 都加锁
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
    // 缓存 Source trait 方法的返回值，避免频繁加锁
    cached_channels: u16,
    cached_sample_rate: u32,
    cached_total_duration: Option<Duration>,
}

impl LockFreeSymphoniaSource {
    /// 创建无锁版本的音频源
    /// 启动后台解码线程，通过无锁通道传递数据
    pub fn new(mut decoder: SymphoniaDecoder) -> Self {
        // 缓存元数据（这些值在解码器生命周期内不变）
        let channels = decoder.target_channels();
        let sample_rate = decoder.sample_rate();
        let total_duration = decoder.total_duration();
        
        // 创建无锁通道
        let (sender, receiver) = unbounded();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        
        // 预填充缓冲区
        if let Err(e) = decoder.prefill_buffer() {
            eprintln!("警告: 预填充失败: {}", e);
        }
        
        // 启动后台解码线程
        let decoder_thread = thread::spawn(move || {
            // 批量解码并发送数据
            let mut batch = Vec::with_capacity(16384); // 约 370ms @ 44.1kHz stereo
            
            loop {
                // 检查停止标志（无锁原子操作）
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
                
                // 批量解码
                batch.clear();
                for _ in 0..16384 {
                    if let Some(sample) = decoder.next() {
                        batch.push(sample);
                    } else {
                        // 解码器耗尽（文件结束）
                        break;
                    }
                }
                
                if batch.is_empty() {
                    // 没有更多数据，发送结束信号
                    break;
                }
                
                // 批量发送数据（无锁操作）
                for sample in &batch {
                    if sender.send(*sample).is_err() {
                        // 接收端已关闭，停止解码
                        break;
                    }
                }
            }
        });
        
        LockFreeSymphoniaSource {
            receiver,
            _decoder_thread: decoder_thread,
            stop_flag,
            cached_channels: channels,
            cached_sample_rate: sample_rate,
            cached_total_duration: total_duration,
            chunk_buffer: Vec::with_capacity(16384),
            chunk_pos: 0,
        }
    }
}

impl Iterator for LockFreeSymphoniaSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // 1. 快速路径：如果本地缓存有数据，直接返回
        if self.chunk_pos < self.chunk_buffer.len() {
            let sample = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(sample);
        }

        // 2. 从无锁通道批量接收数据
        // 针对性能优化：先尝试非阻塞接收，如果数据充足则快速填充
        // 如果数据不足，再使用阻塞接收，避免频繁轮询
        self.chunk_buffer.clear();
        self.chunk_pos = 0;
        
        // 首先尝试非阻塞批量接收
        let mut received_count = 0;
        loop {
            match self.receiver.try_recv() {
                Ok(sample) => {
                    self.chunk_buffer.push(sample);
                    received_count += 1;
                    // 如果已经接收到足够的数据（16384个样本），跳出循环
                    if received_count >= 16384 {
                        break;
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    // 通道暂时为空
                    if received_count > 0 {
                        // 已经接收到一些数据，先返回这些数据
                        break;
                    }
                    // 如果没有接收到任何数据，使用阻塞接收等待数据
                    // 使用很短的超时时间，避免长时间阻塞
                    match self.receiver.recv_timeout(Duration::from_micros(100)) {
                        Ok(sample) => {
                            self.chunk_buffer.push(sample);
                            received_count += 1;
                        }
                        Err(_) => {
                            // 超时或通道关闭，跳出循环
                            break;
                        }
                    }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    // 通道已关闭，解码线程已结束
                    break;
                }
            }
        }

        // 3. 再次检查缓存
        if self.chunk_pos < self.chunk_buffer.len() {
            let sample = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(sample);
        }

        None
    }
}

impl Source for LockFreeSymphoniaSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.cached_channels
    }

    fn sample_rate(&self) -> u32 {
        self.cached_sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.cached_total_duration
    }
}

impl Drop for LockFreeSymphoniaSource {
    fn drop(&mut self) {
        // 设置停止标志（无锁原子操作）
        self.stop_flag.store(true, Ordering::Relaxed);
        // 线程会在下次循环检查时退出
    }
}

impl SymphoniaSource {
    pub fn new(decoder: SymphoniaDecoder) -> Self {
        // 初始化时获取一次锁，缓存所有不变的值
        let channels = decoder.target_channels();
        let sample_rate = decoder.sample_rate();
        let total_duration = decoder.total_duration();
        
        SymphoniaSource {
            decoder: Arc::new(Mutex::new(decoder)),
            // 针对IO延迟优化：预分配 16384 个采样点的缓存 (约 370ms @ 44.1kHz stereo)
            // 更大的本地缓存可以减少锁竞争，并在IO延迟时提供更多缓冲
            chunk_buffer: Vec::with_capacity(16384),
            chunk_pos: 0,
            // 缓存 Source trait 方法的返回值，避免频繁加锁
            cached_channels: channels,
            cached_sample_rate: sample_rate,
            cached_total_duration: total_duration,
        }
    }
}

impl Iterator for SymphoniaSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        // 1. 快速路径：如果本地缓存有数据，直接返回
        if self.chunk_pos < self.chunk_buffer.len() {
            let sample = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(sample);
        }

        // 2. 慢速路径：本地缓存耗尽，获取锁并批量拉取
        // 针对锁竞争优化：尽量减少锁持有时间
        // 策略：快速批量拉取数据，然后立即释放锁
        self.chunk_buffer.clear();
        self.chunk_pos = 0;

        // 针对锁竞争优化：使用更大的批量大小，减少锁获取频率
        // 同时确保尽快释放锁，避免长时间持有
        let batch_size = 8192;
        
        // 快速获取锁并批量拉取
        {
            let mut decoder = match self.decoder.try_lock() {
                Ok(guard) => guard,
                Err(_) => {
                    // try_lock 失败，使用阻塞锁
                    // 但我们应该尽快完成操作并释放锁
                    self.decoder.lock().unwrap()
                }
            };
            
            // 快速批量拉取数据到本地缓存
            for _ in 0..batch_size {
                if let Some(sample) = decoder.next() {
                    self.chunk_buffer.push(sample);
                } else {
                    // 解码器耗尽（文件结束或暂时无数据）
                    break;
                }
            }
            // 锁在这里自动释放
        }

        // 3. 再次检查缓存（如果刚才拉取到了数据）
        if self.chunk_pos < self.chunk_buffer.len() {
            let sample = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(sample);
        }

        None
    }
}

impl Source for SymphoniaSource {
    fn current_frame_len(&self) -> Option<usize> {
        // 这个方法返回 None 是合理的，因为流式解码的长度未知
        None
    }

    fn channels(&self) -> u16 {
        // 使用缓存值，避免加锁
        self.cached_channels
    }

    fn sample_rate(&self) -> u32 {
        // 使用缓存值，避免加锁
        self.cached_sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        // 使用缓存值，避免加锁
        self.cached_total_duration
    }
}

/// Symphonia解码器实现
pub struct SymphoniaDecoder {
    path: String,
    sample_rate: u32,
    total_duration: Option<Duration>,
    state: DecoderState,

    // 内部主缓冲区
    buffer: AudioBuffer,
    // 暂存缓冲区，用于避免在解码循环中反复分配内存
    scratch_buffer: Vec<f32>,

    decoder: Option<Box<dyn symphonia::core::codecs::Decoder>>,
    format: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    track_id: Option<u32>,
    current_sample: u64,

    target_channels: u16,
    source_channels: u16,
    channel_map: Option<Vec<usize>>,
}

impl SymphoniaDecoder {
    pub fn new(path: &str) -> Result<Self, String> {
        Self::new_with_buffer_duration(path, None)
    }

    pub fn new_with_buffer_duration(
        path: &str,
        buffer_duration_ms: Option<u32>,
    ) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(
            Box::new(file.try_clone().map_err(|e| e.to_string())?),
            Default::default(),
        );

        let mut hint = Hint::new();
        if let Some(extension) = Path::new(path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }

        let meta_opts: MetadataOptions = Default::default();
        let mut fmt_opts: FormatOptions = Default::default();
        fmt_opts.enable_gapless = true;

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| format!("Failed to probe format: {}", e))?;

        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;

        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let source_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2) as u16;

        let total_duration = if let (Some(n_frames), Some(sample_rate)) =
            (track.codec_params.n_frames, track.codec_params.sample_rate)
        {
            Some(Duration::from_secs_f64(
                n_frames as f64 / sample_rate as f64,
            ))
        } else {
            None
        };

        // 针对IO延迟优化：大幅增加缓冲区大小
        // 当硬盘占用率100%时，文件读取可能延迟，需要更大的缓冲区来应对
        // 更大的缓冲区可以提供足够的缓冲时间，即使IO延迟几百毫秒也不会中断
        let buffer_duration_ms = buffer_duration_ms.unwrap_or_else(|| match sample_rate {
            0..=48000 => 500,
            _ => 400,
        });

        let target_channels = 2u16;
        let buffer_size = calculate_buffer_size(sample_rate, target_channels, buffer_duration_ms);
        let channel_map = Self::create_channel_mapping(source_channels);

        Ok(SymphoniaDecoder {
            path: path.to_string(),
            sample_rate,
            source_channels,
            total_duration,
            state: DecoderState::Uninitialized,
            buffer: AudioBuffer::new(buffer_size, sample_rate, target_channels),
            scratch_buffer: Vec::with_capacity(4096), // 预分配暂存区
            decoder: None,
            format: None,
            track_id: None,
            current_sample: 0,
            target_channels,
            channel_map,
        })
    }

    fn create_channel_mapping(channels: u16) -> Option<Vec<usize>> {
        match channels {
            6 => Some(vec![0, 1]), // 5.1 -> Stereo
            8 => Some(vec![0, 1]), // 7.1 -> Stereo
            n if n > 2 => Some(vec![0, 1]),
            _ => None,
        }
    }

    pub fn adjust_buffer_settings(&mut self, buffer_duration_ms: u32, refill_threshold_ms: u32) {
        let new_buffer_size =
            calculate_buffer_size(self.sample_rate, self.target_channels, buffer_duration_ms);
        // 保存旧数据
        self.buffer = AudioBuffer::new(new_buffer_size, self.sample_rate, self.target_channels);
        self.buffer.set_refill_threshold(refill_threshold_ms);
    }

    pub fn get_buffer_info(&self) -> (u32, u32, usize) {
        let buffer_duration_ms = ((self.buffer.capacity as u64 * 1000)
            / (self.sample_rate as u64 * self.target_channels as u64))
            as u32;
        (
            buffer_duration_ms,
            self.buffer.refill_threshold_ms,
            self.buffer.capacity,
        )
    }

    pub fn target_channels(&self) -> u16 {
        self.target_channels
    }

    pub fn source_channels(&self) -> u16 {
        self.source_channels
    }

    /// 预填充缓冲区，确保在播放开始前有足够的数据
    /// 针对IO延迟优化：更积极的填充策略，确保有足够缓冲应对IO繁忙
    pub fn prefill_buffer(&mut self) -> Result<(), String> {
        // 初始化解码器（如果尚未初始化）
        if self.state == DecoderState::Uninitialized {
            if let Err(e) = self.initialize_decoder() {
                self.state = DecoderState::Error(e.clone());
                return Err(e);
            }
        }

        // 针对IO延迟优化：填充缓冲区直到达到至少 95% 容量
        // 更大的预填充可以确保即使在IO繁忙时也有足够的数据缓冲
        let target_size = (self.buffer.capacity * 95) / 100;
        let mut attempts = 0;
        // 针对IO延迟：增加最大尝试次数，允许更多次重试以应对IO延迟
        const MAX_ATTEMPTS: usize = 200; // 从 50 增加到 200，应对IO延迟

        while self.buffer.remaining() < target_size && attempts < MAX_ATTEMPTS {
            match self.fill_buffer() {
                Ok(_) => {
                    // 填充成功，继续
                }
                Err(e) => {
                    // 如果填充失败，检查是否是因为流结束
                    if self.state == DecoderState::EndOfStream {
                        break; // 文件已结束，无需继续填充
                    }
                    // 对于IO错误，允许重试，不立即返回错误
                    // 这可以应对临时的IO延迟
                    if attempts > 10 && self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
                        // 如果尝试多次后缓冲区仍然不足50%，返回错误
                        return Err(format!("Buffer prefill failed after {} attempts: {}", attempts, e));
                    }
                    // 否则继续尝试
                }
            }
            attempts += 1;
        }

        // 即使没有达到95%，如果已经填充了足够的数据（至少50%），也认为成功
        if self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
            return Err(format!("Buffer prefill incomplete: only {}% filled", 
                (self.buffer.remaining() * 100) / self.buffer.capacity));
        }

        Ok(())
    }

    pub fn seek(&mut self, time: Duration) -> Result<(), String> {
        let target_seconds = time.as_secs_f64();
        let target_ts = (target_seconds * self.sample_rate as f64) as u64;

        self.current_sample = target_ts;
        self.buffer.clear();

        if let (Some(format), Some(decoder)) = (&mut self.format, &mut self.decoder) {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp {
                ts: target_ts,
                track_id: self.track_id.unwrap(),
            };

            match format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to) {
                Ok(_) => {
                    decoder.reset();
                    self.state = DecoderState::Ready;
                    Ok(())
                }
                Err(e) => {
                    // Seek 失败尝试软重置
                    self.current_sample = 0;
                    self.state = DecoderState::Uninitialized;
                    Err(format!("Seek failed: {:?}", e))
                }
            }
        } else {
            self.state = DecoderState::Uninitialized;
            Ok(())
        }
    }

    fn initialize_decoder(&mut self) -> Result<(), String> {
        let file = File::open(&self.path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(
            Box::new(file.try_clone().map_err(|e| e.to_string())?),
            Default::default(),
        );

        let mut hint = Hint::new();
        if let Some(extension) = Path::new(&self.path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }

        let meta_opts: MetadataOptions = Default::default();
        let mut fmt_opts: FormatOptions = Default::default();
        fmt_opts.enable_gapless = true;

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &fmt_opts, &meta_opts)
            .map_err(|e| format!("Failed to probe format: {}", e))?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or("No audio track found")?;

        let track_id = track.id;

        let dec_opts: DecoderOptions = Default::default();
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &dec_opts)
            .map_err(|e| format!("Failed to create decoder: {}", e))?;

        if self.current_sample > 0 {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp {
                ts: self.current_sample,
                track_id,
            };
            if format
                .seek(symphonia::core::formats::SeekMode::Accurate, seek_to)
                .is_ok()
            {
                decoder.reset();
            } else {
                self.current_sample = 0;
            }
        }

        self.format = Some(format);
        self.decoder = Some(decoder);
        self.track_id = Some(track_id);
        self.state = DecoderState::Ready;

        Ok(())
    }

    fn fill_buffer(&mut self) -> Result<(), String> {
        if self.state == DecoderState::Uninitialized {
            if let Err(e) = self.initialize_decoder() {
                self.state = DecoderState::Error(e.clone());
                return Err(e);
            }
        }

        match &self.state {
            DecoderState::Error(_) | DecoderState::EndOfStream => return Ok(()),
            _ => {}
        }

        // 避免借用检查器问题，先获取必要的可变引用
        let format = self.format.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let track_id = self.track_id.unwrap();

        let mut decoded_packets = 0;
        // 针对锁竞争和IO延迟优化：增加每次解码的包数量
        // 一次性填充更多数据，减少锁持有时间和IO访问频率
        let max_packets = 50; // 从 20 增加到 50，应对IO延迟和锁竞争

        // 针对锁竞争优化：快速填充缓冲区，尽快完成操作
        // 限制单次填充的目标，避免长时间持有锁
        let target_fill = (self.buffer.capacity * 80) / 100; // 填充到80%即可，避免过度填充

        while self.buffer.remaining() < target_fill && decoded_packets < max_packets {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(Error::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(e) => {
                    // 非致命错误可能需要继续，但这里简化为错误状态
                    self.state = DecoderState::Error(format!("Read packet error: {}", e));
                    return Err(format!("Read packet error: {}", e));
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    // 使用暂存缓冲区，避免分配
                    self.scratch_buffer.clear();

                    // 转换并写入 scratch_buffer
                    Self::convert_audio_buffer(
                        decoded,
                        &mut self.scratch_buffer,
                        &self.channel_map,
                    );

                    // 将 scratch_buffer 的内容追加到主 buffer
                    self.buffer.append(&self.scratch_buffer);
                    decoded_packets += 1;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(Error::DecodeError(_)) => continue,
                Err(e) => {
                    self.state = DecoderState::Error(format!("Decode error: {}", e));
                    return Err(format!("Decode error: {}", e));
                }
            }
        }

        Ok(())
    }

    /// 优化后的转换函数：直接展开循环，避免闭包，利于 SIMD
    fn convert_audio_buffer(
        audio_buf: AudioBufferRef,
        samples: &mut Vec<f32>,
        channel_map: &Option<Vec<usize>>,
    ) {
        let frames = audio_buf.frames();
        let channels = audio_buf.spec().channels.count();

        // 预分配空间
        let target_channels = if let Some(map) = channel_map {
            map.len()
        } else {
            channels
        };
        samples.reserve(frames * target_channels);

        match audio_buf {
            AudioBufferRef::F32(buf) => {
                // 修复：将 planes() 调用拆分为两步，避免临时值被丢弃
                let planes = buf.planes();
                let src = planes.planes();
                Self::copy_planes(src, samples, frames, channels, channel_map, |s| *s);
            }
            AudioBufferRef::S16(buf) => {
                let planes = buf.planes();
                let src = planes.planes();
                Self::copy_planes(src, samples, frames, channels, channel_map, |s| {
                    (*s as f32) / 32768.0
                });
            }
            AudioBufferRef::U8(buf) => {
                let planes = buf.planes();
                let src = planes.planes();
                Self::copy_planes(src, samples, frames, channels, channel_map, |s| {
                    (*s as f32 - 128.0) / 128.0
                });
            }
            AudioBufferRef::S32(buf) => {
                let planes = buf.planes();
                let src = planes.planes();
                Self::copy_planes(src, samples, frames, channels, channel_map, |s| {
                    (*s as f32) / 2147483648.0
                });
            }
            AudioBufferRef::S24(buf) => {
                let planes = buf.planes();
                let src = planes.planes();
                // i24 类型的内部表示是一个 i32，范围是 -8388608 到 8388607
                Self::copy_planes(src, samples, frames, channels, channel_map, |s| {
                    (s.inner() as f32) / 8388608.0
                });
            }
            _ => {
                // 对于未优化的格式，静默处理或添加更多 match arm
                eprintln!("Unsupported audio buffer format encountered");
            }
        }
    }

    /// 核心拷贝逻辑：按帧交错 (Interleave)
    #[inline(always)]
    fn copy_planes<T, F>(
        planes: &[&[T]],
        out: &mut Vec<f32>,
        frames: usize,
        src_channels: usize,
        map: &Option<Vec<usize>>,
        converter: F,
    ) where
        T: Copy,
        F: Fn(&T) -> f32,
    {
        if let Some(map) = map {
            // 映射模式
            for i in 0..frames {
                for &ch_idx in map {
                    if ch_idx < src_channels {
                        let sample = converter(&planes[ch_idx][i]);
                        out.push(sample.max(-1.0).min(1.0));
                    }
                }
            }
        } else {
            // 直通模式 (Interleave)
            for i in 0..frames {
                for ch in 0..src_channels {
                    let sample = converter(&planes[ch][i]);
                    out.push(sample);
                }
            }
        }
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // 更积极的缓冲区填充策略：即使还没到阈值，也要保持缓冲区足够满
        // 这可以减少在系统繁忙时的中断概率
        if self.buffer.is_empty() || self.buffer.needs_refill() {
            // 尝试填充缓冲区，如果失败则返回 None
            if let Err(e) = self.fill_buffer() {
                eprintln!("Buffer fill error: {}", e);
                // 即使填充失败，如果缓冲区还有数据，仍然返回
                // 只有在缓冲区完全为空时才返回 None
                if self.buffer.is_empty() {
                    return None;
                }
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
        // 自动清理
    }
}
