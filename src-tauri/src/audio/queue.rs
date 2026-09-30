//! 播放队列与媒体控制入口。Android 后台时 WebView 的 JS 被冻结，`track-ended` 无人处理，
//! 故队列在 Rust 侧维护、EOF 由 Rust 推进（桌面端 `auto_advance=false`，行为零改变）；
//! 随机序/循环序由前端算好后整条推过来，Rust 不实现 shuffle，避免两端算法分歧。

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use crate::config::manager::TrackSnapshot;
use crate::error::AppError;

use super::LockOrErr;

/// 最近一次上报的播放位置（毫秒）。通知栏/MediaSession 只在状态变化时刷新，之后由系统用
/// `updateTime + speed` 自行推算进度，因此这里只需一个原子量。
static LAST_POSITION_MS: AtomicU64 = AtomicU64::new(0);

/// 记录播放位置（由 `emit_playback_position` 调用）
pub fn note_position(secs: f32) {
    let ms = if secs > 0.0 {
        (secs * 1000.0) as u64
    } else {
        0
    };
    LAST_POSITION_MS.store(ms, Ordering::Relaxed);
}

/// 取最近一次播放位置（毫秒）
#[must_use]
pub fn last_position_ms() -> u64 {
    LAST_POSITION_MS.load(Ordering::Relaxed)
}

/// 循环模式（与前端 `repeatMode` 对齐）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    /// 不循环：队列播完即停
    #[default]
    Off,
    /// 列表循环：播到末尾回到开头
    All,
    /// 单曲循环：一直重播当前曲目
    One,
}

impl RepeatMode {
    /// 从前端字符串解析（兼容前端的 'off' / 'all' / 'one' 与历史别名）
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "all" | "list" | "loop" => Self::All,
            "one" | "track" | "single" => Self::One,
            _ => Self::Off,
        }
    }

    /// 转回前端字符串
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }
}

/// 播放队列
///
/// tracks/repeat/auto_advance 由前端经 `set_play_queue` 整条同步，index 两端都会更新。
#[derive(Debug, Default)]
pub struct PlaybackQueue {
    /// 已按最终播放顺序排列的曲目（随机序由前端算好）
    tracks: Vec<TrackSnapshot>,
    /// 当前曲目下标
    index: Option<usize>,
    /// 循环模式
    repeat: RepeatMode,
    /// 是否由 Rust 接管曲目结束后的推进（仅 Android 开启）
    auto_advance: bool,
}

impl PlaybackQueue {
    /// 创建空队列
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 整条替换队列。`index` 为当前曲目在 `tracks` 中的下标；越界或不传时按「未知」处理
    /// （`auto_advance` 依赖它，未知则本次不推进）。
    pub fn set_queue(
        &mut self,
        tracks: Vec<TrackSnapshot>,
        index: Option<usize>,
        repeat: RepeatMode,
        auto_advance: bool,
    ) {
        let index = index.filter(|i| *i < tracks.len());
        self.tracks = tracks;
        self.index = index;
        self.repeat = repeat;
        self.auto_advance = auto_advance;
    }

    /// 仅更新当前下标（前端手动切歌时同步）
    pub fn set_index(&mut self, index: usize) {
        self.index = Some(index).filter(|i| *i < self.tracks.len());
    }

    /// 按路径定位当前下标（前端直接调 `play_track` 时用）
    pub fn sync_index_by_path(&mut self, path: &str) {
        if let Some(pos) = self.tracks.iter().position(|t| t.path == path) {
            self.index = Some(pos);
        }
    }

    /// 更新循环模式
    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    /// 当前曲目下标
    #[must_use]
    pub const fn index(&self) -> Option<usize> {
        self.index
    }

    /// 当前曲目快照
    #[must_use]
    pub fn current(&self) -> Option<&TrackSnapshot> {
        self.index.and_then(|i| self.tracks.get(i))
    }

    /// Rust 是否接管自动推进
    #[must_use]
    pub const fn auto_advance(&self) -> bool {
        self.auto_advance
    }

    /// 曲目自然结束后推进：返回下一首的 `(下标, 快照)`，`None` 表示队列到底应停止；
    /// 单曲循环时返回当前曲目本身。
    pub fn advance_on_end(&mut self) -> Option<(usize, TrackSnapshot)> {
        let len = self.tracks.len();
        if len == 0 {
            return None;
        }
        let current = self.index?;
        match self.repeat {
            RepeatMode::One => self.tracks.get(current).cloned().map(|t| (current, t)),
            RepeatMode::All => {
                let next = (current + 1) % len;
                self.index = Some(next);
                self.tracks.get(next).cloned().map(|t| (next, t))
            }
            RepeatMode::Off => {
                let next = current + 1;
                if next >= len {
                    None
                } else {
                    self.index = Some(next);
                    self.tracks.get(next).cloned().map(|t| (next, t))
                }
            }
        }
    }

    /// 手动「下一首」的下标（单曲循环下也前进，与前端行为一致）
    #[must_use]
    pub fn next_index(&self) -> Option<usize> {
        let len = self.tracks.len();
        if len == 0 {
            return None;
        }
        let current = self.index?;
        match self.repeat {
            RepeatMode::All => Some((current + 1) % len),
            RepeatMode::Off | RepeatMode::One => (current + 1 < len).then_some(current + 1),
        }
    }

    /// 手动「上一首」的下标
    #[must_use]
    pub fn prev_index(&self) -> Option<usize> {
        let len = self.tracks.len();
        if len == 0 {
            return None;
        }
        let current = self.index?;
        match self.repeat {
            RepeatMode::All => Some((current + len - 1) % len),
            RepeatMode::Off | RepeatMode::One => current.checked_sub(1),
        }
    }
}

/// 队列推进事件（前端据此同步播放中曲目）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueTrackChangedEvent {
    /// 新曲目下标（队列到底时为 null）
    pub index: Option<usize>,
    /// 新曲目快照（队列到底时为 null）
    pub track: Option<TrackSnapshot>,
    /// 触发原因：`advance`(自然结束) / `next` / `previous`
    pub reason: String,
}

fn emit_queue_changed(
    app: &AppHandle,
    index: Option<usize>,
    track: Option<TrackSnapshot>,
    reason: &str,
) {
    let _ = app.emit(
        "queue-track-changed",
        QueueTrackChangedEvent {
            index,
            track,
            reason: reason.to_string(),
        },
    );
}

/// 播放到队列末尾且不再循环：暂停输出并通知前端与（Android）通知栏
fn stop_at_queue_end(app: &AppHandle, state: &AppState) {
    if let Ok(sink) = state.player.output.sink.lock().lock_or_err("player") {
        sink.pause();
    }
    emit_queue_changed(app, None, None, "advance");
    sync_media_session(app, state);
}

/// 曲目自然结束的统一入口，由 `emit::emit_track_ended` 在发出 `track-ended` 之后调用：
/// 未开 `auto_advance`（桌面端）立即返回，开则推进队列并播下一首。
/// 调用方是分析线程而非音频回调，直接推进不会与回调形成锁序环。
pub fn handle_track_ended(app: &AppHandle, state: &AppState) {
    let next = {
        let Ok(mut queue) = state.player.queue.lock().lock_or_err("playback queue") else {
            return;
        };
        if !queue.auto_advance() {
            return;
        }
        queue.advance_on_end()
    };

    let Some((index, track)) = next else {
        stop_at_queue_end(app, state);
        return;
    };
    if let Err(e) = play_queue_track(app, state, index, &track, "advance") {
        log::error!("队列自动推进失败: {e}");
    }
}

/// 播放队列中指定下标的曲目，并同步 UI / 通知栏。
///
/// 独占模式开着时必须走独占起播路径，否则后台自动推进会把用户从独占输出上悄悄摘下来
/// （表现为切到下一首就变成系统混音）。独占起播是 async，而这里可能跑在解码/分析线程上
/// （见 [`handle_track_ended`]），所以那条分支整段丢给 Tauri 异步运行时执行。
fn play_queue_track(
    app: &AppHandle,
    state: &AppState,
    index: usize,
    track: &TrackSnapshot,
    reason: &str,
) -> Result<(), AppError> {
    let exclusive = state
        .player
        .output
        .exclusive_mode
        .lock()
        .lock_or_err("exclusive mode")
        .map(|g| *g)?;

    if exclusive {
        let app = app.clone();
        let path = track.path.clone();
        let track = track.clone();
        let reason = reason.to_string();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            if let Err(e) =
                super::playback::play_track_exclusive(&app, &state, &path, None, true).await
            {
                log::error!("队列自动推进(独占模式)失败: {e}");
                return;
            }
            if let Ok(mut queue) = state.player.queue.lock() {
                queue.set_index(index);
            }
            emit_queue_changed(&app, Some(index), Some(track), &reason);
            sync_media_session(&app, &state);
        });
        return Ok(());
    }

    super::playback::play_track_shared(app, state, &track.path, None)?;
    if let Ok(mut queue) = state.player.queue.lock() {
        queue.set_index(index);
    }
    emit_queue_changed(app, Some(index), Some(track.clone()), reason);
    sync_media_session(app, state);
    Ok(())
}

/// 播放开始后同步队列下标与（Android）通知栏：前端任何切歌都走这里，
/// 保证队列下标与通知栏始终跟随实际播放的曲目。
pub fn note_playback_started(app: &AppHandle, state: &AppState, path: &str) {
    if let Ok(mut queue) = state.player.queue.lock() {
        queue.sync_index_by_path(path);
    }
    sync_media_session(app, state);
}

/// 媒体控制动作（通知栏 / MediaSession / 耳机线控共用入口），与前端操作走同一套播放函数。
/// 注意：本入口只驱动共享模式的 rodio sink，不走独占播放器分支。
pub fn media_control(
    app: &AppHandle,
    state: &AppState,
    action: &str,
    position: Option<f32>,
) -> Result<(), AppError> {
    match action {
        "play" => super::commands::resume_playback(state),
        "pause" => super::commands::pause_playback(state),
        "toggle" => {
            let paused = state
                .player
                .output
                .sink
                .lock()
                .lock_or_err("player")
                .map(|sink| sink.is_paused())?;
            if paused {
                super::commands::resume_playback(state)
            } else {
                super::commands::pause_playback(state)
            }
        }
        "stop" => {
            super::commands::pause_playback(state)?;
            if let Ok(sink) = state.player.output.sink.lock().lock_or_err("player") {
                sink.stop();
            }
            Ok(())
        }
        "seek" => {
            let Some(position) = position else {
                return Err(AppError::Audio("seek 缺少目标位置".to_string()));
            };
            let path = state
                .player
                .track
                .current_path
                .lock()
                .lock_or_err("current path")?
                .clone();
            let Some(path) = path else {
                return Err(AppError::Audio("当前没有正在播放的曲目".to_string()));
            };
            super::playback::seek_track_shared(app, state, &path, position)?;
            sync_media_session(app, state);
            Ok(())
        }
        "next" | "previous" => {
            let target = {
                let queue = state.player.queue.lock().lock_or_err("playback queue")?;
                if action == "next" {
                    queue.next_index()
                } else {
                    queue.prev_index()
                }
            };
            let Some(index) = target else {
                return Err(AppError::Audio("队列中没有可切换的曲目".to_string()));
            };
            let track = {
                let mut queue = state.player.queue.lock().lock_or_err("playback queue")?;
                queue.set_index(index);
                queue.current().cloned()
            };
            let Some(track) = track else {
                return Err(AppError::Audio("队列下标越界".to_string()));
            };
            play_queue_track(app, state, index, &track, action)
        }
        "sync" => {
            // App 回到前台：后台期间 WebView 的 JS 被冻结，事件可能积压或丢失，
            // 这里把真实播放状态重新推给前端，避免 UI 停留在后台前的旧曲目/进度。
            let (index, track) = {
                let queue = state.player.queue.lock().lock_or_err("playback queue")?;
                (queue.index(), queue.current().cloned())
            };
            let playing = state
                .player
                .output
                .sink
                .lock()
                .map(|sink| !sink.is_paused())
                .unwrap_or(false);
            let _ = app.emit(
                "playback-state-sync",
                serde_json::json!({
                    "index": index,
                    "track": track,
                    "playing": playing,
                    "positionMs": last_position_ms(),
                }),
            );
            sync_media_session(app, state);
            Ok(())
        }
        other => Err(AppError::Audio(format!("未知的媒体控制动作: {other}"))),
    }
}

/// 把当前播放状态同步给 Android 通知栏 / MediaSession，桌面端为空实现。
/// 仅在切歌、播放状态变化、seek 时调用（低频）；进度由系统的 `updateTime + speed` 推算。
pub fn sync_media_session(app: &AppHandle, state: &AppState) {
    #[cfg(target_os = "android")]
    {
        crate::android::notify_media_session(app, state);
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, state);
    }
}

#[cfg(test)]
mod tests {
    use super::{PlaybackQueue, RepeatMode};
    use crate::config::manager::TrackSnapshot;

    fn snapshot(path: &str) -> TrackSnapshot {
        TrackSnapshot {
            path: path.to_string(),
            ..TrackSnapshot::default()
        }
    }

    fn queue_of(n: usize) -> Vec<TrackSnapshot> {
        (0..n)
            .map(|i| snapshot(&format!("/music/{i}.mp3")))
            .collect()
    }

    #[test]
    fn 空队列不推进() {
        let mut q = PlaybackQueue::new();
        assert!(q.advance_on_end().is_none());
        assert!(q.next_index().is_none());
        assert!(q.prev_index().is_none());
    }

    #[test]
    fn 不循环时播到末尾停止() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(2), Some(0), RepeatMode::Off, true);
        assert_eq!(
            q.advance_on_end().map(|(i, t)| (i, t.path)),
            Some((1, "/music/1.mp3".to_string()))
        );
        assert_eq!(q.index(), Some(1));
        assert!(q.advance_on_end().is_none(), "队列到底应返回 None");
    }

    #[test]
    fn 列表循环回到开头() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(3), Some(2), RepeatMode::All, true);
        assert_eq!(q.advance_on_end().map(|(i, _)| i), Some(0));
        assert_eq!(q.next_index(), Some(1));
        assert_eq!(q.prev_index(), Some(2), "循环模式上一首应回绕到末尾");
    }

    #[test]
    fn 单曲循环重复当前曲目() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(3), Some(1), RepeatMode::One, true);
        assert_eq!(
            q.advance_on_end().map(|(i, _)| i),
            Some(1),
            "结束时重复当前"
        );
        assert_eq!(q.next_index(), Some(2), "手动下一首仍前进");
        assert_eq!(q.prev_index(), Some(0));
    }

    #[test]
    fn 非循环模式的边界不回绕() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(3), Some(0), RepeatMode::Off, true);
        assert_eq!(q.prev_index(), None, "首曲上一首越界返回 None");
        q.set_index(2);
        assert_eq!(q.next_index(), None, "末曲下一首越界返回 None");
    }

    #[test]
    fn 越界下标被丢弃() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(2), Some(99), RepeatMode::All, true);
        assert_eq!(q.index(), None);
        assert!(q.current().is_none());
    }

    #[test]
    fn 按路径定位下标() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(3), None, RepeatMode::Off, false);
        q.sync_index_by_path("/music/2.mp3");
        assert_eq!(q.index(), Some(2));
        q.sync_index_by_path("/music/not-exist.mp3");
        assert_eq!(q.index(), Some(2), "未命中时保持原下标");
    }

    #[test]
    fn 自动推进开关可关闭() {
        let mut q = PlaybackQueue::new();
        q.set_queue(queue_of(3), Some(0), RepeatMode::All, false);
        assert!(!q.auto_advance());
        assert!(q.current().is_some());
    }

    #[test]
    fn 循环模式解析兼容别名() {
        assert_eq!(RepeatMode::parse("off"), RepeatMode::Off);
        assert_eq!(RepeatMode::parse("all"), RepeatMode::All);
        assert_eq!(RepeatMode::parse("list"), RepeatMode::All);
        assert_eq!(RepeatMode::parse("track"), RepeatMode::One);
        assert_eq!(RepeatMode::parse("one"), RepeatMode::One);
        assert_eq!(RepeatMode::parse("未知值"), RepeatMode::Off);
        assert_eq!(RepeatMode::All.as_str(), "all");
    }
}
