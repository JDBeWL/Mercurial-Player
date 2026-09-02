//! 元数据缓存持久化。

use crate::error::AppError;
use crate::security::is_sensitive_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::UNIX_EPOCH;

use super::extractor::TrackMetadata;

/// 缓存的元数据条目
#[derive(Debug, Serialize, Deserialize, Clone)]
struct CachedMetadata {
    #[serde(flatten)]
    metadata: TrackMetadata,
    /// 文件最后修改时间（Unix 时间戳）
    modified_time: u64,
    /// 缓存创建时间
    cached_at: u64,
}

/// 元数据缓存
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct MetadataCache {
    /// 版本号，用于缓存格式升级
    version: u32,
    /// 缓存条目，key 为文件路径
    entries: std::collections::HashMap<String, CachedMetadata>,
}

const CACHE_VERSION: u32 = 1;
const METADATA_CACHE_FILENAME: &str = "metadata-cache.json";

/// 获取元数据缓存文件路径
fn metadata_cache_path() -> PathBuf {
    // 优先使用自定义缓存路径
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join(METADATA_CACHE_FILENAME);
    }
    // 默认使用系统临时目录
    std::env::temp_dir()
        .join("mercurial-player")
        .join(METADATA_CACHE_FILENAME)
}

/// 加载元数据缓存
fn load_metadata_cache() -> MetadataCache {
    let cache_path = metadata_cache_path();
    if !cache_path.exists() {
        return MetadataCache {
            version: CACHE_VERSION,
            entries: std::collections::HashMap::new(),
        };
    }

    match fs::read_to_string(&cache_path) {
        Ok(content) => match serde_json::from_str::<MetadataCache>(&content) {
            Ok(cache) if cache.version == CACHE_VERSION => cache,
            Ok(_) => {
                log::info!("元数据缓存版本不匹配，重新创建");
                MetadataCache {
                    version: CACHE_VERSION,
                    entries: std::collections::HashMap::new(),
                }
            }
            Err(e) => {
                log::warn!("加载元数据缓存失败: {e}, 将重新创建");
                MetadataCache {
                    version: CACHE_VERSION,
                    entries: std::collections::HashMap::new(),
                }
            }
        },
        Err(e) => {
            log::warn!("读取元数据缓存文件失败: {e}, 将重新创建");
            MetadataCache {
                version: CACHE_VERSION,
                entries: std::collections::HashMap::new(),
            }
        }
    }
}

/// 保存元数据缓存
fn save_metadata_cache(cache: &MetadataCache) -> Result<(), AppError> {
    let cache_path = metadata_cache_path();

    // 确保父目录存在
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }

    let content =
        serde_json::to_string_pretty(cache).map_err(|e| format!("序列化缓存失败: {e}"))?;

    fs::write(&cache_path, content).map_err(|e| format!("写入缓存文件失败: {e}"))?;

    Ok(())
}

/// 获取文件的修改时间
fn get_file_modified_time(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// 从缓存获取元数据（如果文件未修改）
pub fn get_metadata_from_cache(path: &str) -> Option<TrackMetadata> {
    let cached = get_cached_entry(path)?;

    // 检查文件是否被修改
    let current_modified = get_file_modified_time(Path::new(path))?;
    if current_modified != cached.modified_time {
        log::debug!("文件已修改，缓存失效: {path}");
        return None;
    }

    log::debug!("缓存命中: {path}");

    Some(cached.metadata)
}

/// 将元数据保存到缓存（仅写入内存，由 flush_metadata_cache 统一持久化）
pub fn save_metadata_to_cache(path: &str, metadata: &TrackMetadata) {
    save_metadata_to_memory_cache(path, metadata);
    log::debug!("元数据已缓存: {path}");
}

/// 清理元数据缓存中不存在的文件
pub fn clean_metadata_cache() -> Result<usize, AppError> {
    let mut cache = load_metadata_cache();
    let original_count = cache.entries.len();

    // 移除不存在的文件
    cache.entries.retain(|path, _| Path::new(path).exists());

    let removed_count = original_count - cache.entries.len();

    if removed_count > 0 {
        save_metadata_cache(&cache)?;
        log::info!("清理了 {removed_count} 个无效的元数据缓存条目");
    }

    Ok(removed_count)
}

/// 清除所有元数据缓存
pub fn clear_metadata_cache() -> Result<(), AppError> {
    let cache_path = metadata_cache_path();
    if cache_path.exists() {
        fs::remove_file(&cache_path).map_err(|e| format!("删除缓存文件失败: {e}"))?;
    }
    log::info!("元数据缓存已清除");
    Ok(())
}

/// 获取缓存统计信息
pub fn get_metadata_cache_stats() -> (usize, u64) {
    let cache = load_metadata_cache();
    let entry_count = cache.entries.len();

    let total_size = cache
        .entries
        .values()
        .map(|e| size_of_val(&e.metadata) as u64)
        .sum();

    (entry_count, total_size)
}

// 全局自定义缓存路径
static CUSTOM_CACHE_PATH: RwLock<Option<String>> = RwLock::new(None);

// 全局内存缓存，用于批量保存（读多写少，用 RwLock）
static MEMORY_CACHE: RwLock<Option<MetadataCache>> = RwLock::new(None);

/// 从内存缓存中查询单条记录（只克隆单条，不克隆整个 HashMap）
fn get_cached_entry(path: &str) -> Option<CachedMetadata> {
    // 快速路径：读锁查询
    if let Ok(lock) = MEMORY_CACHE.read() {
        if let Some(cache) = lock.as_ref() {
            if let Some(entry) = cache.entries.get(path) {
                return Some(entry.clone());
            }
            return None;
        }
    }
    // 慢速路径：内存缓存未初始化，从磁盘加载
    let cache = load_metadata_cache();
    let result = cache.entries.get(path).cloned();
    if let Ok(mut lock) = MEMORY_CACHE.write() {
        if lock.is_none() {
            *lock = Some(cache);
        }
    }
    result
}

/// 将内存缓存持久化到磁盘（在锁外执行 I/O，不阻塞其他读者）
fn flush_memory_cache() -> Result<(), AppError> {
    // 读锁下克隆缓存快照，然后释放锁
    let snapshot = {
        let lock = lock_or_log!(MEMORY_CACHE.read());
        lock.as_ref().map(|cache| cache.clone())
    };
    // 在锁外执行磁盘 I/O
    if let Some(cache) = snapshot {
        return save_metadata_cache(&cache);
    }
    Ok(())
}

/// 保存元数据到内存缓存（不立即写入磁盘）
pub fn save_metadata_to_memory_cache(path: &str, metadata: &TrackMetadata) {
    if let Some(modified_time) = get_file_modified_time(Path::new(path)) {
        let cached = CachedMetadata {
            metadata: metadata.clone(),
            modified_time,
            cached_at: std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let mut lock = lock_or_log!(MEMORY_CACHE.write());
        if lock.is_none() {
            *lock = Some(load_metadata_cache());
        }
        if let Some(cache) = lock.as_mut() {
            cache.entries.insert(path.to_string(), cached);
            log::debug!("元数据已加入内存缓存: {path}");
        }
    }
}

/// 批量保存内存缓存到磁盘
pub fn flush_metadata_cache() -> Result<(), AppError> {
    flush_memory_cache()
}

/// 设置自定义封面缓存路径
///
/// 校验路径合法性，防止缓存清理逻辑被引导到敏感目录执行任意删除
pub fn set_cover_cache_path(path: Option<String>) -> Result<(), AppError> {
    if let Some(p) = &path {
        if p.is_empty() {
            return Err("缓存路径不能为空".to_string().into());
        }
        if is_sensitive_path(p) {
            return Err("安全限制：不允许使用敏感目录作为缓存路径"
                .to_string()
                .into());
        }
    }

    let mut cache_path = lock_or_log!(CUSTOM_CACHE_PATH.write());
    *cache_path = path;
    drop(cache_path);
    Ok(())
}

/// 获取自定义封面缓存路径
pub fn get_cover_cache_path_setting() -> Option<String> {
    lock_or_log!(CUSTOM_CACHE_PATH.read()).clone()
}

pub(super) fn cover_cache_dir() -> PathBuf {
    // 优先使用自定义缓存路径
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join("cover-cache");
    }
    // 默认使用系统临时目录
    std::env::temp_dir()
        .join("mercurial-player")
        .join("cover-cache")
}

// ============================================================================
// 缓存清理配置
// ============================================================================

/// 默认缓存最大大小（1GB）
const DEFAULT_MAX_CACHE_SIZE_MB: u64 = 1024;

/// 缓存过期时间（30天，单位：秒）
const CACHE_EXPIRE_SECONDS: u64 = 30 * 24 * 60 * 60;

/// 缓存文件信息
struct CacheFileInfo {
    path: PathBuf,
    size: u64,
    last_accessed: u64,
}

/// 清理过期的缓存文件
fn clean_expired_cache_files() -> Result<usize, AppError> {
    let cache_dir = cover_cache_dir();

    if !cache_dir.exists() {
        return Ok(0);
    }

    let mut cleaned_count = 0;
    let current_time = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("获取系统时间失败: {e}"))?
        .as_secs();

    let entries = fs::read_dir(&cache_dir).map_err(|e| format!("读取缓存目录失败: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {e}"))?;
        let path = entry.path();

        if path.is_file() {
            if let Ok(metadata) = fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        let file_age = current_time.saturating_sub(duration.as_secs());

                        if file_age > CACHE_EXPIRE_SECONDS {
                            if fs::remove_file(&path).is_ok() {
                                cleaned_count += 1;
                                log::debug!("删除过期缓存文件: {}", path.display());
                            }
                        }
                    }
                }
            }
        }
    }

    if cleaned_count > 0 {
        log::info!("清理了 {cleaned_count} 个过期缓存文件");
    }

    Ok(cleaned_count)
}

/// 获取缓存文件列表，按最后访问时间排序（最旧的在前）
fn get_cache_files_sorted() -> Result<Vec<CacheFileInfo>, AppError> {
    let cache_dir = cover_cache_dir();
    let mut files = Vec::new();

    if !cache_dir.exists() {
        return Ok(files);
    }

    let entries = fs::read_dir(&cache_dir).map_err(|e| format!("读取缓存目录失败: {e}"))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {e}"))?;
        let path = entry.path();

        if path.is_file() {
            if let Ok(metadata) = fs::metadata(&path) {
                let size = metadata.len();
                let last_accessed = metadata
                    .accessed()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0_u64, |d| d.as_secs());

                files.push(CacheFileInfo {
                    path,
                    size,
                    last_accessed,
                });
            }
        }
    }

    // 按最后访问时间排序，最旧的在前
    files.sort_by_key(|a| a.last_accessed);

    Ok(files)
}

/// 清理超出大小限制的缓存文件
fn clean_cache_by_size(max_cache_size_mb: u64) -> Result<usize, AppError> {
    let max_cache_size_bytes = max_cache_size_mb * 1024 * 1024;
    let mut files = get_cache_files_sorted()?;
    let mut total_size: u64 = files.iter().map(|f| f.size).sum();
    let mut cleaned_count = 0;

    while total_size > max_cache_size_bytes && !files.is_empty() {
        let file = files.remove(0);
        if fs::remove_file(&file.path).is_ok() {
            total_size = total_size.saturating_sub(file.size);
            cleaned_count += 1;
            log::debug!("删除缓存文件以控制大小: {}", file.path.display());
        }
    }

    if cleaned_count > 0 {
        log::info!("清理了 {cleaned_count} 个缓存文件以控制大小");
    }

    Ok(cleaned_count)
}

/// 清理封面缓存（综合清理策略）
///
/// 执行以下清理操作：
/// 1. 删除超过 30 天未使用的文件
/// 2. 删除超出大小限制的文件（默认 1GB，可配置）
///
/// # Arguments
/// * max_cache_size_mb - 最大缓存大小（单位：MB），如果为 None 则使用默认值 1GB
pub fn clean_cover_cache(max_cache_size_mb: Option<u64>) -> Result<usize, AppError> {
    let max_size = max_cache_size_mb.unwrap_or(DEFAULT_MAX_CACHE_SIZE_MB);
    log::info!("开始清理封面缓存（最大大小: {max_size}MB）...");

    let mut total_cleaned = 0;

    // 清理过期文件
    total_cleaned += clean_expired_cache_files().unwrap_or(0);

    // 清理超出大小限制的文件
    total_cleaned += clean_cache_by_size(max_size).unwrap_or(0);

    if total_cleaned > 0 {
        log::info!("封面缓存清理完成，共删除 {total_cleaned} 个文件");
    } else {
        log::debug!("封面缓存无需清理");
    }

    Ok(total_cleaned)
}
