//! 音频解码器模块
//!
//! 使用 Symphonia 库实现高性能音频解码，支持多种格式。

use crossbeam_channel::{unbounded, Receiver};
use rodio::Source;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

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
        Self { samples: Vec::with_capacity(capacity), position: 0, capacity, sample_rate, channels, refill_threshold_ms: 100 }
    }
    #[inline(always)] fn is_empty(&self) -> bool { self.position >= self.samples.len() }
    #[inline(always)] fn next(&mut self) -> Option<f32> {
        if self.position < self.samples.len() { let s = self.samples[self.position]; self.position += 1; Some(s) } else { None }
    }
    fn clear(&mut self) { self.samples.clear(); self.position = 0; }
    fn append(&mut self, samples: &[f32]) { self.samples.extend_from_slice(samples); }
    #[inline] fn remaining(&self) -> usize { self.samples.len() - self.position }
    fn needs_refill(&self) -> bool {
        (self.remaining() as u64 * 1000) < (self.sample_rate as u64 * self.channels as u64 * self.refill_threshold_ms as u64)
    }
    fn set_refill_threshold(&mut self, threshold_ms: u32) { self.refill_threshold_ms = threshold_ms; }
}

pub struct LockFreeSymphoniaSource {
    receiver: Receiver<f32>,
    _decoder_thread: thread::JoinHandle<()>,
    stop_flag: Arc<AtomicBool>,
    cached_channels: u16,
    cached_sample_rate: u32,
    cached_total_duration: Option<Duration>,
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
}

impl LockFreeSymphoniaSource {
    pub fn new(mut decoder: SymphoniaDecoder) -> Self {
        let (channels, sample_rate, total_duration) = (decoder.target_channels(), decoder.sample_rate(), decoder.total_duration());
        let (sender, receiver) = unbounded();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = Arc::clone(&stop_flag);
        let _ = decoder.prefill_buffer();

        let decoder_thread = thread::spawn(move || {
            let mut batch = Vec::with_capacity(16384);
            loop {
                if stop_flag_clone.load(Ordering::Relaxed) { break; }
                batch.clear();
                for _ in 0..16384 { if let Some(s) = decoder.next() { batch.push(s); } else { break; } }
                if batch.is_empty() { break; }
                for s in &batch { if sender.send(*s).is_err() { break; } }
            }
        });

        Self { receiver, _decoder_thread: decoder_thread, stop_flag, cached_channels: channels, cached_sample_rate: sample_rate, cached_total_duration: total_duration, chunk_buffer: Vec::with_capacity(16384), chunk_pos: 0 }
    }
}

impl Iterator for LockFreeSymphoniaSource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.chunk_pos < self.chunk_buffer.len() { let s = self.chunk_buffer[self.chunk_pos]; self.chunk_pos += 1; return Some(s); }
        self.chunk_buffer.clear(); self.chunk_pos = 0;
        let mut count = 0;
        loop {
            match self.receiver.try_recv() {
                Ok(s) => { self.chunk_buffer.push(s); count += 1; if count >= 16384 { break; } }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    if count > 0 { break; }
                    match self.receiver.recv_timeout(Duration::from_micros(100)) { Ok(s) => { self.chunk_buffer.push(s); count += 1; } Err(_) => break }
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => break,
            }
        }
        if self.chunk_pos < self.chunk_buffer.len() { let s = self.chunk_buffer[self.chunk_pos]; self.chunk_pos += 1; Some(s) } else { None }
    }
}

impl Source for LockFreeSymphoniaSource {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.cached_channels }
    fn sample_rate(&self) -> u32 { self.cached_sample_rate }
    fn total_duration(&self) -> Option<Duration> { self.cached_total_duration }
}

impl Drop for LockFreeSymphoniaSource {
    fn drop(&mut self) { self.stop_flag.store(true, Ordering::Relaxed); }
}

pub struct SymphoniaSource {
    decoder: Arc<Mutex<SymphoniaDecoder>>,
    chunk_buffer: Vec<f32>,
    chunk_pos: usize,
    cached_channels: u16,
    cached_sample_rate: u32,
    cached_total_duration: Option<Duration>,
}

impl SymphoniaSource {
    #[must_use]
    pub fn new(decoder: SymphoniaDecoder) -> Self {
        let (channels, sample_rate, total_duration) = (decoder.target_channels(), decoder.sample_rate(), decoder.total_duration());
        Self { decoder: Arc::new(Mutex::new(decoder)), chunk_buffer: Vec::with_capacity(16384), chunk_pos: 0, cached_channels: channels, cached_sample_rate: sample_rate, cached_total_duration: total_duration }
    }
}

impl Iterator for SymphoniaSource {
    type Item = f32;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.chunk_pos < self.chunk_buffer.len() { let s = self.chunk_buffer[self.chunk_pos]; self.chunk_pos += 1; return Some(s); }
        self.chunk_buffer.clear(); self.chunk_pos = 0;
        { let mut dec = self.decoder.try_lock().unwrap_or_else(|_| self.decoder.lock().unwrap()); for _ in 0..8192 { if let Some(s) = dec.next() { self.chunk_buffer.push(s); } else { break; } } }
        if self.chunk_pos < self.chunk_buffer.len() { let s = self.chunk_buffer[self.chunk_pos]; self.chunk_pos += 1; Some(s) } else { None }
    }
}

impl Source for SymphoniaSource {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.cached_channels }
    fn sample_rate(&self) -> u32 { self.cached_sample_rate }
    fn total_duration(&self) -> Option<Duration> { self.cached_total_duration }
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
    pub fn new(path: &str) -> Result<Self, String> { Self::new_with_buffer_duration(path, None) }

    pub fn new_with_buffer_duration(path: &str, buffer_duration_ms: Option<u32>) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(file.try_clone().map_err(|e| e.to_string())?), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) { hint.with_extension(ext); }
        let mut fmt_opts: FormatOptions = Default::default();
        fmt_opts.enable_gapless = true;
        let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &MetadataOptions::default()).map_err(|e| format!("Failed to probe format: {e}"))?;
        let format = probed.format;
        let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or("No audio track found")?;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let source_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2) as u16;
        let total_duration = track.codec_params.n_frames.and_then(|n| track.codec_params.sample_rate.map(|sr| Duration::from_secs_f64(n as f64 / sr as f64)));
        let buffer_duration_ms = buffer_duration_ms.unwrap_or(if sample_rate <= 48000 { 500 } else { 400 });
        let target_channels = 2u16;
        let buffer_size = calculate_buffer_size(sample_rate, target_channels, buffer_duration_ms);
        let channel_map = Self::create_channel_mapping(source_channels);

        Ok(Self { path: path.to_string(), sample_rate, source_channels, total_duration, state: DecoderState::Uninitialized, buffer: AudioBuffer::new(buffer_size, sample_rate, target_channels), scratch_buffer: Vec::with_capacity(4096), decoder: None, format: None, track_id: None, current_sample: 0, target_channels, channel_map })
    }

    fn create_channel_mapping(channels: u16) -> Option<Vec<usize>> {
        // 返回 None 表示需要使用专门的混音算法，而不是简单的声道映射
        match channels {
            1 | 2 => None, // 单声道或立体声，不需要映射
            _ => Some(vec![]), // 多声道，标记需要混音处理
        }
    }
    
    /// 5.1/7.1 环绕声到立体声的专业混音
    /// 使用 ITU-R BS.775-1 标准的下混系数
    fn downmix_to_stereo(planes: &[&[f32]], frame: usize, src_channels: usize) -> (f32, f32) {
        // 标准声道布局:
        // 5.1: FL(0), FR(1), FC(2), LFE(3), SL/BL(4), SR/BR(5)
        // 7.1: FL(0), FR(1), FC(2), LFE(3), BL(4), BR(5), SL(6), SR(7)
        
        let fl = if src_channels > 0 { planes[0][frame] } else { 0.0 };
        let fr = if src_channels > 1 { planes[1][frame] } else { fl };
        let fc = if src_channels > 2 { planes[2][frame] } else { 0.0 };
        let _lfe = if src_channels > 3 { planes[3][frame] } else { 0.0 }; // LFE 通常不混入
        
        // 混音系数 (ITU-R BS.775-1)
        const CENTER_MIX: f32 = 0.707; // -3dB
        const SURROUND_MIX: f32 = 0.707; // -3dB
        const BACK_MIX: f32 = 0.5; // -6dB (用于 7.1)
        
        let (mut left, mut right) = (fl, fr);
        
        // 添加中置声道
        left += fc * CENTER_MIX;
        right += fc * CENTER_MIX;
        
        match src_channels {
            6 => {
                // 5.1: SL(4), SR(5)
                let sl = planes[4][frame];
                let sr = planes[5][frame];
                left += sl * SURROUND_MIX;
                right += sr * SURROUND_MIX;
            }
            8 => {
                // 7.1: BL(4), BR(5), SL(6), SR(7)
                let bl = planes[4][frame];
                let br = planes[5][frame];
                let sl = planes[6][frame];
                let sr = planes[7][frame];
                left += sl * SURROUND_MIX + bl * BACK_MIX;
                right += sr * SURROUND_MIX + br * BACK_MIX;
            }
            n if n > 2 => {
                // 其他多声道格式，简单混合额外声道
                for ch in 2..src_channels {
                    let sample = planes[ch][frame];
                    let mix = 0.5 / (src_channels - 2) as f32;
                    left += sample * mix;
                    right += sample * mix;
                }
            }
            _ => {}
        }
        
        // 归一化防止削波（5.1 最大增益约 1.414，7.1 约 1.5）
        let normalize = match src_channels {
            6 => 0.707,  // 1/sqrt(2)
            8 => 0.667,  // 约 1/1.5
            _ => 0.8,
        };
        
        (
            (left * normalize).clamp(-1.0, 1.0),
            (right * normalize).clamp(-1.0, 1.0)
        )
    }

    pub fn adjust_buffer_settings(&mut self, buffer_duration_ms: u32, refill_threshold_ms: u32) {
        let new_size = calculate_buffer_size(self.sample_rate, self.target_channels, buffer_duration_ms);
        self.buffer = AudioBuffer::new(new_size, self.sample_rate, self.target_channels);
        self.buffer.set_refill_threshold(refill_threshold_ms);
    }

    #[must_use] pub fn get_buffer_info(&self) -> (u32, u32, usize) {
        let dur = ((self.buffer.capacity as u64 * 1000) / (self.sample_rate as u64 * self.target_channels as u64)) as u32;
        (dur, self.buffer.refill_threshold_ms, self.buffer.capacity)
    }

    #[must_use] pub const fn target_channels(&self) -> u16 { self.target_channels }
    #[must_use] pub const fn source_channels(&self) -> u16 { self.source_channels }
    #[must_use] pub const fn sample_rate(&self) -> u32 { self.sample_rate }
    #[must_use] pub const fn total_duration(&self) -> Option<Duration> { self.total_duration }

    pub fn prefill_buffer(&mut self) -> Result<(), String> {
        if self.state == DecoderState::Uninitialized { self.initialize_decoder()?; }
        let target_size = (self.buffer.capacity * 95) / 100;
        let mut attempts = 0;
        while self.buffer.remaining() < target_size && attempts < 200 {
            if let Err(e) = self.fill_buffer() {
                if self.state == DecoderState::EndOfStream { break; }
                if attempts > 10 && self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
                    return Err(format!("Buffer prefill failed after {attempts} attempts: {e}"));
                }
            }
            attempts += 1;
        }
        if self.buffer.remaining() < (self.buffer.capacity * 50) / 100 {
            return Err(format!("Buffer prefill incomplete: only {}% filled", (self.buffer.remaining() * 100) / self.buffer.capacity));
        }
        Ok(())
    }

    pub fn seek(&mut self, time: Duration) -> Result<(), String> {
        let target_ts = (time.as_secs_f64() * self.sample_rate as f64) as u64;
        self.current_sample = target_ts;
        self.buffer.clear();
        if let (Some(format), Some(decoder)) = (&mut self.format, &mut self.decoder) {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp { ts: target_ts, track_id: self.track_id.unwrap() };
            match format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to) {
                Ok(_) => { decoder.reset(); self.state = DecoderState::Ready; Ok(()) }
                Err(e) => { self.current_sample = 0; self.state = DecoderState::Uninitialized; Err(format!("Seek failed: {e:?}")) }
            }
        } else { self.state = DecoderState::Uninitialized; Ok(()) }
    }

    fn initialize_decoder(&mut self) -> Result<(), String> {
        let file = File::open(&self.path).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(file.try_clone().map_err(|e| e.to_string())?), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = Path::new(&self.path).extension().and_then(|s| s.to_str()) { hint.with_extension(ext); }
        let mut fmt_opts: FormatOptions = Default::default();
        fmt_opts.enable_gapless = true;
        let probed = symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &MetadataOptions::default()).map_err(|e| format!("Failed to probe format: {e}"))?;
        let mut format = probed.format;
        let track = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or("No audio track found")?;
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|e| format!("Failed to create decoder: {e}"))?;
        if self.current_sample > 0 {
            let seek_to = symphonia::core::formats::SeekTo::TimeStamp { ts: self.current_sample, track_id };
            if format.seek(symphonia::core::formats::SeekMode::Accurate, seek_to).is_ok() { decoder.reset(); } else { self.current_sample = 0; }
        }
        self.format = Some(format); self.decoder = Some(decoder); self.track_id = Some(track_id); self.state = DecoderState::Ready;
        Ok(())
    }

    fn fill_buffer(&mut self) -> Result<(), String> {
        if self.state == DecoderState::Uninitialized { self.initialize_decoder()?; }
        if matches!(self.state, DecoderState::Error(_) | DecoderState::EndOfStream) { return Ok(()); }
        let format = self.format.as_mut().unwrap();
        let decoder = self.decoder.as_mut().unwrap();
        let track_id = self.track_id.unwrap();
        let mut decoded_packets = 0;
        let target_fill = (self.buffer.capacity * 80) / 100;

        while self.buffer.remaining() < target_fill && decoded_packets < 50 {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(Error::ResetRequired) => { decoder.reset(); continue; }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => { self.state = DecoderState::EndOfStream; break; }
                Err(e) => { self.state = DecoderState::Error(format!("Read packet error: {e}")); return Err(format!("Read packet error: {e}")); }
            };
            if packet.track_id() != track_id { continue; }
            match decoder.decode(&packet) {
                Ok(decoded) => { 
                    self.scratch_buffer.clear(); 
                    Self::convert_audio_buffer(decoded, &mut self.scratch_buffer, &self.channel_map, self.source_channels as usize); 
                    self.buffer.append(&self.scratch_buffer); 
                    decoded_packets += 1; 
                }
                Err(Error::IoError(ref e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => { self.state = DecoderState::EndOfStream; break; }
                Err(Error::DecodeError(_)) => continue,
                Err(e) => { self.state = DecoderState::Error(format!("Decode error: {e}")); return Err(format!("Decode error: {e}")); }
            }
        }
        Ok(())
    }

    fn convert_audio_buffer(audio_buf: AudioBufferRef, samples: &mut Vec<f32>, channel_map: &Option<Vec<usize>>, src_channels: usize) {
        let frames = audio_buf.frames();
        let channels = audio_buf.spec().channels.count();
        
        // 如果需要混音（多声道到立体声）
        let needs_downmix = channel_map.is_some() && src_channels > 2;
        let target_ch = if needs_downmix { 2 } else { channels };
        samples.reserve(frames * target_ch);

        match audio_buf {
            AudioBufferRef::F32(buf) => { 
                let p = buf.planes(); 
                Self::copy_planes_with_downmix(p.planes(), samples, frames, channels, needs_downmix, |s| *s); 
            }
            AudioBufferRef::S16(buf) => { 
                let p = buf.planes(); 
                Self::copy_planes_with_downmix(p.planes(), samples, frames, channels, needs_downmix, |s| (*s as f32) / 32768.0); 
            }
            AudioBufferRef::U8(buf) => { 
                let p = buf.planes(); 
                Self::copy_planes_with_downmix(p.planes(), samples, frames, channels, needs_downmix, |s| (*s as f32 - 128.0) / 128.0); 
            }
            AudioBufferRef::S32(buf) => { 
                let p = buf.planes(); 
                Self::copy_planes_with_downmix(p.planes(), samples, frames, channels, needs_downmix, |s| (*s as f32) / 2_147_483_648.0); 
            }
            AudioBufferRef::S24(buf) => { 
                let p = buf.planes(); 
                Self::copy_planes_with_downmix(p.planes(), samples, frames, channels, needs_downmix, |s| (s.inner() as f32) / 8_388_608.0); 
            }
            _ => eprintln!("Unsupported audio buffer format"),
        }
    }

    #[inline(always)]
    fn copy_planes_with_downmix<T, F>(planes: &[&[T]], out: &mut Vec<f32>, frames: usize, src_ch: usize, needs_downmix: bool, conv: F) 
    where 
        T: Copy, 
        F: Fn(&T) -> f32 
    {
        if needs_downmix && src_ch > 2 {
            // 先转换所有声道到 f32
            let mut float_planes: Vec<Vec<f32>> = Vec::with_capacity(src_ch);
            for ch in 0..src_ch {
                float_planes.push(planes[ch].iter().map(|s| conv(s)).collect());
            }
            
            // 创建引用切片
            let float_refs: Vec<&[f32]> = float_planes.iter().map(|v| v.as_slice()).collect();
            
            // 使用专业混音算法
            for i in 0..frames {
                let (left, right) = Self::downmix_to_stereo(&float_refs, i, src_ch);
                out.push(left);
                out.push(right);
            }
        } else {
            // 直接复制（单声道或立体声）
            for i in 0..frames { 
                for ch in 0..src_ch { 
                    out.push(conv(&planes[ch][i])); 
                } 
            }
        }
    }
}

impl Iterator for SymphoniaDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.buffer.is_empty() || self.buffer.needs_refill() {
            if let Err(e) = self.fill_buffer() { eprintln!("Buffer fill error: {e}"); if self.buffer.is_empty() { return None; } }
        }
        let sample = self.buffer.next();
        if sample.is_some() { self.current_sample += 1; }
        sample
    }
}

impl Source for SymphoniaDecoder {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.target_channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { self.total_duration }
}
