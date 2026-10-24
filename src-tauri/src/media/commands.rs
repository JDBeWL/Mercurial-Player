//! 媒体命令
//!
//! 文件系统操作与元数据获取的 Tauri 命令。
use crate::error::AppError;

use super::beat_cache;
use super::beat_model;
use super::bpm::{self, BpmAnalysisResult};
use super::filesystem::{
    check_file_exists_internal, get_all_audio_files_from_dirs, get_audio_files_from_dir, read_dir,
    read_lyrics_file_internal, write_lyrics_file_internal,
};
use super::metadata::{
    Playlist, TrackMetadata, clean_cover_cache, clear_metadata_cache, extract_cover_internal,
    flush_metadata_cache, get_metadata_cache_stats, get_track_cover_path_internal,
    get_track_metadata_internal, set_cover_cache_path,
};
use crate::AppState;
use crate::security::is_within_music_dirs;
use serde::Serialize;
use tauri::{Emitter, State, command};

/// 扫描前的白名单门禁，判定见 `is_within_music_dirs`
fn ensure_scannable(music_dirs: &[String], path: &str) -> Result<(), AppError> {
    if is_within_music_dirs(path, music_dirs) {
        Ok(())
    } else {
        Err(AppError::msg("安全限制：该目录不在已添加的音乐目录内"))
    }
}

#[command]
pub fn read_directory(state: State<'_, AppState>, path: String) -> Result<Vec<String>, AppError> {
    let music_dirs = state.config_manager.load_config()?.music_directories;
    ensure_scannable(&music_dirs, &path)?;
    read_dir(&path)
}

/// 扫描是 CPU + IPC 密集型，放到阻塞线程池执行
#[command]
pub async fn get_audio_files(
    state: State<'_, AppState>,
    path: String,
) -> Result<Playlist, AppError> {
    let music_dirs = state.config_manager.load_config()?.music_directories;
    ensure_scannable(&music_dirs, &path)?;
    tauri::async_runtime::spawn_blocking(move || get_audio_files_from_dir(&path))
        .await
        .map_err(|e| AppError::msg(format!("扫描任务异常退出: {e}")))?
}

#[command]
pub async fn get_all_audio_files(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<Playlist>, AppError> {
    let config = state.config_manager.load_config()?;
    for path in &paths {
        ensure_scannable(&config.music_directories, path)?;
    }
    tauri::async_runtime::spawn_blocking(move || get_all_audio_files_from_dirs(&paths, &config))
        .await
        .map_err(|e| AppError::msg(format!("批量扫描任务异常退出: {e}")))?
}

#[command]
pub fn check_file_exists(path: String) -> Result<bool, AppError> {
    Ok(check_file_exists_internal(&path))
}

#[command]
pub fn read_lyrics_file(path: String) -> Result<String, AppError> {
    read_lyrics_file_internal(&path)
}

#[command]
pub fn write_lyrics_file(path: String, content: String) -> Result<(), AppError> {
    write_lyrics_file_internal(&path, &content)
}

#[command]
pub fn get_track_metadata(path: String) -> Result<TrackMetadata, AppError> {
    get_track_metadata_internal(&path)
}

/// 批量元数据；rayon 并行的理由见 `filesystem::scan_content_tree`
#[command]
pub async fn get_tracks_metadata_batch(paths: Vec<String>) -> Vec<TrackMetadata> {
    tauri::async_runtime::spawn_blocking(move || {
        use rayon::prelude::*;
        paths
            .par_iter()
            .filter_map(|path| {
                get_track_metadata_internal(path)
                    .map_err(|e| log::warn!("批量元数据提取失败 {path}: {e}"))
                    .ok()
            })
            .collect()
    })
    .await
    .unwrap_or_else(|e| {
        log::error!("批量元数据提取任务失败（返回空列表）: {e}");
        Vec::new()
    })
}

#[command]
pub fn get_track_cover_path(path: String) -> Result<Option<String>, AppError> {
    get_track_cover_path_internal(&path)
}

/// 识别一批曲目的 BPM（标签优先，缺标签时解码音频）。
/// 前端只传当前播放的一首；一律串行，避免与播放解码抢 CPU。
#[command]
pub async fn analyze_tracks_bpm(paths: Vec<String>) -> Result<Vec<BpmAnalysisResult>, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut results = Vec::with_capacity(paths.len());
        for path in paths {
            let bpm = bpm::resolve(&path);
            results.push(BpmAnalysisResult { path, bpm });
            // 让出一次,把 CPU 让给播放线程与其它阻塞任务
            std::thread::yield_now();
        }
        if let Err(e) = bpm::flush_bpm_cache() {
            log::warn!("BPM 分析缓存落盘失败: {e}");
        }
        results
    })
    .await
    .map_err(|e| AppError::msg(format!("BPM 分析任务异常退出: {e}")))
}

#[command]
pub fn extract_cover(audio_path: String, output_path: String) -> Result<String, AppError> {
    extract_cover_internal(&audio_path, &output_path)
}

#[command]
pub fn clean_cover_cache_command(max_cache_size_mb: Option<u64>) -> Result<usize, AppError> {
    clean_cover_cache(max_cache_size_mb)
}

#[command]
pub fn set_cover_cache_path_command(path: Option<String>) -> Result<(), AppError> {
    set_cover_cache_path(path)
}

#[command]
pub fn clear_metadata_cache_command() -> Result<(), AppError> {
    clear_metadata_cache()
}

#[command]
pub fn get_metadata_cache_stats_command() -> (usize, u64) {
    get_metadata_cache_stats()
}

#[command]
pub fn flush_metadata_cache_command() -> Result<(), AppError> {
    flush_metadata_cache()
}

/// 系统临时目录；未设置封面缓存路径时设置页展示的就是它
#[command]
pub fn get_temp_dir_command() -> String {
    std::env::temp_dir().to_string_lossy().to_string()
}

/// 调起系统目录选择器（Android SAF），仅移动端生效；桌面端为 no-op
#[command]
pub fn saf_request_pick() -> Result<(), AppError> {
    crate::android::saf::request_pick_directory()
}

/// 获取已保存的音乐目录树 URI（Android SAF；桌面端返回 None）
#[command]
pub fn saf_get_saved_tree() -> Result<Option<String>, AppError> {
    crate::android::saf::get_saved_tree_uri()
}

/// SAF 授权状态；前端轮询 `version` 判断选择器是否已返回：重新授权同一目录时 URI 不变，只有版本递增。
#[command]
pub fn saf_get_pick_state() -> Result<crate::android::saf::SafPickState, AppError> {
    crate::android::saf::get_pick_state()
}

/// 移除某个 SAF 目录时调用：释放该树的持久授权（并清掉残留记录，避免影响重新授权）
#[command]
pub fn saf_clear_saved_tree(uri: String) -> Result<(), AppError> {
    crate::android::saf::clear_saved_tree(&uri)
}

/// 节拍模型是否可用（模型文件就位）；前端据此决定是否启用后台精算。
#[command]
pub fn beat_model_available() -> bool {
    beat_model::is_available()
}

/// 预估后端 `directml` / `rten`；只查运行库是否存在，不加载模型。
#[command]
pub fn beat_backend() -> &'static str {
    beat_model::backend_hint()
}

/// 实际后端标签 `directml` / `onnxruntime-cpu` / `rten-cpu`；未分析过时返回探测值，空串会被前端当成 CPU。
#[command]
pub fn beat_backend_actual() -> &'static str {
    beat_model::backend_label()
}

/// 可选组件（模型 + GPU 运行时）的安装状态。二者不随安装包分发。
#[command]
pub fn beat_component_status() -> beat_model::ComponentStatus {
    beat_model::component_status()
}

#[command]
pub fn open_beat_component_dir() -> Result<(), AppError> {
    let dir = beat_model::component_dir()
        .ok_or_else(|| AppError::Audio("无法定位组件目录".to_string()))?;
    for sub in ["models", "runtime"] {
        std::fs::create_dir_all(dir.join(sub))
            .map_err(|e| AppError::Audio(format!("无法创建组件目录: {e}")))?;
    }
    tauri_plugin_opener::reveal_item_in_dir(&dir)
        .map_err(|e| AppError::Audio(format!("无法打开目录: {e}")))?;
    Ok(())
}

/// 整曲节拍跟踪；单首 6–7 秒，只应由后台任务调用，失败时调用方降级到 `bpm::resolve`。
#[command]
pub async fn analyze_beats(path: String) -> Result<beat_model::BeatTrack, AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(track) = beat_cache::cached(&path) {
            return Ok(track);
        }
        let track = beat_model::analyze(&path)?;
        beat_cache::store(&path, &track);
        // 同步写回元数据缓存：界面读的是 `metadata.bpm`，只写 beat-cache 不会显示变化
        super::metadata::cache::patch_cached_bpm(&path, track.bpm);
        if let Err(e) = beat_cache::flush() {
            log::warn!("节拍缓存落盘失败: {e}");
        }
        if let Err(e) = flush_metadata_cache() {
            log::warn!("元数据缓存落盘失败: {e}");
        }
        Ok(track)
    })
    .await
    .map_err(|e| AppError::Audio(format!("节拍分析任务异常: {e}")))?
}

/// 批量分析进度事件名
pub const BEATS_PROGRESS_EVENT: &str = "beats-analysis-progress";

/// 批量分析的单步进度
#[derive(Debug, Clone, Serialize)]
pub struct BeatsProgress {
    /// 已完成的曲目数（含跳过）；`started` 状态下是这一首之前的完成数
    pub done: usize,
    pub total: usize,
    /// 当前曲目文件名（`started` 时就是正在分析的那首）
    pub current: String,
    /// 当前曲目完整路径（文件名可能重名）
    pub path: String,
    /// 失败、跳过或未算完时为 `None`
    pub bpm: Option<f64>,
    /// started / analyzed / skipped / missing / failed / finished / cancelled
    pub status: String,
    /// 这一首实际耗时（ms），`started` 与 `finished` 为 0；前端据此按实测速度估算剩余时间
    pub track_ms: u64,
}

/// 一类失败原因及其数量
#[derive(Debug, Clone, Serialize)]
pub struct BeatIssue {
    pub message: String,
    pub count: usize,
}

/// 批量分析汇总
#[derive(Debug, Clone, Serialize)]
pub struct BeatsBatchSummary {
    pub analyzed: usize,
    pub skipped: usize,
    pub failed: usize,
    pub cancelled: bool,
    /// 按原因归并的失败/跳过明细，避免只报一条样本把"文件不存在"误报成算法问题
    pub issues: Vec<BeatIssue>,
}

/// 取消标志设为全局静态，才能让一个命令影响另一个正在执行的命令。
static BEATS_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 中止批量分析；已完成曲目的结果保留。
#[command]
pub fn cancel_beats_batch() {
    BEATS_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn record_issue(issues: &mut Vec<BeatIssue>, message: &str) {
    if let Some(found) = issues.iter_mut().find(|issue| issue.message == message) {
        found.count += 1;
    } else {
        issues.push(BeatIssue {
            message: message.to_string(),
            count: 1,
        });
    }
}

/// 批量整曲节拍分析；`force = false` 时跳过已有缓存的曲目，支持中断后继续，进度经 `BEATS_PROGRESS_EVENT` 上报。
#[command]
pub async fn analyze_beats_batch(
    app: tauri::AppHandle,
    paths: Vec<String>,
    force: bool,
) -> Result<BeatsBatchSummary, AppError> {
    BEATS_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let total = paths.len();
        let mut summary = BeatsBatchSummary {
            analyzed: 0,
            skipped: 0,
            failed: 0,
            cancelled: false,
            issues: Vec::new(),
        };
        for (index, path) in paths.iter().enumerate() {
            // 单首内部不可中断，最坏等当前这首算完
            if BEATS_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                summary.cancelled = true;
                let _ = app.emit(
                    BEATS_PROGRESS_EVENT,
                    BeatsProgress {
                        done: index,
                        total,
                        current: String::new(),
                        path: String::new(),
                        bpm: None,
                        status: "cancelled".to_string(),
                        track_ms: 0,
                    },
                );
                break;
            }
            let name = std::path::Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());

            // 文件不存在不算失败（连输入都没有）
            if !check_file_exists_internal(path) {
                summary.skipped += 1;
                record_issue(&mut summary.issues, "曲目文件不存在（可能已被移动或删除）");
                let _ = app.emit(
                    BEATS_PROGRESS_EVENT,
                    BeatsProgress {
                        done: index + 1,
                        total,
                        current: name,
                        path: path.clone(),
                        bpm: None,
                        status: "missing".to_string(),
                        track_ms: 0,
                    },
                );
                continue;
            }

            if !force && beat_cache::cached(path).is_some() {
                summary.skipped += 1;
                let _ = app.emit(
                    BEATS_PROGRESS_EVENT,
                    BeatsProgress {
                        done: index + 1,
                        total,
                        current: name,
                        path: path.clone(),
                        bpm: None,
                        status: "skipped".to_string(),
                        track_ms: 0,
                    },
                );
                continue;
            }

            // 先报 started 再动手：单首要 6–9 秒，只在算完后发事件会让界面长时间无反馈
            let _ = app.emit(
                BEATS_PROGRESS_EVENT,
                BeatsProgress {
                    done: index,
                    total,
                    current: name.clone(),
                    path: path.clone(),
                    bpm: None,
                    status: "started".to_string(),
                    track_ms: 0,
                },
            );

            let started_at = std::time::Instant::now();
            let outcome = beat_model::analyze(path);
            let track_ms = started_at.elapsed().as_millis() as u64;
            match outcome {
                Ok(track) => {
                    beat_cache::store(path, &track);
                    super::metadata::cache::patch_cached_bpm(path, track.bpm);
                    summary.analyzed += 1;
                    let _ = app.emit(
                        BEATS_PROGRESS_EVENT,
                        BeatsProgress {
                            done: index + 1,
                            total,
                            current: name,
                            path: path.clone(),
                            bpm: track.bpm,
                            status: "analyzed".to_string(),
                            track_ms,
                        },
                    );
                }
                Err(e) => {
                    summary.failed += 1;
                    record_issue(&mut summary.issues, &format!("{e}"));
                    let _ = app.emit(
                        BEATS_PROGRESS_EVENT,
                        BeatsProgress {
                            done: index + 1,
                            total,
                            current: name,
                            path: path.clone(),
                            bpm: None,
                            status: "failed".to_string(),
                            track_ms,
                        },
                    );
                }
            }
            // 每首落一次盘：整库十几分钟，崩溃不该丢掉已算完的结果
            if let Err(e) = beat_cache::flush() {
                log::warn!("节拍缓存中途落盘失败: {e}");
            }
        }
        // 元数据缓存整批落一次盘：逐首落盘会让几十 MB 的文件被反复重写
        if let Err(e) = flush_metadata_cache() {
            log::warn!("批量分析结束后元数据缓存落盘失败: {e}");
        }
        // 多者排前：UI 只展示前几条
        summary.issues.sort_by(|a, b| b.count.cmp(&a.count));
        let _ = app.emit(
            BEATS_PROGRESS_EVENT,
            BeatsProgress {
                done: total,
                total,
                current: String::new(),
                path: String::new(),
                bpm: None,
                status: "finished".to_string(),
                track_ms: 0,
            },
        );
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Audio(format!("批量节拍分析任务异常: {e}")))?
}

/// 节拍缓存统计：有效曲目数、缓存文件占用（KB）
#[command]
pub fn beat_cache_stats() -> (usize, u64) {
    (beat_cache::valid_count(), beat_cache::cache_size_kb())
}
