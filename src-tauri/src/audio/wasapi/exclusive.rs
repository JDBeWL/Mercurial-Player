//! WASAPI 独占模式音频播放实现
//!
//! 这个模块实现了真正的 WASAPI 独占模式音频输出。

#![allow(dead_code)]

use crossbeam_channel::{bounded, Receiver, Sender};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    #[must_use]
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
            audio_thread_main(command_rx, response_tx, state_clone, volume_clone, is_running_clone, sample_buffer_clone);
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

    pub fn initialize(&self, device_name: Option<&str>) -> Result<(u32, u16, String), String> {
        self.command_tx
            .send(AudioCommand::Initialize { device_name: device_name.map(String::from) })
            .map_err(|e| format!("Failed to send initialize command: {e}"))?;

        match self.response_rx.recv() {
            Ok(AudioResponse::Initialized { sample_rate, channels, device_name }) => {
                self.sample_rate.store(sample_rate, Ordering::SeqCst);
                self.channels.store(u32::from(channels), Ordering::SeqCst);
                *self.state.lock().unwrap() = PlaybackState::Stopped;
                Ok((sample_rate, channels, device_name))
            }
            Ok(AudioResponse::InitFailed(e)) => Err(e),
            Ok(other) => Err(format!("Unexpected response: {other:?}")),
            Err(e) => Err(format!("Failed to receive response: {e}")),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        self.command_tx.send(AudioCommand::Start).map_err(|e| format!("Failed to send start command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        self.command_tx.send(AudioCommand::Stop).map_err(|e| format!("Failed to send stop command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Stopped;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        self.command_tx.send(AudioCommand::Pause).map_err(|e| format!("Failed to send pause command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Paused;
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        self.command_tx.send(AudioCommand::Resume).map_err(|e| format!("Failed to send resume command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }

    pub fn set_volume(&self, vol: f32) -> Result<(), String> {
        let vol = vol.clamp(0.0, 1.0);
        *self.volume.lock().unwrap() = vol;
        self.command_tx.send(AudioCommand::SetVolume(vol)).map_err(|e| format!("Failed to send volume command: {e}"))
    }

    pub fn push_samples(&self, samples: Vec<f32>) -> Result<(), String> {
        let (buffer, cvar) = &*self.sample_buffer;
        buffer.lock().unwrap().extend(samples);
        cvar.notify_one();
        Ok(())
    }

    pub fn clear_buffer(&self) -> Result<(), String> {
        let (buffer, _) = &*self.sample_buffer;
        buffer.lock().unwrap().clear();
        Ok(())
    }

    #[must_use]
    pub fn get_state(&self) -> PlaybackState {
        *self.state.lock().unwrap()
    }

    #[must_use]
    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn get_channels(&self) -> u16 {
        self.channels.load(Ordering::SeqCst) as u16
    }

    #[must_use]
    pub fn get_volume(&self) -> f32 {
        *self.volume.lock().unwrap()
    }

    #[must_use]
    pub fn get_buffer_size(&self) -> usize {
        let (buffer, _) = &*self.sample_buffer;
        buffer.lock().unwrap().len()
    }
}

impl Default for WasapiExclusivePlayback {
    fn default() -> Self {
        Self::new()
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

fn audio_thread_main(
    command_rx: Receiver<AudioCommand>,
    response_tx: Sender<AudioResponse>,
    state: Arc<Mutex<PlaybackState>>,
    _volume: Arc<Mutex<f32>>,
    is_running: Arc<AtomicBool>,
    sample_buffer: Arc<(Mutex<VecDeque<f32>>, Condvar)>,
) {
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
        match command_rx.try_recv() {
            Ok(AudioCommand::Initialize { device_name }) => {
                handle_initialize(
                    device_name.as_deref(),
                    &response_tx,
                    &mut audio_client,
                    &mut render_client,
                    &mut event_handle,
                    &mut current_channels,
                    &mut current_bits,
                    &mut current_sample_type_is_float,
                );
            }
            Ok(AudioCommand::Start) => {
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
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
                    sample_buffer.0.lock().unwrap().clear();
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
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::SetVolume(vol)) => current_volume = vol,
            Ok(AudioCommand::ClearBuffer) => sample_buffer.0.lock().unwrap().clear(),
            Ok(AudioCommand::Shutdown) => break,
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
        }

        if is_playing {
            process_audio_output(
                &audio_client,
                &render_client,
                &event_handle,
                &sample_buffer,
                current_channels,
                current_bits,
                current_sample_type_is_float,
                current_volume,
                &mut is_playing,
                &state,
            );
        } else {
            thread::sleep(Duration::from_millis(10));
        }
    }

    if let Some(ref client) = audio_client {
        let _ = client.stop_stream();
    }

    println!("WASAPI audio thread stopped");
}

fn handle_initialize(
    device_name: Option<&str>,
    response_tx: &Sender<AudioResponse>,
    audio_client: &mut Option<wasapi::AudioClient>,
    render_client: &mut Option<wasapi::AudioRenderClient>,
    event_handle: &mut Option<wasapi::Handle>,
    current_channels: &mut u16,
    current_bits: &mut u16,
    current_sample_type_is_float: &mut bool,
) {
    match initialize_exclusive_device(device_name) {
        Ok((client, format_info)) => {
            let (sr, ch, name, bits, is_float) = format_info;
            *current_channels = ch;
            *current_bits = bits;
            *current_sample_type_is_float = is_float;

            println!("Audio format: {sr}Hz, {ch} channels, {bits} bits, float: {is_float}");

            match client.get_audiorenderclient() {
                Ok(rc) => match client.set_get_eventhandle() {
                    Ok(eh) => {
                        *render_client = Some(rc);
                        *event_handle = Some(eh);
                        *audio_client = Some(client);
                        let _ = response_tx.send(AudioResponse::Initialized {
                            sample_rate: sr,
                            channels: ch,
                            device_name: name,
                        });
                    }
                    Err(e) => {
                        let _ = response_tx.send(AudioResponse::InitFailed(format!("Failed to get event handle: {e:?}")));
                    }
                },
                Err(e) => {
                    let _ = response_tx.send(AudioResponse::InitFailed(format!("Failed to get render client: {e:?}")));
                }
            }
        }
        Err(e) => {
            let _ = response_tx.send(AudioResponse::InitFailed(e));
        }
    }
}

fn process_audio_output(
    audio_client: &Option<wasapi::AudioClient>,
    render_client: &Option<wasapi::AudioRenderClient>,
    event_handle: &Option<wasapi::Handle>,
    sample_buffer: &Arc<(Mutex<VecDeque<f32>>, Condvar)>,
    current_channels: u16,
    current_bits: u16,
    current_sample_type_is_float: bool,
    current_volume: f32,
    is_playing: &mut bool,
    state: &Arc<Mutex<PlaybackState>>,
) {
    if let (Some(client), Some(rc), Some(eh)) = (audio_client, render_client, event_handle) {
        if eh.wait_for_event(10).is_ok() {
            if let Ok(frames_available) = client.get_available_space_in_frames() {
                if frames_available > 0 {
                    let samples_needed = frames_available as usize * current_channels as usize;
                    let (buffer, _) = &**sample_buffer;
                    let mut buf = buffer.lock().unwrap();

                    let output_samples: Vec<f32> = (0..samples_needed)
                        .map(|_| buf.pop_front().unwrap_or(0.0) * current_volume)
                        .collect();

                    drop(buf);

                    let output_bytes = convert_samples_to_bytes(&output_samples, current_bits, current_sample_type_is_float);

                    if rc.write_to_device(frames_available as usize, &output_bytes, None).is_err() {
                        *is_playing = false;
                        *state.lock().unwrap() = PlaybackState::Stopped;
                    }
                }
            }
        }
    }
}

fn initialize_exclusive_device(device_name: Option<&str>) -> Result<(wasapi::AudioClient, (u32, u16, String, u16, bool)), String> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat};

    let enumerator = DeviceEnumerator::new().map_err(|e| format!("Failed to create device enumerator: {e:?}"))?;

    let device = if let Some(name) = device_name {
        let collection = enumerator.get_device_collection(&Direction::Render).map_err(|e| format!("Failed to get device collection: {e:?}"))?;
        collection.into_iter().flatten().find(|device| device.get_friendlyname().is_ok_and(|n| n == name)).ok_or_else(|| format!("Device not found: {name}"))?
    } else {
        enumerator.get_default_device(&Direction::Render).map_err(|e| format!("Failed to get default device: {e:?}"))?
    };

    let device_name = device.get_friendlyname().unwrap_or_else(|_| "Unknown".to_string());
    let mut audio_client = device.get_iaudioclient().map_err(|e| format!("Failed to get audio client: {e:?}"))?;

    let default_format = audio_client.get_mixformat().map_err(|e| format!("Failed to get mix format: {e:?}"))?;
    let default_sample_rate = default_format.get_samplespersec() as usize;
    let default_channels = default_format.get_nchannels() as usize;

    println!("Device default format: {default_sample_rate}Hz, {default_channels} channels");

    let sample_rates_to_try: [usize; 12] = [default_sample_rate, 384000, 352800, 192000, 176400, 96000, 88200, 48000, 44100, 32000, 22050, 16000];
    let bit_depths: [(usize, bool); 4] = [(32, true), (32, false), (24, false), (16, false)];
    let channels_to_try: [usize; 2] = [default_channels, 2];

    let mut found_format = None;

    'outer: for &sample_rate in &sample_rates_to_try {
        for &channels in &channels_to_try {
            for &(bits, is_float) in &bit_depths {
                let sample_type = if is_float { SampleType::Float } else { SampleType::Int };
                let wave_format = WaveFormat::new(bits, bits, &sample_type, sample_rate, channels, None);

                if audio_client.is_supported(&wave_format, &ShareMode::Exclusive).is_ok() {
                    found_format = Some((wave_format, sample_rate as u32, channels as u16, bits as u16, is_float));
                    break 'outer;
                }
            }
        }
    }

    let (wave_format, sample_rate, channels, bits, is_float) = found_format.ok_or_else(|| "No supported exclusive format found".to_string())?;

    let (_default_period, min_period) = audio_client.get_device_period().map_err(|e| format!("Failed to get device period: {e:?}"))?;

    let stream_mode = StreamMode::EventsExclusive { period_hns: min_period };

    audio_client.initialize_client(&wave_format, &Direction::Render, &stream_mode).map_err(|e| format!("Failed to initialize exclusive mode: {e:?}"))?;

    println!("WASAPI Exclusive Mode initialized: {device_name} @ {sample_rate}Hz, {channels} channels, {bits} bits, float: {is_float}");

    Ok((audio_client, (sample_rate, channels, device_name, bits, is_float)))
}

fn convert_samples_to_bytes(samples: &[f32], bits: u16, is_float: bool) -> Vec<u8> {
    match (bits, is_float) {
        (32, true) => samples.iter().flat_map(|&s| s.to_le_bytes()).collect(),
        (32, false) => samples.iter().flat_map(|&s| ((s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32).to_le_bytes()).collect(),
        (24, _) => samples.iter().flat_map(|&s| {
            let int_val = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
            let bytes = int_val.to_le_bytes();
            [bytes[0], bytes[1], bytes[2]]
        }).collect(),
        (16, _) => samples.iter().flat_map(|&s| ((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes()).collect(),
        _ => samples.iter().flat_map(|&s| s.to_le_bytes()).collect(),
    }
}
