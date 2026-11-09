//! 智能过渡（仅共享模式）：按节拍对齐切入下一首，并做等功率交叉淡化 + DJ 式低频互换。
//!
//! 调度线程在旧曲快结束前预解码下一首，然后让两条音源同时出声：
//! - 旧曲：整体音量 1→0，低频在淡化前半段先退场（bass swap）
//! - 新曲：整体音量 0→1，低频在淡化后半段进场
//!
//! 独占模式不参与：位完美直出要求单条流，和"两首同时混音"天然冲突。

use crate::AppState;
use crate::config::manager::TrackSnapshot;
use crate::equalizer::EqSettings;
use crate::media::{beat_cache, beat_model, metadata};

use super::decoder::{LockFreeSymphoniaSource, SymphoniaDecoder};
use super::queue;
use super::shared_output::SharedOutput;
use super::spectrum::{SpectrumGate, VisualizationSource};

use rodio::Source;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 低频互换的分频点（Hz）
const BASS_SPLIT_HZ: f32 = 120.0;
/// 增益平滑时间常数（秒）：把调度线程 20ms 一步的台阶抹平，避免 zipper noise
const GAIN_SMOOTH_SECS: f32 = 0.025;
/// 两首都没有节拍数据时的兜底淡化时长（秒）：纯固定淡化不宜太长，
/// 否则两首人声重叠 4 秒会很吵
const ADAPTIVE_FADE_SECS: f64 = 3.0;
/// 淡化时长下限/上限（秒）：过长的重叠会让两首歌同时有人声，听感反而浑浊
const MIN_FADE_SECS: f64 = 2.5;
const MAX_FADE_SECS: f64 = 5.0;
/// BPM 差得越远，拍点只会越走越偏，重叠越短越干净
const MISMATCH_MIN_FADE_SECS: f64 = 1.8;
const MISMATCH_MAX_FADE_SECS: f64 = 3.0;
/// 尾部能量分析窗口长度（秒）
const ENERGY_TAIL_SECS: f64 = 20.0;
/// 头部能量分析窗口长度（秒）
const ENERGY_HEAD_SECS: f64 = 10.0;
/// 能量窗口步长（秒）
const ENERGY_WINDOW_SECS: f64 = 0.1;
/// 预解码提前量（秒）：在下一首准备出声前 10 秒就把解码器打开并填满缓冲，
/// 冷盘/高码率 FLAC 也不至于到切点才开始读盘
const PRELOAD_LEAD_SECS: f64 = 10.0;
/// 无缝接续判定：上一曲尾部这段长度内没有任何留白，下一曲从 0 秒起就是音乐
const GAPLESS_TAIL_TOLERANCE_SECS: f64 = 0.35;
/// 无缝接续时提前多久把下一首挂进同一条播放队列（给调度抖动留余量）
const GAPLESS_LEAD_SECS: f64 = 0.25;

/// 一条音源的过渡增益控制器。
///
/// 音频线程逐采样读取目标值并向其平滑逼近；控制侧（调度线程）只做低频写入。
pub struct TransitionGain {
    gain: AtomicU32,
    bass: AtomicU32,
}

impl TransitionGain {
    #[must_use]
    pub fn new(gain: f32, bass: f32) -> Self {
        Self {
            gain: AtomicU32::new(gain.to_bits()),
            bass: AtomicU32::new(bass.to_bits()),
        }
    }

    pub fn set(&self, gain: f32, bass: f32) {
        self.gain
            .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.bass
            .store(bass.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    fn bass(&self) -> f32 {
        f32::from_bits(self.bass.load(Ordering::Relaxed))
    }
}

/// 包在最终输出音源上的增益/滤波层
pub struct TransitionSource<I> {
    inner: I,
    control: Arc<TransitionGain>,
    channels: usize,
    channel_index: usize,
    current_gain: f32,
    current_bass: f32,
    smooth_coeff: f32,
    lowpass_coeff: f32,
    lowpass_state: Vec<f32>,
}

impl<I: Source<Item = f32> + Send> TransitionSource<I> {
    pub fn new(inner: I, control: Arc<TransitionGain>) -> Self {
        let sample_rate = inner.sample_rate().get();
        let channels = usize::from(inner.channels().get()).max(1);
        let smooth_coeff = 1.0 - (-1.0 / (GAIN_SMOOTH_SECS * sample_rate as f32)).exp();
        let lowpass_coeff =
            1.0 - (-std::f32::consts::TAU * BASS_SPLIT_HZ / sample_rate as f32).exp();
        let initial_gain = control.gain();
        let initial_bass = control.bass();
        Self {
            inner,
            control,
            channels,
            channel_index: 0,
            current_gain: initial_gain,
            current_bass: initial_bass,
            smooth_coeff,
            lowpass_coeff,
            lowpass_state: vec![0.0; channels],
        }
    }
}

impl<I: Source<Item = f32> + Send> Iterator for TransitionSource<I> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let input = self.inner.next()?;

        let target_gain = self.control.gain();
        let target_bass = self.control.bass();
        self.current_gain += (target_gain - self.current_gain) * self.smooth_coeff;
        self.current_bass += (target_bass - self.current_bass) * self.smooth_coeff;

        let channel = self.channel_index;
        self.channel_index = (self.channel_index + 1) % self.channels;
        let state = &mut self.lowpass_state[channel];
        *state += self.lowpass_coeff * (input - *state);
        let low = *state;
        let high = input - low;

        Some(high * self.current_gain + low * self.current_gain * self.current_bass)
    }
}

impl<I: Source<Item = f32> + Send> Source for TransitionSource<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> std::num::NonZero<u16> {
        self.inner.channels()
    }

    fn sample_rate(&self) -> std::num::NonZero<u32> {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// 一次过渡的计划
struct TransitionPlan {
    next_index: usize,
    next: TrackSnapshot,
    /// 旧曲开始淡出的绝对位置（秒）
    fade_start: f64,
    /// 淡化时长（秒）
    fade_len: f64,
    /// 新曲起播位置（秒），用于跳过开头静音 / 对齐小节线
    new_start: f64,
    /// 是否两首都命中了节拍缓存，按小节线对齐
    beat_aligned: bool,
    /// 无缝接续：不做任何重叠，旧曲播完后新曲直接从同一条队列接上
    gapless: bool,
}

/// 当前正在播放的曲目在调度线程里的游标
struct TrackCursor {
    path: String,
    duration: Option<f64>,
    samples: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
    gain: Arc<TransitionGain>,
}

/// 在共享模式起播后启动智能过渡调度线程。
///
/// `enabled` 关闭时立即返回；线程会一直跟随后续曲目（过渡成功后自己切换到下一首），
/// 直到队列到底、取消或禁用。
pub fn spawn_smart_transition(
    app: AppHandle,
    state: &AppState,
    path: String,
    duration: Option<f64>,
    samples: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
) {
    let transition = &state.player.transition;
    if !transition.enabled.load(Ordering::SeqCst) {
        return;
    }

    let sink = Arc::clone(&state.player.output.sink);
    let queue = Arc::clone(&state.player.queue);
    let current_path = Arc::clone(&state.player.track.current_path);
    let current_gain = Arc::clone(&transition.current_gain);
    let enabled = Arc::clone(&transition.enabled);
    let generation = Arc::clone(&transition.generation);
    let authority = Arc::clone(&state.player.track.playback_token);
    let eq_settings = state.equalizer.get_settings_handle();
    let spectrum_data = Arc::clone(&state.player.visualization.spectrum_data);
    let target_fps = Arc::clone(&state.player.visualization.target_fps);
    let gate = Arc::clone(&state.player.visualization.spectrum_gate);

    let my_generation = generation.load(Ordering::SeqCst);
    let initial_gain = lock_gain(&current_gain);
    let Some(initial_gain) = initial_gain else {
        log::warn!("智能过渡缺少当前音源的增益句柄，跳过调度: {path}");
        return;
    };

    std::thread::spawn(move || {
        let mut cursor = TrackCursor {
            path,
            duration,
            samples,
            sample_rate,
            channels,
            gain: initial_gain,
        };

        // 当前曲也补一次节拍分析：首次起播/手动跳歌时缓存往往还没有，
        // 等到片尾 20 秒再分析就赶不上 A→B 的切点决策了
        if cursor
            .duration
            .is_some_and(|d| d - cursor_samples_secs(&cursor) > PLAN_WINDOW_SECS + 5.0)
        {
            maybe_analyze_beats(&cursor.path);
        }

        loop {
            if !enabled.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != my_generation
            {
                return;
            }
            let Some(duration) = cursor.duration else {
                return;
            };

            // 先确定下一首并启动节拍分析，等快到片尾再算切点：
            // 给 Beat This! 留出几秒分析时间，A→B 才有机会按小节对齐。
            let Some(next_path) = peek_next_path(&queue, &cursor.path) else {
                return;
            };
            maybe_analyze_beats(&next_path);
            if !wait_for_plan_window(&enabled, &generation, my_generation, &cursor, duration) {
                return;
            }
            let position = cursor_samples_secs(&cursor);
            let Some(plan) = compute_plan(&queue, &cursor.path, duration, position) else {
                return;
            };
            // 队列可能在等待期间被改过；新计划的下一首也要补一次分析
            maybe_analyze_beats(&plan.next.path);

            let Some(next) = wait_and_preload(&enabled, &generation, my_generation, &cursor, &plan)
            else {
                return;
            };

            let Some(transition) = start_transition(
                &app,
                &sink,
                &queue,
                &current_path,
                &current_gain,
                &authority,
                &eq_settings,
                &spectrum_data,
                &target_fps,
                &gate,
                &cursor,
                &plan,
                next,
                &generation,
                my_generation,
            ) else {
                return;
            };

            // 按新曲实际消费的采样数推进淡入淡出：暂停时两条链路一起停，进度也一起停
            let fade_start = transition.start;
            let old_gain = cursor.gain;
            // 无缝直切不碰旧曲增益，让它按自身内容播完；新曲已在同一条队列里等着接上
            if !plan.gapless {
                loop {
                    if !enabled.load(Ordering::SeqCst)
                        || generation.load(Ordering::SeqCst) != my_generation
                    {
                        transition.new_gain.set(1.0, 1.0);
                        lock_or_log!(sink.lock()).cancel_transition();
                        return;
                    }

                    let elapsed = (cursor_samples_secs_of(
                        &transition.new_samples,
                        transition.sample_rate,
                        transition.channels,
                    ) - fade_start)
                        .max(0.0);
                    let progress = (elapsed / plan.fade_len).clamp(0.0, 1.0) as f32;
                    let curve = progress * std::f32::consts::FRAC_PI_2;
                    let old_g = curve.cos();
                    let new_g = curve.sin();
                    // 低频走线性互补（旧曲低频 1→0、新曲 0→1），中高频走等功率：
                    // 这样低频交接连续，不会在淡化中点出现"低音空洞"。
                    let old_bass = if old_g > 1e-4 {
                        ((1.0 - progress) / old_g).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let new_bass = if new_g > 1e-4 {
                        (progress / new_g).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };

                    old_gain.set(old_g, old_bass);
                    transition.new_gain.set(new_g, new_bass);

                    if progress >= 1.0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }

                old_gain.set(0.0, 0.0);
                transition.new_gain.set(1.0, 1.0);
            }
            lock_or_log!(sink.lock()).finish_transition();

            // 新曲成为当前曲目，继续为下一次过渡做准备
            cursor = TrackCursor {
                path: plan.next.path,
                duration: transition.duration,
                samples: Arc::clone(&transition.new_samples),
                sample_rate: transition.sample_rate,
                channels: transition.channels,
                gain: Arc::clone(&transition.new_gain),
            };
            if cursor.duration.is_none() {
                return;
            }
        }
    });
}

fn cursor_samples_secs(cursor: &TrackCursor) -> f64 {
    cursor_samples_secs_of(&cursor.samples, cursor.sample_rate, cursor.channels)
}

fn cursor_samples_secs_of(samples: &Arc<AtomicU64>, sample_rate: u32, channels: u16) -> f64 {
    let divisor = f64::from(sample_rate) * f64::from(channels);
    if divisor <= 0.0 {
        return 0.0;
    }
    samples.load(Ordering::Relaxed) as f64 / divisor
}

fn lock_gain(slot: &Arc<Mutex<Option<Arc<TransitionGain>>>>) -> Option<Arc<TransitionGain>> {
    lock_or_log!(slot.lock()).clone()
}

/// 计算切点前预留的分析窗口（秒）：让下一首的节拍分析有机会在决策前落进缓存
const PLAN_WINDOW_SECS: f64 = 20.0;

fn peek_next_path(queue: &Arc<Mutex<queue::PlaybackQueue>>, current_path: &str) -> Option<String> {
    let guard = lock_or_log!(queue.lock());
    if guard.current().map(|t| t.path.as_str()) != Some(current_path) {
        return None;
    }
    guard.next_on_end().map(|(_, track)| track.path)
}

/// 等到距离曲尾 `PLAN_WINDOW_SECS` 以内（短曲立即返回），期间持续检查取消条件
fn wait_for_plan_window(
    enabled: &Arc<AtomicBool>,
    generation: &Arc<AtomicU64>,
    my_generation: u64,
    cursor: &TrackCursor,
    duration: f64,
) -> bool {
    loop {
        if !enabled.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != my_generation {
            return false;
        }
        if duration - cursor_samples_secs(cursor) <= PLAN_WINDOW_SECS {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

struct PreloadedTrack {
    source: LockFreeSymphoniaSource,
    duration: Option<f64>,
    sample_rate: u32,
    channels: u16,
    /// 实际起播位置：seek 失败或越界时会回落到 0
    start: f64,
}

struct StartedTransition {
    new_gain: Arc<TransitionGain>,
    new_samples: Arc<AtomicU64>,
    sample_rate: u32,
    channels: u16,
    duration: Option<f64>,
    start: f64,
}

/// 等到接近淡化点并预解码下一首；期间持续检查取消条件。
fn wait_and_preload(
    enabled: &Arc<AtomicBool>,
    generation: &Arc<AtomicU64>,
    my_generation: u64,
    cursor: &TrackCursor,
    plan: &TransitionPlan,
) -> Option<PreloadedTrack> {
    let mut preloaded: Option<PreloadedTrack> = None;
    loop {
        if !enabled.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != my_generation {
            return None;
        }
        let position = cursor_samples_secs(cursor);
        let remaining = plan.fade_start - position;
        // 统一在片尾前 10 秒开始预载：普通交叉淡化时切点会比片尾更早，
        // 只按"距切点"判断会让大文件起播得太晚
        let until_end = cursor.duration.unwrap_or(remaining) - position;
        let preload_due = remaining <= PRELOAD_LEAD_SECS || until_end <= PRELOAD_LEAD_SECS;
        if preload_due && preloaded.is_none() {
            preloaded = preload_track(&plan.next.path, plan.new_start);
            if preloaded.is_none() {
                log::warn!("智能过渡预解码失败，回退到普通切歌: {}", plan.next.path);
                return None;
            }
        }
        if remaining <= 0.0 {
            return preloaded;
        }
        let sleep_ms = if remaining > 3.0 {
            200
        } else if remaining > 0.5 {
            25
        } else {
            // 切点前最后半秒加密轮询：无缝直切只有 0.25s 余量，晚到就会掉进间隙
            10
        };
        std::thread::sleep(Duration::from_millis(sleep_ms));
    }
}

fn preload_track(path: &str, start_secs: f64) -> Option<PreloadedTrack> {
    let mut decoder = match SymphoniaDecoder::new(path) {
        Ok(decoder) => decoder,
        Err(e) => {
            log::warn!("智能过渡打开下一首失败: {path}: {e}");
            return None;
        }
    };
    let duration = decoder.total_duration().map(|d| d.as_secs_f64());
    let mut actual_start = start_secs.max(0.0);
    if actual_start > 0.0 {
        let out_of_range = duration.is_some_and(|d| actual_start >= d - 0.2);
        let seek_failed = decoder.seek(Duration::from_secs_f64(actual_start)).is_err();
        if out_of_range || seek_failed {
            if seek_failed {
                log::warn!("智能过渡 Seek 下一首失败，从头部起播: {path}");
            }
            actual_start = 0.0;
        }
    }
    let _ = decoder.prefill_buffer();
    let sample_rate = decoder.sample_rate();
    let channels = decoder.target_channels();
    let source = LockFreeSymphoniaSource::new(decoder);
    Some(PreloadedTrack {
        source,
        duration,
        sample_rate,
        channels,
        start: actual_start,
    })
}

#[allow(clippy::too_many_arguments)]
fn start_transition(
    app: &AppHandle,
    sink: &Arc<Mutex<SharedOutput>>,
    queue: &Arc<Mutex<queue::PlaybackQueue>>,
    current_path: &Arc<Mutex<Option<String>>>,
    current_gain: &Arc<Mutex<Option<Arc<TransitionGain>>>>,
    authority: &Arc<AtomicU64>,
    eq_settings: &Arc<RwLock<EqSettings>>,
    spectrum_data: &Arc<Mutex<Vec<f32>>>,
    target_fps: &Arc<AtomicU64>,
    gate: &Arc<SpectrumGate>,
    cursor: &TrackCursor,
    plan: &TransitionPlan,
    preloaded: PreloadedTrack,
    generation: &Arc<AtomicU64>,
    my_generation: u64,
) -> Option<StartedTransition> {
    let preloaded_duration = preloaded.duration;
    let preloaded_sample_rate = preloaded.sample_rate;
    let preloaded_channels = preloaded.channels;
    let actual_start = preloaded.start;

    // 只有真正提交时才创建音源（创建会让播放权威令牌前移），避免中止时把旧曲的
    // 进度与 track-ended 永久压掉。
    let (new_gain, new_samples) = {
        let transition_gate = super::playback::shared_sink_gate();
        let _gate = lock_or_log!(transition_gate.lock());
        // 等锁期间可能已被手动切歌/seek 取消：绝不能在别人的新播放上再叠一条音源
        if generation.load(Ordering::SeqCst) != my_generation {
            return None;
        }
        // 队列在等待期间也可能被改过：确认当前曲目与"下一首"都还是计划里的那两首
        {
            let guard = lock_or_log!(queue.lock());
            let still_current =
                guard.current().map(|t| t.path.as_str()) == Some(cursor.path.as_str());
            let still_next = guard.next_on_end().is_some_and(|(index, track)| {
                index == plan.next_index && track.path == plan.next.path
            });
            drop(guard);
            if !still_current || !still_next {
                return None;
            }
        }

        let (sink_sr, sink_ch) = {
            let guard = lock_or_log!(sink.lock());
            (guard.sample_rate(), guard.channels())
        };
        let sink_ch_nz = std::num::NonZero::new(sink_ch)
            .unwrap_or_else(|| std::num::NonZero::new(2).expect("2 is nonzero"));
        let sink_sr_nz = std::num::NonZero::new(sink_sr)
            .unwrap_or_else(|| std::num::NonZero::new(48_000).expect("48000 is nonzero"));

        let initial = if plan.gapless { 1.0 } else { 0.0 };
        let new_gain = Arc::new(TransitionGain::new(initial, initial));
        let visualization = VisualizationSource::new(
            preloaded.source,
            Arc::clone(spectrum_data),
            Some(app.clone()),
            Arc::clone(target_fps),
            Arc::clone(gate),
            Arc::clone(authority),
        )
        .with_start_position(actual_start as f32)
        .with_eq_settings(Arc::clone(eq_settings));
        let new_samples = visualization.samples_played_handle();
        let source = TransitionSource::new(
            rodio::source::UniformSourceIterator::new(visualization, sink_ch_nz, sink_sr_nz),
            Arc::clone(&new_gain),
        );

        {
            let mut guard = lock_or_log!(sink.lock());
            if plan.gapless {
                // 无缝专辑：新曲挂到旧曲所在的那条队列后面，旧曲播完的同一采样回调里
                // 直接接上，没有任何重叠或留白
                guard.append(source);
            } else {
                guard.begin_transition(source);
            }
        }
        *lock_or_log!(current_gain.lock()) = Some(Arc::clone(&new_gain));
        *lock_or_log!(current_path.lock()) = Some(plan.next.path.clone());
        // 队列下标必须跟着切到新曲：后续自然结束时才能从正确位置推进
        if let Ok(mut guard) = queue.lock() {
            guard.set_index(plan.next_index);
        }
        (new_gain, new_samples)
    };
    let state = app.state::<AppState>();
    queue::note_playback_started(app, &state, &plan.next.path, Some(actual_start as f32));
    let _ = app.emit(
        "queue-track-changed",
        queue::QueueTrackChangedEvent {
            index: Some(plan.next_index),
            track: Some(plan.next.clone()),
            reason: "transition".to_string(),
        },
    );
    queue::sync_media_session(app, &state);

    log::info!(
        "智能过渡开始: {} -> {} (淡出 {:.2}s @ {:.2}s, 新曲起于 {:.2}s, 节拍对齐 {}, 无缝直切 {})",
        cursor.path,
        plan.next.path,
        plan.fade_len,
        plan.fade_start,
        actual_start,
        plan.beat_aligned,
        plan.gapless
    );

    Some(StartedTransition {
        new_gain,
        new_samples,
        sample_rate: preloaded_sample_rate,
        channels: preloaded_channels,
        duration: preloaded_duration,
        start: actual_start,
    })
}

fn compute_plan(
    queue: &Arc<Mutex<queue::PlaybackQueue>>,
    current_path: &str,
    duration: f64,
    position: f64,
) -> Option<TransitionPlan> {
    if duration <= 2.0 {
        return None;
    }
    let (next_index, next) = {
        let guard = lock_or_log!(queue.lock());
        if guard.current().map(|t| t.path.as_str()) != Some(current_path) {
            return None;
        }
        guard.next_on_end()?
    };

    let old_beat = beat_cache::cached(current_path);
    let new_beat = beat_cache::cached(&next.path);

    let bpm = old_beat
        .as_ref()
        .and_then(|b| b.bpm)
        .or_else(|| new_beat.as_ref().and_then(|b| b.bpm))
        .or(next.bpm)
        .unwrap_or(120.0)
        .clamp(60.0, 240.0);
    let beats_per_bar = old_beat
        .as_ref()
        .or(new_beat.as_ref())
        .and_then(|beat| beat.meter.iter().max_by_key(|m| m.count))
        .map_or(4, |m| m.beats_per_bar)
        .clamp(2, 8);
    let bar_secs = (f64::from(beats_per_bar) * 60.0 / bpm).clamp(0.5, 4.0);
    let bpm_ratio = match (
        old_beat.as_ref().and_then(|b| b.bpm),
        new_beat.as_ref().and_then(|b| b.bpm),
    ) {
        (Some(old), Some(new)) => (new / old - 1.0).abs(),
        _ => f64::INFINITY,
    };
    // 节拍接近时 4 小节让混合更充分；一般差距 2 小节；差距大到拍点越走越偏时
    // 只保留 1 小节，避免两首节奏在重叠期间"打架"
    let (bars, min_fade, max_fade) = if bpm_ratio < 0.03 {
        (4.0, MIN_FADE_SECS, MAX_FADE_SECS)
    } else if bpm_ratio < 0.10 {
        (2.0, MIN_FADE_SECS, MAX_FADE_SECS)
    } else {
        (1.0, MISMATCH_MIN_FADE_SECS, MISMATCH_MAX_FADE_SECS)
    };

    let has_old_beats = old_beat
        .as_ref()
        .is_some_and(|b| !b.downbeats.is_empty() && b.span_secs > 0.0);
    let has_new_beats = new_beat.as_ref().is_some_and(|b| !b.downbeats.is_empty());
    // 片尾静音不参与过渡，否则会在无声段里干等
    let tail = tail_music_end(current_path, duration);
    let music_end = tail.unwrap_or(duration);
    // 单曲循环重播同一首时从 0 开始，避免把开头的前奏裁掉
    let music_start = if next.path == current_path {
        0.0
    } else {
        head_music_start(&next.path).unwrap_or(0.0)
    };

    // 无缝接续判定：上一曲的音乐一直持续到最后一个采样，下一曲 0 秒起就是音乐。
    // 这类 gapless 专辑里两首原本就是一段连续音频，任何长度的重叠都会把两首歌叠在一起，
    // 所以这里必须零重叠直切，而不是套用普通曲目的小节对齐交叉淡化。
    // 单曲循环重播同一首同理：与自身做交叉淡化等于把同一段音乐错位叠放，必须无缝重播。
    // 尾部 20 秒完全没检出音乐时 `tail` 为 None，那不属于"音乐延续到结尾"，不能算无缝。
    let gapless = tail.is_some()
        && (duration - music_end).abs() <= GAPLESS_TAIL_TOLERANCE_SECS
        && music_start <= 0.2;
    if gapless {
        let fade_start = (music_end - GAPLESS_LEAD_SECS).max(0.0);
        if fade_start <= position + 0.4 {
            return None;
        }
        return Some(TransitionPlan {
            next_index,
            next,
            fade_start,
            fade_len: 0.0,
            new_start: 0.0,
            beat_aligned: false,
            gapless: true,
        });
    }

    let fade_end = (music_end + 0.3).clamp(0.0, (duration - 0.15).max(0.0));

    // 两首都完全没有节拍数据时退回固定的自适应淡化长度；有任一首的拍点就按小节长度算
    let suggested_fade_len = if old_beat.is_none() && new_beat.is_none() {
        ADAPTIVE_FADE_SECS
    } else {
        (bars * bar_secs).clamp(min_fade, max_fade)
    };
    let mut fade_len = suggested_fade_len.min((fade_end - 0.5).max(1.0));
    let original_start = (fade_end - fade_len).max(0.0);
    let mut fade_start = original_start;

    if has_old_beats {
        if let Some(beat) = &old_beat {
            // 让淡化从一个小节线开始；只在小半格内寻找，避免把切点拉得太远
            if let Some(downbeat) = beat
                .downbeats
                .iter()
                .rev()
                .find(|d| **d <= fade_start && **d >= fade_start - bar_secs * 0.75)
            {
                fade_start = *downbeat;
            }
        }
    }

    // 对齐到更早的小节线后，把淡化时长补足到 fade_end；补足后超出上限就退回原切点
    let aligned_len = fade_end - fade_start;
    if (min_fade..=max_fade).contains(&aligned_len) {
        fade_len = aligned_len;
    } else {
        fade_start = original_start;
    }

    if fade_start <= position + 0.4 {
        return None;
    }

    // 只在第一拍离音乐起点很近（≤1s）时跳到小节线，保留正常前奏/人声 intro
    let new_start = new_beat
        .as_ref()
        .and_then(|beat| {
            beat.downbeats
                .iter()
                .find(|d| **d > music_start && **d <= music_start + 1.0)
                .copied()
        })
        .unwrap_or(music_start);

    Some(TransitionPlan {
        next_index,
        next,
        fade_start,
        fade_len,
        new_start,
        beat_aligned: has_old_beats && has_new_beats,
        gapless: false,
    })
}

/// 尾部能量分析：找出最后一个"还有音乐"的 0.1s 窗口，避开片尾静音
fn tail_music_end(path: &str, duration: f64) -> Option<f64> {
    let start = (duration - ENERGY_TAIL_SECS).max(0.0);
    let windows = energy_windows(path, start, ENERGY_TAIL_SECS)?;
    let max = windows.iter().copied().fold(0.0_f32, f32::max);
    if max < 0.0005 {
        return None;
    }
    // 2% ≈ -34dB：只把明显低于峰值的长段当成"片尾静音"，不裁掉正常的渐弱尾奏
    let threshold = (max * 0.02).max(0.0005);
    let last = windows.iter().rposition(|rms| *rms > threshold)?;
    Some(start + (last as f64 + 1.0) * ENERGY_WINDOW_SECS)
}

/// 头部能量分析：只跳过开头真正的静音（≥1.2s），不裁掉安静的前奏或人声 intro
fn head_music_start(path: &str) -> Option<f64> {
    let windows = energy_windows(path, 0.0, ENERGY_HEAD_SECS)?;
    // 绝对阈值 ≈ -56 dBFS：只有接近数字静音的开头才跳过
    let silence_threshold = 0.0015_f32;
    let mut quiet_windows = 0usize;
    for rms in &windows {
        if *rms < silence_threshold {
            quiet_windows += 1;
        } else {
            break;
        }
    }
    let silent_secs = quiet_windows as f64 * ENERGY_WINDOW_SECS;
    if silent_secs >= 1.2 {
        Some((silent_secs - ENERGY_WINDOW_SECS).max(0.0))
    } else {
        Some(0.0)
    }
}

fn energy_windows(path: &str, start_secs: f64, max_secs: f64) -> Option<Vec<f32>> {
    let mut decoder = SymphoniaDecoder::new(path).ok()?;
    if start_secs > 0.0 {
        decoder.seek(Duration::from_secs_f64(start_secs)).ok()?;
    }
    let sample_rate = decoder.sample_rate();
    let channels = usize::from(decoder.target_channels()).max(1);
    let frames_per_window = (sample_rate as f64 * ENERGY_WINDOW_SECS) as usize;
    let max_windows = (max_secs / ENERGY_WINDOW_SECS) as usize;
    let mut windows = Vec::with_capacity(max_windows);
    let mut sum_squares = 0.0_f64;
    let mut frame_in_window = 0usize;
    let mut frame_samples = 0usize;
    let mut frame_sum = 0.0_f64;

    while windows.len() < max_windows {
        let Some(sample) = decoder.next() else {
            break;
        };
        frame_sum += f64::from(sample);
        frame_samples += 1;
        if frame_samples < channels {
            continue;
        }
        let mono = frame_sum / channels as f64;
        frame_sum = 0.0;
        frame_samples = 0;
        sum_squares += mono * mono;
        frame_in_window += 1;
        if frame_in_window >= frames_per_window {
            windows.push((sum_squares / frame_in_window as f64).sqrt() as f32);
            sum_squares = 0.0;
            frame_in_window = 0;
        }
    }
    if frame_in_window > 0 {
        windows.push((sum_squares / frame_in_window as f64).sqrt() as f32);
    }
    if windows.is_empty() {
        None
    } else {
        Some(windows)
    }
}

/// 后台补齐下一首的节拍缓存：当前曲目播放期间完成分析，下一次过渡就能按小节对齐。
fn maybe_analyze_beats(path: &str) {
    static IN_FLIGHT: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);
    if beat_cache::cached(path).is_some() || !beat_model::is_available() {
        return;
    }
    let inserted = lock_or_log!(IN_FLIGHT.lock())
        .get_or_insert_with(std::collections::HashSet::new)
        .insert(path.to_string());
    if !inserted {
        return;
    }

    let path = path.to_string();
    std::thread::spawn(move || {
        match beat_model::analyze(&path) {
            Ok(track) => {
                beat_cache::store(&path, &track);
                metadata::cache::patch_cached_bpm(&path, track.bpm);
                if let Err(e) = beat_cache::flush() {
                    log::warn!("智能过渡节拍缓存落盘失败: {e}");
                }
            }
            Err(e) => log::debug!("智能过渡节拍分析失败({path}): {e}"),
        }
        if let Ok(mut guard) = IN_FLIGHT.lock() {
            if let Some(set) = guard.as_mut() {
                set.remove(&path);
            }
        }
    });
}

/// 取消智能过渡：作废调度线程、恢复当前音源增益，并丢弃仍在淡出的旧曲。
pub fn cancel_smart_transition(state: &AppState) {
    state
        .player
        .transition
        .generation
        .fetch_add(1, Ordering::SeqCst);
    if let Some(gain) = lock_or_log!(state.player.transition.current_gain.lock()).as_ref() {
        gain.set(1.0, 1.0);
    }
    lock_or_log!(state.player.output.sink.lock()).cancel_transition();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZero;

    struct ConstSource {
        sample_rate: NonZero<u32>,
        channels: NonZero<u16>,
        value: f32,
        remaining: usize,
    }

    impl ConstSource {
        fn new(sample_rate: u32, channels: u16, value: f32, remaining: usize) -> Self {
            Self {
                sample_rate: NonZero::new(sample_rate).unwrap(),
                channels: NonZero::new(channels).unwrap(),
                value,
                remaining,
            }
        }
    }

    impl Iterator for ConstSource {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            if self.remaining == 0 {
                return None;
            }
            self.remaining -= 1;
            Some(self.value)
        }
    }

    impl Source for ConstSource {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.remaining)
        }

        fn channels(&self) -> NonZero<u16> {
            self.channels
        }

        fn sample_rate(&self) -> NonZero<u32> {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    #[test]
    fn 满增益满低频时保持透明() {
        let control = Arc::new(TransitionGain::new(1.0, 1.0));
        let mut source = TransitionSource::new(ConstSource::new(48_000, 2, 1.0, 64), control);
        for _ in 0..64 {
            let sample = source.next().expect("source should produce samples");
            assert!(
                (sample - 1.0).abs() < 1e-5,
                "透明链路不应改变样本: {sample}"
            );
        }
    }

    #[test]
    fn 低频被移除后直流只剩高通分量() {
        let control = Arc::new(TransitionGain::new(1.0, 0.0));
        let mut source = TransitionSource::new(ConstSource::new(48_000, 1, 1.0, 4_000), control);
        let mut last = 1.0;
        for _ in 0..2_000 {
            last = source.next().expect("source should produce samples");
        }
        assert!(last.abs() < 0.1, "低频互换后应压掉直流: {last}");
    }
}
