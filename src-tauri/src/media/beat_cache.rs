//! 节拍分析结果的持久化缓存
//!
//! 单独成文件：拍点数组有数 MB，混进 `bpm-cache.json` 会让每次查 BPM 都反序列化它们。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

use super::beat_model::{BeatTrack, MeterCount};
use super::metadata::cache::{cache_dir, get_file_modified_time};

const BEAT_CACHE_FILENAME: &str = "beat-cache.json";

/// 缓存格式版本。换模型或改拍点后处理时必须递增，否则新旧结果无法归因。
const BEAT_CACHE_VERSION: u32 = 1;

/// 单条缓存。拍点用 `f32`：毫秒级精度足够同步，体积是 `f64` 的一半。
#[derive(Debug, Serialize, Deserialize, Clone)]
struct BeatEntry {
    modified_time: u64,
    bpm: Option<f64>,
    beats: Vec<f32>,
    downbeats: Vec<f32>,
    /// `[每小节拍数, 出现次数]`
    meter: Vec<[u32; 2]>,
    span_secs: f32,
    analyzed_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BeatCache {
    version: u32,
    entries: HashMap<String, BeatEntry>,
}

impl BeatCache {
    fn empty() -> Self {
        Self {
            version: BEAT_CACHE_VERSION,
            entries: HashMap::new(),
        }
    }
}

static BEAT_CACHE: RwLock<Option<BeatCache>> = RwLock::new(None);

fn beat_cache_path() -> PathBuf {
    cache_dir().join(BEAT_CACHE_FILENAME)
}

fn load_from_disk() -> BeatCache {
    let path = beat_cache_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return BeatCache::empty();
    };
    match serde_json::from_str::<BeatCache>(&content) {
        Ok(cache) if cache.version == BEAT_CACHE_VERSION => cache,
        Ok(cache) => {
            log::info!(
                "节拍缓存版本 {} 与当前 {BEAT_CACHE_VERSION} 不符，全部作废",
                cache.version
            );
            BeatCache::empty()
        }
        Err(e) => {
            log::warn!("节拍缓存解析失败，按空缓存处理: {e}");
            BeatCache::empty()
        }
    }
}

/// 取缓存的节拍结果；未分析过或文件已变时返回 `None`
#[must_use]
pub fn cached(path: &str) -> Option<BeatTrack> {
    let current = get_file_modified_time(path)?;
    let mut guard = BEAT_CACHE.write().ok()?;
    if guard.is_none() {
        *guard = Some(load_from_disk());
    }
    let cache = guard.as_mut()?;
    let entry = cache.entries.get(path)?;
    if entry.modified_time != current {
        // 文件已变：旧拍点对不上音频，删掉等重算
        cache.entries.remove(path);
        return None;
    }
    let track = BeatTrack {
        bpm: entry.bpm,
        beats: entry.beats.iter().map(|&v| f64::from(v)).collect(),
        downbeats: entry.downbeats.iter().map(|&v| f64::from(v)).collect(),
        meter: entry
            .meter
            .iter()
            .map(|[beats_per_bar, count]| MeterCount {
                beats_per_bar: *beats_per_bar,
                count: *count,
            })
            .collect(),
        span_secs: f64::from(entry.span_secs),
    };
    // 拍点数组可能有数 MB，拷贝期间没必要一直占着全局写锁
    drop(guard);
    Some(track)
}

/// 最近一次分析时间（用于 UI 判断"是否分析过整库"）
#[must_use]
pub fn cached_at(path: &str) -> Option<u64> {
    let current = get_file_modified_time(path)?;
    let guard = BEAT_CACHE.read().ok()?;
    let cache = guard.as_ref()?;
    let entry = cache.entries.get(path)?;
    let analyzed_at = (entry.modified_time == current).then_some(entry.analyzed_at);
    drop(guard);
    analyzed_at
}

/// 已缓存且仍然有效的曲目数
#[must_use]
pub fn valid_count() -> usize {
    // 用写锁而非读锁：首次调用时内存是 `None`，需借此加载磁盘缓存，否则设置页一直显示"已分析 0 首"。
    let mut guard = match BEAT_CACHE.write() {
        Ok(guard) => guard,
        Err(_) => return 0,
    };
    if guard.is_none() {
        *guard = Some(load_from_disk());
    }
    // 逐首 stat 一遍可能很慢（整库），统计完立刻放锁，别把写锁按住那么久
    let count = guard.as_ref().map_or(0, |cache| {
        cache
            .entries
            .iter()
            .filter(|(path, entry)| get_file_modified_time(path) == Some(entry.modified_time))
            .count()
    });
    drop(guard);
    count
}

/// 写入一条结果，不落盘；批量结束后由调用方 `flush`
pub fn store(path: &str, track: &BeatTrack) {
    let Some(modified_time) = get_file_modified_time(path) else {
        return;
    };
    let analyzed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = BeatEntry {
        modified_time,
        bpm: track.bpm,
        beats: track.beats.iter().map(|&v| v as f32).collect(),
        downbeats: track.downbeats.iter().map(|&v| v as f32).collect(),
        meter: track
            .meter
            .iter()
            .map(|m| [m.beats_per_bar, m.count])
            .collect(),
        span_secs: track.span_secs as f32,
        analyzed_at,
    };
    let mut guard = match BEAT_CACHE.write() {
        Ok(guard) => guard,
        Err(e) => {
            log::error!("节拍缓存写锁失效: {e}");
            return;
        }
    };
    if guard.is_none() {
        *guard = Some(load_from_disk());
    }
    if let Some(cache) = guard.as_mut() {
        cache.entries.insert(path.to_string(), entry);
    }
}

/// 只存 BPM 不存拍点，避免为已有可信 BPM 的曲目再跑模型；`beats` 为空即"有 BPM 无拍点"。
pub fn store_bpm_only(path: &str, bpm: f64) {
    let track = BeatTrack {
        bpm: Some(bpm),
        beats: Vec::new(),
        downbeats: Vec::new(),
        meter: Vec::new(),
        span_secs: 0.0,
    };
    store(path, &track);
}

pub fn flush() -> Result<(), AppError> {
    let snapshot = {
        let lock = match BEAT_CACHE.read() {
            Ok(lock) => lock,
            Err(e) => return Err(AppError::msg(format!("节拍缓存读锁失效: {e}"))),
        };
        lock.clone()
    };
    let Some(cache) = snapshot else {
        return Ok(());
    };
    let path = beat_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::msg(format!("创建节拍缓存目录失败: {e}")))?;
    }
    let content = serde_json::to_string(&cache)
        .map_err(|e| AppError::msg(format!("序列化节拍缓存失败: {e}")))?;
    fs::write(&path, content).map_err(|e| AppError::msg(format!("写入节拍缓存失败: {e}")))?;
    Ok(())
}

/// 缓存文件占用（KB）
#[must_use]
pub fn cache_size_kb() -> u64 {
    fs::metadata(beat_cache_path()).map_or(0, |m| m.len() / 1024)
}
