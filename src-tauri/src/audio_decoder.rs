use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// --- 配置常量 ---
// 将默认缓冲时间增加到 3000ms (3秒)，提供更大的安全余量
const DEFAULT_BUFFER_DURATION_MS: u32 = 3000;
// 恢复较小的批处理大小，确保能更频繁地释放锁，避免阻塞音频线程太久
const SOURCE_BATCH_SIZE: usize = 4096;

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
        // 设置填充阈值：当缓冲区剩余时间少于总容量的 40% 时请求填充
        let refill_threshold_ms =
            (capacity as u64 * 1000 / (sample_rate as u64 * channels as u64) * 40 / 100) as u32;

        Self {
            samples: Vec::with_capacity(capacity),
            position: 0,
            capacity,
            sample_rate,
            channels,
            refill_threshold_ms: refill_threshold_ms.max(100), // 至少保证100ms余量
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
        (remaining_samples as u64 * 1000)
            < (self.sample_rate as u64 * self.channels as u64 * self.refill_threshold_ms as u64)
    }

    fn set_refill_threshold(&mut self, threshold_ms: u32) {
        self.refill_threshold_ms = threshold_ms;
    }
}

pub struct SymphoniaSource {
    decoder: Arc<Mutex<SymphoniaDecoder>>,
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
}

impl SymphoniaSource {
    pub fn new(decoder: SymphoniaDecoder) -> Self {
        SymphoniaSource {
            decoder: Arc::new(Mutex::new(decoder)),
            // 增大本地缓存到 4096
            chunk_buffer: Vec::with_capacity(SOURCE_BATCH_SIZE),
            chunk_pos: 0,
        }
    }
}

impl Iterator for SymphoniaSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.chunk_pos < self.chunk_buffer.len() {
            let sample = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(sample);
        }

        if let Ok(mut decoder) = self.decoder.lock() {
            self.chunk_buffer.clear();
            self.chunk_pos = 0;

            // 使用定义的常量 SOURCE_BATCH_SIZE (4096)
            // 这样每次获取锁可以拿到更多数据，减少锁争抢
            for _ in 0..SOURCE_BATCH_SIZE {
                if let Some(sample) = decoder.next() {
                    self.chunk_buffer.push(sample);
                } else {
                    break;
                }
            }
        }

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
        if let Ok(decoder) = self.decoder.lock() {
            decoder.current_frame_len()
        } else {
            None
        }
    }

    fn channels(&self) -> u16 {
        if let Ok(decoder) = self.decoder.lock() {
            decoder.target_channels()
        } else {
            2
        }
    }

    fn sample_rate(&self) -> u32 {
        if let Ok(decoder) = self.decoder.lock() {
            decoder.sample_rate()
        } else {
            44100
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

pub struct SymphoniaDecoder {
    path: String,
    sample_rate: u32,
    total_duration: Option<Duration>,
    state: DecoderState,
    buffer: AudioBuffer,
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

        // --- 核心修复：使用更大的默认缓冲区 ---
        let buffer_duration_ms = buffer_duration_ms.unwrap_or(DEFAULT_BUFFER_DURATION_MS);

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
            scratch_buffer: Vec::with_capacity(SOURCE_BATCH_SIZE * 2), // 稍微预分配大一点
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
        let max_packets = 4; // 减少单次解码包数，避免阻塞

        // 只要缓冲区未满 (remaining < capacity)，就尝试解码
        while self.buffer.remaining() < self.buffer.capacity && decoded_packets < max_packets {
            // 如果已经达到了填充阈值，提前退出以释放锁
            if !self.buffer.needs_refill() && decoded_packets > 0 {
                break;
            }

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
                    self.state = DecoderState::Error(format!("Read packet error: {}", e));
                    return Err(format!("Read packet error: {}", e));
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    self.scratch_buffer.clear();
                    Self::convert_audio_buffer(
                        decoded,
                        &mut self.scratch_buffer,
                        &self.channel_map,
                    );
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

    fn convert_audio_buffer(
        audio_buf: AudioBufferRef,
        samples: &mut Vec<f32>,
        channel_map: &Option<Vec<usize>>,
    ) {
        let frames = audio_buf.frames();
        let channels = audio_buf.spec().channels.count();
        let target_channels = if let Some(map) = channel_map {
            map.len()
        } else {
            channels
        };
        samples.reserve(frames * target_channels);
        match audio_buf {
            AudioBufferRef::F32(buf) => {
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
            _ => {}
        }
    }

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
            for i in 0..frames {
                for &ch_idx in map {
                    if ch_idx < src_channels {
                        let sample = converter(&planes[ch_idx][i]);
                        out.push(sample.max(-1.0).min(1.0));
                    }
                }
            }
        } else {
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
        if self.buffer.is_empty() || self.buffer.needs_refill() {
            let _ = self.fill_buffer();
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
    fn drop(&mut self) {}
}
