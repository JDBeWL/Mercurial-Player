//! WASAPI独占模式音频播放实现
//!
//! 这个模块实现了WASAPI独占模式音频输出。

#![allow(dead_code)]

use crossbeam_channel::{bounded, Receiver, Sender};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// 音频线程命令
pub enum AudioCommand {
    Initialize { device_name: Option<String> },
    Start,
    Stop,
    Pause,
    Resume,
    SetVolume(f32),
    ClearBuffer,
    Shutdown,
    /// 带淡出的停止(用于切歌/退出场景)
    /// 参数:淡出时长(毫秒)
    StopWithFadeOut { duration_ms: u32 },
    /// 带淡出的暂停(用于用户暂停)
    /// 参数:淡出时长(毫秒)
    PauseWithFadeOut { duration_ms: u32 },
    /// 带淡入的恢复(用于用户恢复)
    /// 参数:淡入时长(毫秒)
    ResumeWithFadeIn { duration_ms: u32 },
}

impl std::fmt::Debug for AudioCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialize { device_name } => f.debug_struct("Initialize").field("device_name", device_name).finish(),
            Self::Start => write!(f, "Start"),
            Self::Stop => write!(f, "Stop"),
            Self::Pause => write!(f, "Pause"),
            Self::Resume => write!(f, "Resume"),
            Self::SetVolume(arg0) => f.debug_tuple("SetVolume").field(arg0).finish(),
            Self::ClearBuffer => write!(f, "ClearBuffer"),
            Self::Shutdown => write!(f, "Shutdown"),
            Self::StopWithFadeOut { duration_ms } => f.debug_struct("StopWithFadeOut").field("duration_ms", duration_ms).finish(),
            Self::PauseWithFadeOut { duration_ms } => f.debug_struct("PauseWithFadeOut").field("duration_ms", duration_ms).finish(),
            Self::ResumeWithFadeIn { duration_ms } => f.debug_struct("ResumeWithFadeIn").field("duration_ms", duration_ms).finish(),
        }
    }
}

/// 淡入淡出状态机(音频线程内部维护,不阻塞主线程)
#[derive(Debug, Clone, Copy)]
enum FadeState {
    /// 无淡入淡出,fade_factor = 1.0
    Idle,
    /// 正在淡出
    /// target_factor: 目标系数(通常为 0.0)
    /// remaining_frames: 剩余帧数
    /// total_frames: 总帧数(用于计算进度)
    /// on_complete: 淡出完成后的动作
    FadingOut {
        target_factor: f32,
        remaining_frames: usize,
        total_frames: usize,
        on_complete: FadeAction,
    },
    /// 正在淡入
    /// target_factor: 目标系数(通常为 1.0)
    FadingIn {
        target_factor: f32,
        remaining_frames: usize,
        total_frames: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeAction {
    /// 淡出后无动作(继续播放)
    None,
    /// 淡出后暂停(stop_stream)
    Pause,
    /// 淡出后停止(stop_stream + clear_buffer)
    Stop,
}

/// 音频线程响应
#[derive(Debug)]
pub enum AudioResponse {
    Initialized { sample_rate: u32, channels: u16, device_name: String },
    InitFailed(String),
    Ok,
    Error(String),
}

/// WASAPI独占模式播放器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Uninitialized,
    Stopped,
    Playing,
    Paused,
    /// 带淡出的停止中:已发送 StopWithFadeOut 命令,音频线程仍在淡出,完成后转为 Stopped
    Stopping,
    /// 带淡出的暂停中:已发送 PauseWithFadeOut 命令,音频线程仍在淡出,完成后转为 Paused
    Pausing,
}

/// WASAPI独占模式播放器
pub struct WasapiExclusivePlayback {
    command_tx: Sender<AudioCommand>,
    response_rx: Receiver<AudioResponse>,
    audio_thread: Option<JoinHandle<()>>,
    state: Arc<Mutex<PlaybackState>>,
    sample_rate: Arc<AtomicU32>,
    channels: AtomicU32,
    volume: Arc<Mutex<f32>>,
    is_running: Arc<AtomicBool>,
    sample_buffer: Arc<(Mutex<VecDeque<f32>>, Condvar)>,
    /// 已写入硬件的采样数（用于计算播放位置）
    samples_written: Arc<AtomicU64>,
}

/// 根据采样率和声道数计算 WASAPI 缓冲区容量
/// 目标是保持约4秒的缓冲（48000 * 2 * 4 @48kHz stereo）
#[must_use]
fn calculate_wasapi_buffer_capacity(sample_rate: u32, channels: u16) -> usize {
    // 基准：48kHz立体声使用4秒缓冲
    // 高采样率需要更大缓冲以保持相同时长
    let duration_secs = 4;
    sample_rate as usize * channels as usize * duration_secs
}

impl WasapiExclusivePlayback {
    #[must_use]
    pub fn new() -> Self {
        let (command_tx, command_rx) = bounded::<AudioCommand>(64);
        let (response_tx, response_rx) = bounded::<AudioResponse>(64);

        let state = Arc::new(Mutex::new(PlaybackState::Uninitialized));
        let volume = Arc::new(Mutex::new(1.0f32));
        let is_running = Arc::new(AtomicBool::new(true));
        let samples_written = Arc::new(AtomicU64::new(0));
        // 初始使用默认48kHz立体声的缓冲区大小，初始化后会调整
        let initial_capacity = calculate_wasapi_buffer_capacity(48000, 2);
        let sample_buffer = Arc::new((Mutex::new(VecDeque::with_capacity(initial_capacity)), Condvar::new()));
        let sample_rate = Arc::new(AtomicU32::new(48000));

        let state_clone = Arc::clone(&state);
        let volume_clone = Arc::clone(&volume);
        let is_running_clone = Arc::clone(&is_running);
        let sample_buffer_clone = Arc::clone(&sample_buffer);
        let samples_written_clone = Arc::clone(&samples_written);
        let sample_rate_clone = Arc::clone(&sample_rate);

        let audio_thread = thread::spawn(move || {
            audio_thread_main(command_rx, response_tx, state_clone, volume_clone, is_running_clone, sample_buffer_clone, samples_written_clone, sample_rate_clone);
        });

        Self {
            command_tx,
            response_rx,
            audio_thread: Some(audio_thread),
            state,
            sample_rate,
            channels: AtomicU32::new(2),
            volume,
            is_running,
            sample_buffer,
            samples_written,
        }
    }

    pub fn initialize(&self, device_name: Option<&str>) -> Result<(u32, u16, String), String> {
        self.command_tx
            .send(AudioCommand::Initialize { device_name: device_name.map(String::from) })
            .map_err(|e| format!("Failed to send initialize command: {e}"))?;

        // 使用超时接收响应，防止无限等待
        match self.response_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(AudioResponse::Initialized { sample_rate, channels, device_name }) => {
                self.sample_rate.store(sample_rate, Ordering::SeqCst);
                self.channels.store(u32::from(channels), Ordering::SeqCst);
                *self.state.lock().unwrap() = PlaybackState::Stopped;
                
                // 根据实际采样率和声道数调整缓冲区容量
                let new_capacity = calculate_wasapi_buffer_capacity(sample_rate, channels);
                let (buffer, _) = &*self.sample_buffer;
                let mut buf = buffer.lock().unwrap();
                // 如果新容量更大，预留更多空间
                let current_capacity = buf.capacity();
                if new_capacity > current_capacity {
                    buf.reserve(new_capacity - current_capacity);
                }
                drop(buf);
                
                Ok((sample_rate, channels, device_name))
            }
            Ok(AudioResponse::InitFailed(e)) => Err(e),
            Ok(other) => Err(format!("Unexpected response: {other:?}")),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                log::warn!("WASAPI initialize timed out, cleaning up stale responses");
                // 清空可能残留的过时响应,避免影响后续命令
                while self.response_rx.try_recv().is_ok() {}
                // 超时后设备未成功初始化,重置状态为 Uninitialized
                *self.state.lock().unwrap() = PlaybackState::Uninitialized;
                Err("Device initialization timeout - device may be in use or unavailable".to_string())
            }
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

    /// 带淡出的停止(用于切歌/退出)
    /// 主线程发送命令后立即返回,音频线程内部完成淡出再 stop_stream
    #[allow(dead_code)]
    pub fn stop_with_fade_out(&self, duration_ms: u32) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::StopWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send stop_with_fade_out command: {e}"))?;
        // 状态标记为 Stopping,表示音频线程仍在淡出;淡出完成后由 FadeAction::Stop 转为 Stopped
        *self.state.lock().unwrap() = PlaybackState::Stopping;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        // 默认 30ms 淡出消除 audible click
        self.pause_with_fade_out(30)
    }

    /// 不带淡出的暂停(用于用户禁用淡入淡出的场景)
    pub fn pause_no_fade(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Pause)
            .map_err(|e| format!("Failed to send pause command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Paused;
        Ok(())
    }

    /// 带淡出的暂停(默认用于用户暂停)
    pub fn pause_with_fade_out(&self, duration_ms: u32) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::PauseWithFadeOut { duration_ms })
            .map_err(|e| format!("Failed to send pause_with_fade_out command: {e}"))?;
        // 状态标记为 Pausing,表示音频线程仍在淡出;淡出完成后由 FadeAction::Pause 转为 Paused
        *self.state.lock().unwrap() = PlaybackState::Pausing;
        Ok(())
    }

    pub fn resume(&self) -> Result<(), String> {
        // 默认 30ms 淡入消除 audible click
        self.resume_with_fade_in(30)
    }

    /// 不带淡入的恢复(用于用户禁用淡入淡出的场景)
    pub fn resume_no_fade(&self) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::Resume)
            .map_err(|e| format!("Failed to send resume command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }

    /// 带淡入的恢复(默认用于用户恢复)
    pub fn resume_with_fade_in(&self, duration_ms: u32) -> Result<(), String> {
        self.command_tx
            .send(AudioCommand::ResumeWithFadeIn { duration_ms })
            .map_err(|e| format!("Failed to send resume_with_fade_in command: {e}"))?;
        *self.state.lock().unwrap() = PlaybackState::Playing;
        Ok(())
    }

    pub fn set_volume(&self, vol: f32) -> Result<(), String> {
        let vol = vol.clamp(0.0, 1.0);
        *self.volume.lock().unwrap() = vol;
        self.command_tx.send(AudioCommand::SetVolume(vol)).map_err(|e| format!("Failed to send volume command: {e}"))
    }

    pub fn push_samples(&self, samples: &[f32]) -> Result<(), String> {
        let (buffer, cvar) = &*self.sample_buffer;
        let mut buf = buffer.lock().unwrap();
        // 预先 reserve 避免多次扩容(VecDeque 的 extend 走迭代器路径无 slice 特化)
        buf.reserve(samples.len());
        // 使用 extend_from_slice 的等价:VecDeque 在 stable 上无该 API,手动 push_back
        // 但 Iter::copied 的 Extend 实现已优化为 memcpy,保留以避免循环开销
        buf.extend(samples.iter().copied());
        drop(buf);
        cvar.notify_one();
        Ok(())
    }

    /// 等待缓冲区水位低于 max_samples。
    /// 返回 true 表示有空间可以 push,false 表示超时或被唤醒时仍满。
    /// WASAPI 消费线程每次消费后会 notify_one,因此生产者会被及时唤醒。
    pub fn wait_for_buffer_space(&self, max_samples: usize, timeout: Duration) -> bool {
        let (buffer, cvar) = &*self.sample_buffer;
        let guard = buffer.lock().unwrap();
        if guard.len() < max_samples {
            return true;
        }
        // 已满时等待,直到条件为 false (有空间) 或超时
        let (guard, _) = cvar
            .wait_timeout_while(guard, timeout, |buf| buf.len() >= max_samples)
            .unwrap_or_else(|e| e.into_inner());
        guard.len() < max_samples
    }

    pub fn clear_buffer(&self) -> Result<(), String> {
        let (buffer, _) = &*self.sample_buffer;
        buffer.lock().unwrap().clear();
        self.samples_written.store(0, Ordering::SeqCst);
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

    /// 获取已写入硬件的采样数
    #[must_use]
    pub fn get_samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::SeqCst)
    }

    /// 重置已写入采样计数器
    pub fn reset_samples_written(&self) {
        self.samples_written.store(0, Ordering::SeqCst);
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
    samples_written: Arc<AtomicU64>,
    sample_rate_atomic: Arc<AtomicU32>,
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
    // 淡入淡出状态(音频线程内部维护,不阻塞主线程)
    // fade_factor 是当前实际应用到采样的系数(0.0..=1.0)
    let mut fade_state = FadeState::Idle;
    let mut fade_factor: f32 = 1.0;

    // 复用缓冲区:避免每次 WASAPI 回调都堆分配(每秒~100次)
    // 容量按 48kHz/10ms/立体声 ≈ 960 samples 估算,预分配 4096 避免初次扩容
    let mut reusable_samples: Vec<f32> = Vec::with_capacity(4096);
    let mut reusable_bytes: Vec<u8> = Vec::with_capacity(4096 * 4);

    log::info!("WASAPI audio thread started");

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
                        fade_state = FadeState::Idle;
                        fade_factor = 1.0;
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::Stop) => {
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    fade_state = FadeState::Idle;
                    fade_factor = 1.0;
                    *state.lock().unwrap() = PlaybackState::Stopped;
                    sample_buffer.0.lock().unwrap().clear();
                }
            }
            Ok(AudioCommand::Pause) => {
                // 兼容接口:不带淡出的暂停(仅用于内部需要立即暂停的场景)
                if let Some(ref client) = audio_client {
                    let _ = client.stop_stream();
                    is_playing = false;
                    fade_state = FadeState::Idle;
                    fade_factor = 1.0;
                    *state.lock().unwrap() = PlaybackState::Paused;
                }
            }
            Ok(AudioCommand::Resume) => {
                // 兼容接口:不带淡入的恢复
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        fade_state = FadeState::Idle;
                        fade_factor = 1.0;
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Ok(AudioCommand::SetVolume(vol)) => {
                current_volume = vol;
                // 注意：wasapi crate的AudioClient没有直接的音量控制方法
                // 音量在process_audio_output中通过软件乘法应用
            }
            Ok(AudioCommand::ClearBuffer) => sample_buffer.0.lock().unwrap().clear(),
            Ok(AudioCommand::Shutdown) => break,
            Ok(AudioCommand::StopWithFadeOut { duration_ms }) => {
                // 切歌/退出场景:启动淡出,完成后 stop_stream + clear_buffer
                let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                let frames = (sr * duration_ms / 1000).max(1) as usize;
                fade_state = FadeState::FadingOut {
                    target_factor: 0.0,
                    remaining_frames: frames,
                    total_frames: frames,
                    on_complete: FadeAction::Stop,
                };
                // 不立即 is_playing = false,让淡出继续播放
            }
            Ok(AudioCommand::PauseWithFadeOut { duration_ms }) => {
                // 用户暂停:启动淡出,完成后 stop_stream(保留缓冲区,不 clear)
                let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                let frames = (sr * duration_ms / 1000).max(1) as usize;
                fade_state = FadeState::FadingOut {
                    target_factor: 0.0,
                    remaining_frames: frames,
                    total_frames: frames,
                    on_complete: FadeAction::Pause,
                };
            }
            Ok(AudioCommand::ResumeWithFadeIn { duration_ms }) => {
                // 用户恢复:start_stream 后立即启动淡入(从 0 渐升到 1.0)
                if let Some(ref client) = audio_client {
                    if client.start_stream().is_ok() {
                        is_playing = true;
                        fade_factor = 0.0;
                        let sr = sample_rate_atomic.load(Ordering::Relaxed).max(1);
                        let frames = (sr * duration_ms / 1000).max(1) as usize;
                        fade_state = FadeState::FadingIn {
                            target_factor: 1.0,
                            remaining_frames: frames,
                            total_frames: frames,
                        };
                        *state.lock().unwrap() = PlaybackState::Playing;
                    }
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => break,
        }

        // 更新淡入淡出状态(每帧推进一次)
        match fade_state {
            FadeState::Idle => {}
            FadeState::FadingOut { target_factor, ref mut remaining_frames, total_frames, on_complete } => {
                if *remaining_frames > 0 {
                    let progress = 1.0 - (*remaining_frames as f32 / total_frames as f32);
                    fade_factor = 1.0 - progress * (1.0 - target_factor);
                }
                if *remaining_frames == 0 {
                    fade_factor = target_factor;
                    let action = on_complete;
                    fade_state = FadeState::Idle;
                    // 执行淡出后的动作
                    match action {
                        FadeAction::None => {}
                        FadeAction::Pause => {
                            if let Some(ref client) = audio_client {
                                let _ = client.stop_stream();
                                is_playing = false;
                                // 不重置 fade_factor,resume 时从 0 渐升
                                *state.lock().unwrap() = PlaybackState::Paused;
                            }
                        }
                        FadeAction::Stop => {
                            if let Some(ref client) = audio_client {
                                let _ = client.stop_stream();
                                is_playing = false;
                                sample_buffer.0.lock().unwrap().clear();
                            }
                            fade_factor = 1.0;  // 重置为 1.0,准备下一次播放
                            *state.lock().unwrap() = PlaybackState::Stopped;
                        }
                    }
                }
            }
            FadeState::FadingIn { target_factor, ref mut remaining_frames, total_frames } => {
                if *remaining_frames > 0 {
                    let progress = 1.0 - (*remaining_frames as f32 / total_frames as f32);
                    fade_factor = progress * target_factor;
                }
                if *remaining_frames == 0 {
                    fade_factor = target_factor;
                    fade_state = FadeState::Idle;
                }
            }
        }

        if is_playing {
            // 应用 fade_factor:实际音量 = current_volume * fade_factor
            let effective_volume = current_volume * fade_factor;
            let frames_processed = process_audio_output(
                audio_client.as_ref(),
                render_client.as_ref(),
                event_handle.as_ref(),
                &sample_buffer,
                current_channels,
                current_bits,
                current_sample_type_is_float,
                effective_volume,
                &mut is_playing,
                &state,
                &samples_written,
                &mut reusable_samples,
                &mut reusable_bytes,
            );
            // 按实际处理的帧数推进淡入淡出剩余帧计数
            if frames_processed > 0 {
                match &mut fade_state {
                    FadeState::FadingOut { remaining_frames, .. }
                    | FadeState::FadingIn { remaining_frames, .. } => {
                        *remaining_frames = remaining_frames.saturating_sub(frames_processed);
                    }
                    FadeState::Idle => {}
                }
            }
        } else {
            thread::sleep(Duration::from_millis(10));
        }
    }

    if let Some(ref client) = audio_client {
        let _ = client.stop_stream();
    }

    log::info!("WASAPI audio thread stopped");
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

            log::info!("Audio format: {sr}Hz, {ch} channels, {bits} bits, float: {is_float}");

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
    audio_client: Option<&wasapi::AudioClient>,
    render_client: Option<&wasapi::AudioRenderClient>,
    event_handle: Option<&wasapi::Handle>,
    sample_buffer: &Arc<(Mutex<VecDeque<f32>>, Condvar)>,
    current_channels: u16,
    current_bits: u16,
    current_sample_type_is_float: bool,
    current_volume: f32,
    is_playing: &mut bool,
    state: &Arc<Mutex<PlaybackState>>,
    samples_written: &Arc<AtomicU64>,
    reusable_samples: &mut Vec<f32>,
    reusable_bytes: &mut Vec<u8>,
) -> usize {
    if let (Some(client), Some(rc), Some(eh)) = (audio_client, render_client, event_handle) {
        // 使用更短的超时以获得更低延迟
        if eh.wait_for_event(5).is_ok() {
            if let Ok(frames_available) = client.get_available_space_in_frames() {
                if frames_available > 0 {
                    let samples_needed = frames_available as usize * current_channels as usize;
                    let (buffer, cvar) = &**sample_buffer;

                    // 检测缓冲区欠载 - 在锁内批量取走,缩短锁持有时间
                    let mut underrun_count = 0;
                    reusable_samples.clear();
                    reusable_samples.reserve(samples_needed);

                    {
                        let mut buf = buffer.lock().unwrap();
                        for _ in 0..samples_needed {
                            let sample = buf.pop_front().unwrap_or_else(|| {
                                underrun_count += 1;
                                0.0
                            });
                            // 音量乘法放到锁外做以缩短锁持有时间
                            reusable_samples.push(sample);
                        }
                    }

                    // 释放锁后做音量乘法(无锁争用)
                    // 注: 乘 1.0 的开销可忽略,无需特判
                    for s in reusable_samples.iter_mut() {
                        *s *= current_volume;
                    }

                    // 记录欠载情况
                    if underrun_count > samples_needed / 2 {
                        log::warn!("WASAPI buffer underrun: {underrun_count}/{samples_needed} samples");
                    }

                    // 通知等待的生产者 (有空间了)
                    cvar.notify_one();

                    // 复用 Vec<u8> 缓冲区
                    convert_samples_to_bytes_into(reusable_samples, current_bits, current_sample_type_is_float, reusable_bytes);

                    if rc.write_to_device(frames_available as usize, reusable_bytes, None).is_ok() {
                        // 更新已写入硬件的采样数
                        samples_written.fetch_add((frames_available as usize * current_channels as usize) as u64, Ordering::SeqCst);
                        return frames_available as usize;
                    }
                    *is_playing = false;
                    *state.lock().unwrap() = PlaybackState::Stopped;
                }
            }
        }
    }
    0
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

    log::info!("Device default format: {default_sample_rate}Hz, {default_channels} channels");

    let sample_rates_to_try: [usize; 12] = [default_sample_rate, 384_000, 352_800, 192_000, 176_400, 96_000, 88_200, 48_000, 44_100, 32_000, 22_050, 16_000];
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

    // 尝试初始化独占模式，添加重试机制
    let mut last_error = None;
    for attempt in 1..=3 {
        match audio_client.initialize_client(&wave_format, &Direction::Render, &stream_mode) {
            Ok(()) => {
                log::info!("WASAPI Exclusive Mode initialized: {device_name} @ {sample_rate}Hz, {channels} channels, {bits} bits, float: {is_float}");
                return Ok((audio_client, (sample_rate, channels, device_name, bits, is_float)));
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < 3 {
                    log::warn!("Exclusive mode initialization attempt {attempt} failed, retrying...");
                    thread::sleep(Duration::from_millis(100 * attempt as u64));
                    
                    // 重新获取audio client
                    audio_client = device.get_iaudioclient().map_err(|e| format!("Failed to get audio client on retry: {e:?}"))?;
                }
            }
        }
    }

    Err(format!(
        "Failed to initialize exclusive mode after 3 attempts: {last_error:?}. The device may be in use by another application or does not support exclusive mode."
    ))
}

/// SIMD 加速的样本转换模块
///
/// 使用 AVX2/FMA + SSE2 intrinsics 加速 f32 → i16/i32 字节转换
/// 因为 SIMD intrinsics 必须 unsafe,这里统一在模块级别 allow unsafe_code
/// 该 allow 仅限此模块,不影响其他代码的 unsafe_code 审查
#[allow(unsafe_code)]
mod simd_convert {
    /// 将 f32 采样转换为指定格式的字节,写入复用 buffer(零分配)
    pub fn convert_samples_to_bytes_into(samples: &[f32], bits: u16, is_float: bool, out: &mut Vec<u8>) {
    // 预先计算所需容量,避免多次扩容
    let bytes_per_sample = match bits {
        16 => 2,
        24 => 3,
        32 => 4,
        _ => 4,
    };
    out.clear();
    out.reserve(samples.len() * bytes_per_sample);

    match (bits, is_float) {
        (32, true) => {
            // 32-bit float: f32 的内存表示即 LE 字节,可整块 memcpy
            // 安全性: f32 与 [u8; 4] 都是 POD,size 一致(f32 恒为 4 字节)
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    samples.as_ptr().cast::<u8>(),
                    samples.len() * 4,
                )
            };
            out.extend_from_slice(bytes);
        }
        (32, false) => {
            // f32 → i32 (AVX2 加速,无 AVX2 时回落 SSE2)
            #[cfg(target_arch = "x86_64")]
            {
                if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                    unsafe { f32_to_i32_bytes_avx2(samples, out) };
                    return;
                }
                // SSE2 是 x86_64 baseline,所有 64 位 Intel/AMD CPU 必然支持
                unsafe { f32_to_i32_bytes_sse2(samples, out) };
                return;
            }
            #[allow(unreachable_code)]
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
                out.extend_from_slice(&int_val.to_le_bytes());
            }
        }
        (24, _) => {
            // 24-bit: 字节打包(取 i32 低 3 字节)难以 SIMD 化,暂用标量
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                let bytes = int_val.to_le_bytes();
                out.extend_from_slice(&bytes[0..3]);
            }
        }
        (16, _) => {
            // f32 → i16 (AVX2 加速,无 AVX2 时回落 SSE2)
            #[cfg(target_arch = "x86_64")]
            {
                if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                    unsafe { f32_to_i16_bytes_avx2(samples, out) };
                    return;
                }
                unsafe { f32_to_i16_bytes_sse2(samples, out) };
                return;
            }
            #[allow(unreachable_code)]
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                out.extend_from_slice(&int_val.to_le_bytes());
            }
        }
        _ => {
            // 兜底:按 32-bit float 处理
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    samples.as_ptr().cast::<u8>(),
                    samples.len() * 4,
                )
            };
            out.extend_from_slice(bytes);
        }
    }
}

// ============================================================
// SIMD 优化: f32 → i16/i32 字节流
// ============================================================
// 三层分发架构(运行时由 is_x86_feature_detected! 选择):
// 1. AVX2 + FMA path (Haswell 2013+ / Zen 2017+) - 一次 8/16 个 f32,最快
// 2. SSE2 path (所有 x86_64 CPU,含老至强) - 一次 4/8 个 f32,中等加速
// 3. 标量 fallback (非 x86_64 平台) - 逐样本循环
//
// 关键技术细节:
// - _mm_cvtps_epi32 / _mm256_cvtps_epi32 在输入超过 i32 范围时返回 0x80000000
//   (saturation indefinite),故 i32 路径必须先 clamp 到 2147483520.0
//   (小于 2^31 的最大 f32 = 2^31 - 128)
// - _mm256_packs_epi32 存在 256-bit lane 交错,需 _mm256_permute4x64_epi64(0xD8) 修复
//   SSE2 的 _mm_packs_epi32 只有 128-bit 单 lane,无交错问题
// - 舍入模式: Rust `as i32/i16` 是 truncation-toward-zero,
//   MXCSR 默认 RNE (round to nearest even),在 .5 边界处差 1 LSB

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)] // 整个函数由 target_feature 限定为 unsafe,内联 unsafe 块冗余
#[allow(clippy::wildcard_imports)] // SIMD intrinsics 数量多,逐个导入冗长
pub(super) unsafe fn f32_to_i16_bytes_avx2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm256_set1_ps(1.0);
    let neg_one = _mm256_set1_ps(-1.0);
    let scale = _mm256_set1_ps(i16::MAX as f32); // 32767.0

    let mut i = 0;
    let chunk = 16; // 16 个 f32 → 32 bytes (i16)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        let b = _mm256_loadu_ps(samples.as_ptr().add(i + 8));

        // clamp(-1, 1) * scale
        let a = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, a)), scale);
        let b = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, b)), scale);

        // f32 → i32 (round to nearest even)
        let a_i32 = _mm256_cvtps_epi32(a);
        let b_i32 = _mm256_cvtps_epi32(b);

        // pack i32 → i16 (saturating) + 修复 lane 交错
        // packs_epi32(a, b) 输出顺序: [a0..3, b0..3, a4..7, b4..7]
        // permute 0xD8 (= 0b11_01_10_00) 重排为: [a0..7, b0..7]
        let packed = _mm256_packs_epi32(a_i32, b_i32);
        let packed = _mm256_permute4x64_epi64(packed, 0xD8);

        // __m256i → [u8; 32]: 同 size(32B) POD 转换
        let bytes: [u8; 32] = core::mem::transmute(packed);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    // 处理剩余尾部
    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i32_bytes_avx2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm256_set1_ps(1.0);
    let neg_one = _mm256_set1_ps(-1.0);
    let scale = _mm256_set1_ps(i32::MAX as f32); // 2147483648.0 (f32 精度损失)
    // 2147483648.0 触发 cvtps_epi32 的 saturation indefinite(返回 0x80000000)
    // 故 clamp 到 2147483520.0(小于 2^31 的最大 f32 = 2^31 - 128)
    let max_safe = _mm256_set1_ps(2_147_483_520.0);
    let min_safe = _mm256_set1_ps(-2_147_483_648.0);

    let mut i = 0;
    let chunk = 8; // 8 个 f32 → 32 bytes (i32)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        // clamp(-1, 1) * scale,再 clamp 到 i32 安全范围
        let a = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, a)), scale);
        let a = _mm256_max_ps(min_safe, _mm256_min_ps(max_safe, a));
        let a_i32 = _mm256_cvtps_epi32(a);

        let bytes: [u8; 32] = core::mem::transmute(a_i32);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

// ============================================================
// SSE2 path: 所有 x86_64 CPU 的兜底加速(baseline feature)
// 一次处理 4/8 个 f32,适用范围: 老 Xeon(Nehalem/Westmere/Sandy/Ivy Bridge)等
// ============================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i16_bytes_sse2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm_set1_ps(1.0);
    let neg_one = _mm_set1_ps(-1.0);
    let scale = _mm_set1_ps(i16::MAX as f32);

    let mut i = 0;
    let chunk = 8; // 8 个 f32 → 16 bytes (i16)

    while i + chunk <= samples.len() {
        let a = _mm_loadu_ps(samples.as_ptr().add(i));
        let b = _mm_loadu_ps(samples.as_ptr().add(i + 4));

        let a = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, a)), scale);
        let b = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, b)), scale);

        let a_i32 = _mm_cvtps_epi32(a);
        let b_i32 = _mm_cvtps_epi32(b);

        // SSE2 packs_epi32 输出顺序: [a0..3, b0..3] - 单 lane 无交错
        let packed = _mm_packs_epi32(a_i32, b_i32);
        let bytes: [u8; 16] = core::mem::transmute(packed);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i32_bytes_sse2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm_set1_ps(1.0);
    let neg_one = _mm_set1_ps(-1.0);
    let scale = _mm_set1_ps(i32::MAX as f32);
    let max_safe = _mm_set1_ps(2_147_483_520.0);
    let min_safe = _mm_set1_ps(-2_147_483_648.0);

    let mut i = 0;
    let chunk = 4; // 4 个 f32 → 16 bytes (i32)

    while i + chunk <= samples.len() {
        let a = _mm_loadu_ps(samples.as_ptr().add(i));
        let a = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, a)), scale);
        let a = _mm_max_ps(min_safe, _mm_min_ps(max_safe, a));
        let a_i32 = _mm_cvtps_epi32(a);

        let bytes: [u8; 16] = core::mem::transmute(a_i32);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

// 模块关闭:使用 simd_convert::convert_samples_to_bytes_into 访问
}

pub use simd_convert::convert_samples_to_bytes_into;

#[cfg(test)]
mod simd_tests {
    use super::*;
    use super::simd_convert::*;

    /// 验证 AVX2 路径与标量路径产生相同字节流(允许 i32 路径 1 LSB 差异)
    /// 样本总数对齐到 16 的倍数,确保 SSE2(chunk=8) 和 AVX2(chunk=16)
    /// 的 SIMD path 都不进入尾部标量循环,从而可以精确对比两条 SIMD path
    fn make_test_samples(n: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / n as f32;
            // 覆盖 [-1, 1] 全范围,含边界
            samples.push((t * 2.0 - 1.0).clamp(-1.0, 1.0));
        }
        // 包含一些特殊值: 0、极值、超过 1.0 的值(测试 clamp)
        // 总数 16 个,确保 SIMD path 不进入尾部标量循环
        samples.extend([
            0.0_f32, 1.0, -1.0, 1.5, -1.5, // clamp 边界测试
            0.5, -0.5, 0.25, -0.25, 0.125, -0.125, // .5 边界舍入测试
            0.0, 0.0, 0.0, 0.0, 0.0, // 填充对齐
        ]);
        // 断言总长度是 16 的倍数(SSE2 chunk=8, AVX2 chunk=16 的 LCM)
        debug_assert!(samples.len() % 16 == 0, "测试样本长度需对齐到 16");
        samples
    }

    #[test]
    fn test_i16_simd_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut simd_out = Vec::new();

        // 强制走标量路径:直接调用核心逻辑
        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        convert_samples_to_bytes_into(&samples, 16, false, &mut simd_out);

        assert_eq!(scalar_out.len(), simd_out.len(), "输出长度不一致");

        let scalar_i16: Vec<i16> = scalar_out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let simd_i16: Vec<i16> = simd_out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        for (i, (s, v)) in scalar_i16.iter().zip(simd_i16.iter()).enumerate() {
            let diff = s.abs_diff(*v);
            // 标量 `as i16` 与 SIMD _mm256_cvtps_epi32 在 .5 边界处采用不同舍入模式
            // (Rust as 是 truncation-toward-zero, MXCSR 默认 RNE),最大差异 1 LSB
            assert!(
                diff <= 1,
                "i16 差异过大 at {}: scalar={}, simd={}, diff={}",
                i,
                s,
                v,
                diff
            );
        }
    }

    #[test]
    fn test_i32_simd_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut simd_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        convert_samples_to_bytes_into(&samples, 32, false, &mut simd_out);

        // i32 路径: SIMD 用 2147483520.0 作上限,标量用 i32::MAX as f32 饱和
        // 输入 = 1.0 时:标量得 i32::MAX(2147483647),SIMD 得 2147483520,差 127
        // 逐字节比较,允许差异时跳过
        assert_eq!(
            scalar_out.len(),
            simd_out.len(),
            "输出长度不一致"
        );

        let scalar_i32: Vec<i32> = scalar_out
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let simd_i32: Vec<i32> = simd_out
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        for (i, (s, v)) in scalar_i32.iter().zip(simd_i32.iter()).enumerate() {
            let diff = (s.abs_diff(*v)) as i64;
            // SIMD scale 比标量小最多 128,允许 1 LSB 误差
            assert!(
                diff <= 128,
                "i32 差异过大 at {}: scalar={}, simd={}, diff={}",
                i,
                s,
                v,
                diff
            );
        }
    }

    #[test]
    fn test_f32_bytes_passthrough() {
        let samples = make_test_samples(32);
        let mut out = Vec::new();
        convert_samples_to_bytes_into(&samples, 32, true, &mut out);

        // 32-bit float 应该是直接的字节拷贝
        assert_eq!(out.len(), samples.len() * 4);
        let as_f32: Vec<f32> = out
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        assert_eq!(as_f32, samples);
    }

    #[test]
    fn test_24bit_scalar_path() {
        let samples = make_test_samples(32);
        let mut out = Vec::new();
        convert_samples_to_bytes_into(&samples, 24, false, &mut out);
        assert_eq!(out.len(), samples.len() * 3);
    }

    /// 直接测试 SSE2 path(强制使用,绕过 is_x86_feature_detected 检测)
    /// 确保老 CPU 上的兜底路径输出正确
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_i16_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut sse2_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        // 直接调用 SSE2 path
        unsafe { f32_to_i16_bytes_sse2(&samples, &mut sse2_out) };

        assert_eq!(scalar_out.len(), sse2_out.len(), "SSE2 i16 长度不一致");

        let scalar_i16: Vec<i16> = scalar_out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        let sse2_i16: Vec<i16> = sse2_out
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();

        for (i, (s, v)) in scalar_i16.iter().zip(sse2_i16.iter()).enumerate() {
            let diff = s.abs_diff(*v);
            // SSE2 _mm_cvtps_epi32 与 AVX2 一样使用 MXCSR 默认 RNE
            // 标量 `as i16` 是 truncation-toward-zero,.5 边界处差 1 LSB
            assert!(
                diff <= 1,
                "SSE2 i16 差异过大 at {}: scalar={}, sse2={}, diff={}",
                i,
                s,
                v,
                diff
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_i32_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut sse2_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        unsafe { f32_to_i32_bytes_sse2(&samples, &mut sse2_out) };

        assert_eq!(scalar_out.len(), sse2_out.len(), "SSE2 i32 长度不一致");

        let scalar_i32: Vec<i32> = scalar_out
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let sse2_i32: Vec<i32> = sse2_out
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        for (i, (s, v)) in scalar_i32.iter().zip(sse2_i32.iter()).enumerate() {
            let diff = (s.abs_diff(*v)) as i64;
            // SSE2 i32 路径同样 clamp 到 2147483520.0,与 AVX2 path 行为一致
            assert!(
                diff <= 128,
                "SSE2 i32 差异过大 at {}: scalar={}, sse2={}, diff={}",
                i,
                s,
                v,
                diff
            );
        }
    }

    /// 验证 SSE2 与 AVX2 path 输出完全一致(两者用相同舍入逻辑)
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_avx2_i16_identical() {
        if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
            return; // 无 AVX2 时跳过(不能调用 AVX2 path)
        }
        let samples = make_test_samples(128);
        let mut sse2_out = Vec::new();
        let mut avx2_out = Vec::new();

        unsafe {
            f32_to_i16_bytes_sse2(&samples, &mut sse2_out);
            f32_to_i16_bytes_avx2(&samples, &mut avx2_out);
        }

        assert_eq!(sse2_out, avx2_out, "SSE2 与 AVX2 i16 输出应完全一致");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_avx2_i32_identical() {
        if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
            return;
        }
        let samples = make_test_samples(128);
        let mut sse2_out = Vec::new();
        let mut avx2_out = Vec::new();

        unsafe {
            f32_to_i32_bytes_sse2(&samples, &mut sse2_out);
            f32_to_i32_bytes_avx2(&samples, &mut avx2_out);
        }

        assert_eq!(sse2_out, avx2_out, "SSE2 与 AVX2 i32 输出应完全一致");
    }
}
