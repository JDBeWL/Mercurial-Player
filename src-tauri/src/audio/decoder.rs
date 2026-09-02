//! 音频解码器模块
//!
//! 使用 Symphonia 库实现高性能音频解码，支持多种格式。

use crate::error::AppError;
use crossbeam_channel::{Receiver, bounded};
use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

#[derive(Debug, PartialEq, Eq)]
enum DecoderState {
    Uninitialized,
    Ready,
    EndOfStream,
    Error(String),
}

#[must_use]
const fn calculate_buffer_size(sample_rate: u32, channels: u16, duration_ms: u32) -> usize {
    ((sample_rate as u64 * duration_ms as u64 * channels as u64) / 1000) as usize
}

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
            let s = self.samples[self.position];
            self.position += 1;
            // 避免“已消费样本”长期滞留导致内存缓慢上涨
            self.compact_if_needed();
            Some(s)
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

    fn needs_refill(&self) -> bool {
        (self.remaining() as u64 * 1000)
            < (self.sample_rate as u64 * self.channels as u64 * self.refill_threshold_ms as u64)
    }

    fn set_refill_threshold(&mut self, threshold_ms: u32) {
        self.refill_threshold_ms = threshold_ms;
    }

    #[inline]
    fn compact_if_needed(&mut self) {
        // 至少回收 4096 个样本，且已消费超过当前数组一半时再压缩，避免频繁搬移
        if self.position >= 4096 && self.position * 2 >= self.samples.len() {
            let remaining = self.samples.len() - self.position;
            self.samples.copy_within(self.position.., 0);
            self.samples.truncate(remaining);
            self.position = 0;
        }
    }
}

/// 解码填充目标:每次填充到缓冲区容量的 80%,留出余量避免写满后阻塞
const TARGET_FILL_RATIO_NUMER: usize = 80;
const TARGET_FILL_RATIO_DENOM: usize = 100;
/// 单次填充最多解码的包数,限制单次填充耗时,避免阻塞播放线程过久
const MAX_PACKETS_PER_FILL: u32 = 50;

pub struct LockFreeSymphoniaSource {
    receiver: Option<Receiver<f32>>,
    /// 保留句柄以便 Drop 时 join,避免解码线程 detached 存活到进程退出
    decoder_thread: Option<thread::JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    producer_finished: Arc<AtomicBool>,
    cached_channels: u16,
    cached_sample_rate: u32,
    cached_total_duration: Option<Duration>,
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
}

impl LockFreeSymphoniaSource {
    // 有界队列：避免“生产者解码速度 > 消费者播放速度”时无限堆积内存
    // 约等于 5 秒 48kHz 立体声 PCM（480000 samples，约 2MB）
    const CHANNEL_CAPACITY_SAMPLES: usize = 48_000 * 2 * 5;

    pub fn new(mut decoder: SymphoniaDecoder) -> Self {
        let (channels, sample_rate, total_duration) = (
            decoder.target_channels(),
            decoder.sample_rate(),
            decoder.total_duration(),
        );
        let (sender, receiver) = bounded(Self::CHANNEL_CAPACITY_SAMPLES);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let producer_finished = Arc::new(AtomicBool::new(false));
        let producer_finished_clone = Arc::clone(&producer_finished);
        let _ = decoder.prefill_buffer();

        let decoder_thread = thread::spawn(move || {
            let mut batch = Vec::with_capacity(16384);
            loop {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }
                batch.clear();
                for _ in 0..16384 {
                    if let Some(s) = decoder.next() {
                        batch.push(s);
                    } else {
                        break;
                    }
                }
                if batch.is_empty() {
                    break;
                }

                // 有界通道 + 阻塞发送，给消费端施加背压，防止内存无限增长
                for s in &batch {
                    if sender.send(*s).is_err() {
                        producer_finished_clone.store(true, Ordering::Release);
                        return;
                    }
                }
            }
            producer_finished_clone.store(true, Ordering::Release);
        });

        Self {
            receiver: Some(receiver),
            decoder_thread: Some(decoder_thread),
            stop_flag,
            producer_finished,
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
        let receiver = self.receiver.as_ref()?;

        if self.chunk_pos < self.chunk_buffer.len() {
            let s = self.chunk_buffer[self.chunk_pos];
            self.chunk_pos += 1;
            return Some(s);
        }

        self.chunk_buffer.clear();
        self.chunk_pos = 0;

        let first = loop {
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(s) => break Some(s),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if self.producer_finished.load(Ordering::Acquire) && receiver.is_empty() {
                        break None;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break None,
            }
        }?;

        self.chunk_buffer.push(first);
        while self.chunk_buffer.len() < 16384 {
            match receiver.try_recv() {
                Ok(s) => self.chunk_buffer.push(s),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }

        let s = self.chunk_buffer[self.chunk_pos];
        self.chunk_pos += 1;
        Some(s)
    }
}

impl Source for LockFreeSymphoniaSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> std::num::NonZero<u16> {
        std::num::NonZero::new(self.cached_channels).unwrap_or(std::num::NonZero::new(2).unwrap())
    }
    fn sample_rate(&self) -> std::num::NonZero<u32> {
        std::num::NonZero::new(self.cached_sample_rate)
            .unwrap_or(std::num::NonZero::new(48000).unwrap())
    }
    fn total_duration(&self) -> Option<Duration> {
        self.cached_total_duration
    }
}

impl Drop for LockFreeSymphoniaSource {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        // 先释放 receiver:若解码线程正阻塞在有界通道的 send 上,通道断开
        // 会让 send 立即失败并退出线程,随后的 join 不会死锁
        drop(self.receiver.take());
        // join 解码线程,避免 detached 线程在后台存活到进程退出
        if let Some(handle) = self.decoder_thread.take() {
            let _ = handle.join();
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
    decoder: Option<Box<dyn symphonia::core::codecs::audio::AudioDecoder>>,
    format: Option<Box<dyn symphonia::core::formats::FormatReader>>,
    track_id: Option<u32>,
    current_sample: u64,
    target_channels: u16,
    source_channels: u16,
    channel_map: Option<Vec<usize>>,
}

impl SymphoniaDecoder {
    pub fn new(path: &str) -> Result<Self, AppError> {
        Self::new_with_buffer_duration(path, None)
    }

    pub fn new_with_buffer_duration(
        path: &str,
        buffer_duration_ms: Option<u32>,
    ) -> Result<Self, AppError> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(
            Box::new(file.try_clone().map_err(|e| e.to_string())?),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }
        let fmt_opts: FormatOptions = FormatOptions::default();
        let format = symphonia::default::get_probe()
            .probe(&hint, mss, fmt_opts, MetadataOptions::default())
            .map_err(|e| format!("Failed to probe format: {e}"))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("No audio track found")?;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(CodecParameters::audio)
            .ok_or("No audio codec parameters")?;
        let sample_rate = audio_params.sample_rate.unwrap_or(44100);
        let source_channels = audio_params
            .channels
            .as_ref()
            .map(|c| c.count())
            .unwrap_or(2) as u16;
        let total_duration = track.num_frames.and_then(|n| {
            audio_params
                .sample_rate
                .map(|sr| Duration::from_secs_f64(n as f64 / sr as f64))
        });
        let buffer_duration_ms =
            buffer_duration_ms.unwrap_or(if sample_rate <= 48000 { 500 } else { 400 });
        let target_channels = 2u16;
        let buffer_size = calculate_buffer_size(sample_rate, target_channels, buffer_duration_ms);
        let channel_map = Self::create_channel_mapping(source_channels);

        Ok(Self {
            path: path.to_string(),
            sample_rate,
            source_channels,
            total_duration,
            state: DecoderState::Uninitialized,
            buffer: AudioBuffer::new(buffer_size, sample_rate, target_channels),
            scratch_buffer: Vec::with_capacity(4096),
            decoder: None,
            format: None,
            track_id: None,
            current_sample: 0,
            target_channels,
            channel_map,
        })
    }

    fn create_channel_mapping(channels: u16) -> Option<Vec<usize>> {
        // 返回 None 表示需要使用专门的混音算法，而不是简单的声道映射
        match channels {
            1 | 2 => None,     // 单声道或立体声，不需要映射
            _ => Some(vec![]), // 多声道，标记需要混音处理
        }
    }

    pub fn adjust_buffer_settings(&mut self, buffer_duration_ms: u32, refill_threshold_ms: u32) {
        let new_size =
            calculate_buffer_size(self.sample_rate, self.target_channels, buffer_duration_ms);
        self.buffer = AudioBuffer::new(new_size, self.sample_rate, self.target_channels);
        self.buffer.set_refill_threshold(refill_threshold_ms);
    }

    #[must_use]
    pub fn get_buffer_info(&self) -> (u32, u32, usize) {
        let dur = ((self.buffer.capacity as u64 * 1000)
            / (self.sample_rate as u64 * self.target_channels as u64)) as u32;
        (dur, self.buffer.refill_threshold_ms, self.buffer.capacity)
    }

    #[must_use]
    pub const fn target_channels(&self) -> u16 {
        self.target_channels
    }
    #[must_use]
    pub const fn source_channels(&self) -> u16 {
        self.source_channels
    }
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    #[must_use]
    pub const fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    pub fn prefill_buffer(&mut self) -> Result<(), AppError> {
        if self.state == DecoderState::Uninitialized {
            self.initialize_decoder()?;
        }
        let target_size = (self.buffer.capacity * 95) / 100;
        let mut attempts = 0;
        while self.buffer.remaining() < target_size && attempts < 200 {
            if let Err(e) = self.fill_buffer() {
                if self.state == DecoderState::EndOfStream {
                    break;
                }
                if attempts > 10 && self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
                    return Err(
                        format!("Buffer prefill failed after {attempts} attempts: {e}").into(),
                    );
                }
            }
            attempts += 1;
        }
        if self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
            return Err(format!(
                "Buffer prefill incomplete: only {}% filled",
                (self.buffer.remaining() * 100) / self.buffer.capacity
            )
            .into());
        }
        Ok(())
    }

    pub fn seek(&mut self, time: Duration) -> Result<(), AppError> {
        let target_ts = (time.as_secs_f64() * self.sample_rate as f64) as u64;
        // format/decoder/track_id 三者在异步初始化完成前可能尚未就绪;
        // 此时走下方 else 分支优雅降级(记录目标位置),而不是报错 ——
        // seek 在初始化完成前被调用是正常场景(如切歌后立即点击歌词)。
        if let (Some(format), Some(decoder), Some(track_id)) =
            (&mut self.format, &mut self.decoder, self.track_id)
        {
            let seek_to = symphonia::core::formats::SeekTo::Timestamp {
                ts: Timestamp::new(target_ts as i64),
                track_id,
            };
            match format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to) {
                Ok(_) => {
                    // seek 成功后再更新位置、清空缓冲区、重置解码器
                    self.current_sample = target_ts;
                    self.buffer.clear();
                    decoder.reset();
                    self.state = DecoderState::Ready;
                    Ok(())
                }
                Err(e) => {
                    // seek 失败:保持原有状态(缓冲区、当前位置)不变,直接返回错误
                    Err(format!("Seek failed: {e:?}").into())
                }
            }
        } else {
            // 解码器尚未初始化:仅记录目标位置,清空缓冲区(此时缓冲区无有效数据)
            self.current_sample = target_ts;
            self.buffer.clear();
            self.state = DecoderState::Uninitialized;
            Ok(())
        }
    }

    fn initialize_decoder(&mut self) -> Result<(), AppError> {
        let file = File::open(&self.path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(
            Box::new(file.try_clone().map_err(|e| e.to_string())?),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        if let Some(ext) = Path::new(&self.path).extension().and_then(|s| s.to_str()) {
            hint.with_extension(ext);
        }
        let fmt_opts: FormatOptions = FormatOptions::default();
        let mut format = symphonia::default::get_probe()
            .probe(&hint, mss, fmt_opts, MetadataOptions::default())
            .map_err(|e| format!("Failed to probe format: {e}"))?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or("No audio track found")?;
        let track_id = track.id;
        let audio_params = track
            .codec_params
            .as_ref()
            .and_then(CodecParameters::audio)
            .ok_or("No audio codec parameters")?
            .clone();
        let mut dec_opts = AudioDecoderOptions::default();
        dec_opts.gapless = true;
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&audio_params, &dec_opts)
            .map_err(|e| format!("Failed to create decoder: {e}"))?;
        if self.current_sample > 0 {
            let seek_to = symphonia::core::formats::SeekTo::Timestamp {
                ts: Timestamp::new(self.current_sample as i64),
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

    fn fill_buffer(&mut self) -> Result<(), AppError> {
        if self.state == DecoderState::Uninitialized {
            self.initialize_decoder()?;
        }
        if matches!(
            self.state,
            DecoderState::Error(_) | DecoderState::EndOfStream
        ) {
            return Ok(());
        }
        // 解码器三件套由 initialize_decoder() 保证在 Ready/缓冲状态下均为 Some,
        // 但显式检查而非 unwrap,避免不变量被破坏时 panic
        let (format, decoder, track_id) =
            match (self.format.as_mut(), self.decoder.as_mut(), self.track_id) {
                (Some(format), Some(decoder), Some(track_id)) => (format, decoder, track_id),
                _ => return Err(AppError::msg("Decoder not initialized")),
            };
        let mut decoded_packets = 0;
        let target_fill =
            (self.buffer.capacity * TARGET_FILL_RATIO_NUMER) / TARGET_FILL_RATIO_DENOM;

        while self.buffer.remaining() < target_fill && decoded_packets < MAX_PACKETS_PER_FILL {
            let packet = match format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(Error::ResetRequired) => {
                    decoder.reset();
                    continue;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(e) => {
                    self.state = DecoderState::Error(format!("Read packet error: {e}"));
                    return Err(format!("Read packet error: {e}").into());
                }
            };
            if packet.track_id != track_id {
                continue;
            }
            match decoder.decode(&packet) {
                Ok(decoded_packet) => {
                    self.scratch_buffer.clear();
                    Self::convert_audio_buffer(
                        decoded_packet,
                        &mut self.scratch_buffer,
                        self.channel_map.as_ref(),
                        self.source_channels as usize,
                    );
                    self.buffer.append(&self.scratch_buffer);
                    decoded_packets += 1;
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    self.state = DecoderState::EndOfStream;
                    break;
                }
                Err(Error::DecodeError(_)) => {}
                Err(e) => {
                    self.state = DecoderState::Error(format!("Decode error: {e}"));
                    return Err(format!("Decode error: {e}").into());
                }
            }
        }
        Ok(())
    }

    fn convert_audio_buffer(
        audio_buf: GenericAudioBufferRef<'_>,
        samples: &mut Vec<f32>,
        channel_map: Option<&Vec<usize>>,
        src_channels: usize,
    ) {
        let frames = audio_buf.frames();
        let channels = audio_buf.spec().channels().count();

        // 如果需要混音（多声道到立体声）
        let needs_downmix = channel_map.is_some() && src_channels > 2;
        let target_ch = if needs_downmix { 2 } else { channels };
        samples.reserve(frames * target_ch);

        // 0.6 API: 先复制为交错 f32(自动做样本格式转换);
        // 多声道场景再原地复用 super::dsp::downmix_surround_to_stereo 下混,
        // 与独占模式(decode_push)共用同一实现,避免两套 5.1/7.1 系数漂移。
        audio_buf.copy_to_vec_interleaved(samples);
        if needs_downmix && channels > 2 {
            let mut write = 0usize;
            for f in 0..frames {
                let (left, right) = super::dsp::downmix_surround_to_stereo(samples, channels, f);
                samples[write] = left;
                samples[write + 1] = right;
                write += 2;
            }
            samples.truncate(write);
        }
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.buffer.is_empty() || self.buffer.needs_refill() {
            if let Err(e) = self.fill_buffer() {
                log::warn!("Buffer fill error: {e}");
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
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> std::num::NonZero<u16> {
        std::num::NonZero::new(self.target_channels).unwrap_or(std::num::NonZero::new(2).unwrap())
    }
    fn sample_rate(&self) -> std::num::NonZero<u32> {
        std::num::NonZero::new(self.sample_rate).unwrap_or(std::num::NonZero::new(48000).unwrap())
    }
    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }
}
