//! 元数据缓存：磁盘 JSON 持久化 + 进程内 `MEMORY_CACHE`

use crate::error::AppError;
use crate::security::is_sensitive_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::UNIX_EPOCH;

use super::extractor::TrackMetadata;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CachedMetadata {
    #[serde(flatten)]
    metadata: TrackMetadata,
    /// 文件最后修改时间（Unix 时间戳）
    modified_time: u64,
    /// 缓存创建时间（Unix 时间戳）
    cached_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct MetadataCache {
    /// 版本号，用于缓存格式升级
    version: u32,
    /// 缓存条目，key 为文件路径
    entries: std::collections::HashMap<String, CachedMetadata>,
}

const CACHE_VERSION: u32 = 1;
const METADATA_CACHE_FILENAME: &str = "metadata-cache.json";

/// 元数据缓存文件路径（缓存根目录下的 metadata-cache.json）
fn metadata_cache_path() -> PathBuf {
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join(METADATA_CACHE_FILENAME);
    }
    std::env::temp_dir()
        .join("mercurial-player")
        .join(METADATA_CACHE_FILENAME)
}

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

fn save_metadata_cache(cache: &MetadataCache) -> Result<(), AppError> {
    let cache_path = metadata_cache_path();

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }

    let content =
        serde_json::to_string_pretty(cache).map_err(|e| format!("序列化缓存失败: {e}"))?;

    fs::write(&cache_path, content).map_err(|e| format!("写入缓存文件失败: {e}"))?;

    Ok(())
}

/// 获取文件的修改时间（content URI 没有可 stat 的路径，从 SAF fd 上取）
fn get_file_modified_time(path: &str) -> Option<u64> {
    let metadata = if crate::android::saf::is_content_uri(path) {
        crate::android::saf::open_media_file(path)
            .ok()?
            .metadata()
            .ok()?
    } else {
        fs::metadata(path).ok()?
    };
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// 从缓存获取元数据（如果文件未修改）
pub fn get_metadata_from_cache(path: &str) -> Option<TrackMetadata> {
    let cached = get_cached_entry(path)?;

    let current_modified = get_file_modified_time(path)?;
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

fn empty_metadata_cache() -> MetadataCache {
    MetadataCache {
        version: CACHE_VERSION,
        entries: std::collections::HashMap::new(),
    }
}

/// 清空内存缓存。内存缓存是写盘的权威来源：只删磁盘不清内存，下一次 flush 会把旧条目写回
/// （锁中毒也要真的清掉，所以用 `lock_or_log!` 而不是 `if let Ok`）
fn clear_memory_cache() {
    let mut lock = lock_or_log!(MEMORY_CACHE.write());
    *lock = Some(empty_metadata_cache());
}

/// 清理元数据缓存中不存在的文件
pub fn clean_metadata_cache() -> Result<usize, AppError> {
    // 锁内只取 key：exists() 是文件 I/O 不放锁内；以内存为准的理由见 clear_memory_cache
    let cached_keys = {
        let lock = lock_or_log!(MEMORY_CACHE.read());
        lock.as_ref()
            .map(|cache| cache.entries.keys().cloned().collect::<Vec<String>>())
    };
    let keys =
        cached_keys.unwrap_or_else(|| load_metadata_cache().entries.keys().cloned().collect());

    // 必须用带 content URI 分支的判断：按本地路径 exists 会把所有 SAF 条目误判为已删除
    let stale: Vec<String> = keys
        .into_iter()
        .filter(|path| !crate::media::filesystem::check_file_exists_internal(path))
        .collect();

    if stale.is_empty() {
        return Ok(0);
    }

    let removed_count = stale.len();

    // 写盘基底取内存快照(克隆在锁外);期间新增的条目仍留在内存,由后续 flush 落盘
    let snapshot = {
        let cached = lock_or_log!(MEMORY_CACHE.read()).clone();
        if let Some(mut cache) = cached {
            // 内存与磁盘一起删，见 clear_memory_cache
            for path in &stale {
                cache.entries.remove(path);
            }
            {
                let mut guard = lock_or_log!(MEMORY_CACHE.write());
                if let Some(mem) = guard.as_mut() {
                    for path in &stale {
                        mem.entries.remove(path);
                    }
                }
            }
            cache
        } else {
            // 内存缓存未初始化:以磁盘内容为基底,不在这里初始化它
            let mut from_disk = load_metadata_cache();
            for path in &stale {
                from_disk.entries.remove(path);
            }
            from_disk
        }
    };
    save_metadata_cache(&snapshot)?;

    log::info!("清理了 {removed_count} 个无效的元数据缓存条目");

    Ok(removed_count)
}

pub fn clear_metadata_cache() -> Result<(), AppError> {
    // 先清内存:避免清理期间并发写入的条目在清完磁盘后又把旧数据 flush 回去
    clear_memory_cache();

    let cache_path = metadata_cache_path();
    if cache_path.exists() {
        fs::remove_file(&cache_path).map_err(|e| format!("删除缓存文件失败: {e}"))?;
    }
    log::info!("元数据缓存已清除");
    Ok(())
}

/// 缓存统计 (条目数, 磁盘占用字节数)：占用取缓存文件大小，清理针对的就是这个文件
pub fn get_metadata_cache_stats() -> (usize, u64) {
    let cache_path = metadata_cache_path();
    let total_size = fs::metadata(&cache_path).map(|m| m.len()).unwrap_or(0);
    let entry_count = load_metadata_cache().entries.len();

    (entry_count, total_size)
}

// 用户自定义的缓存根目录；未设置时各缓存落系统临时目录（取用处见 metadata_cache_path/cover_cache_dir）
static CUSTOM_CACHE_PATH: RwLock<Option<String>> = RwLock::new(None);

// 内存缓存，读多写少故用 RwLock；它相对磁盘文件的权威地位见 clear_memory_cache
static MEMORY_CACHE: RwLock<Option<MetadataCache>> = RwLock::new(None);

/// 从内存缓存中查询单条记录（只克隆单条，不克隆整个 HashMap）
fn get_cached_entry(path: &str) -> Option<CachedMetadata> {
    if let Ok(lock) = MEMORY_CACHE.read() {
        if let Some(cache) = lock.as_ref() {
            if let Some(entry) = cache.entries.get(path) {
                return Some(entry.clone());
            }
            return None;
        }
    }
    // 只有内存缓存尚未初始化时才回磁盘读，并顺手回填
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
    let snapshot = {
        let lock = lock_or_log!(MEMORY_CACHE.read());
        lock.as_ref().map(|cache| cache.clone())
    };
    if let Some(cache) = snapshot {
        return save_metadata_cache(&cache);
    }
    Ok(())
}

pub fn save_metadata_to_memory_cache(path: &str, metadata: &TrackMetadata) {
    if let Some(modified_time) = get_file_modified_time(path) {
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

pub fn flush_metadata_cache() -> Result<(), AppError> {
    flush_memory_cache()
}

/// 设置自定义封面缓存路径：清理会在该目录批量删文件，路径被引导就是任意删除，故必须挡敏感目录
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

pub fn get_cover_cache_path_setting() -> Option<String> {
    lock_or_log!(CUSTOM_CACHE_PATH.read()).clone()
}

/// 封面缓存目录（自定义根目录下的 cover-cache 子目录）
pub(super) fn cover_cache_dir() -> PathBuf {
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join("cover-cache");
    }
    std::env::temp_dir()
        .join("mercurial-player")
        .join("cover-cache")
}

// 缓存清理配置

const DEFAULT_MAX_CACHE_SIZE_MB: u64 = 1024;

/// 缓存过期时间（30天，单位：秒）
const CACHE_EXPIRE_SECONDS: u64 = 30 * 24 * 60 * 60;

struct CacheFileInfo {
    path: PathBuf,
    size: u64,
    last_accessed: u64,
}

/// 封面缓存文件的形状：`<十进制哈希>.<封面扩展名>`（写入端见 cover.rs）
///
/// 缓存根目录可由用户指到任意非敏感路径，清理若不加文件名判别就等于"该目录里任意文件的批量删除"。
fn is_cover_cache_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "gif" | "webp" | "bmp" | "jpg"
    ) && !stem.is_empty()
        && stem.bytes().all(|b| b.is_ascii_digit())
}

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

        if path.is_file() && is_cover_cache_file(&path) {
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

        if path.is_file() && is_cover_cache_file(&path) {
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

    files.sort_by_key(|a| a.last_accessed);

    Ok(files)
}

fn clean_cache_by_size(max_cache_size_mb: u64) -> Result<usize, AppError> {
    let max_cache_size_bytes = max_cache_size_mb * 1024 * 1024;
    // get_cache_files_sorted 已按时间升序，顺序消费即可
    let files = get_cache_files_sorted()?;
    let mut total_size: u64 = files.iter().map(|f| f.size).sum();
    let mut cleaned_count = 0;

    for file in files {
        if total_size <= max_cache_size_bytes {
            break;
        }
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

/// 清理封面缓存：先删过期文件，再删超出大小限制的部分
///
/// `max_cache_size_mb` 单位 MB，为 None 时取 [`DEFAULT_MAX_CACHE_SIZE_MB`]。
pub fn clean_cover_cache(max_cache_size_mb: Option<u64>) -> Result<usize, AppError> {
    let max_size = max_cache_size_mb.unwrap_or(DEFAULT_MAX_CACHE_SIZE_MB);
    log::info!("开始清理封面缓存（最大大小: {max_size}MB）...");

    let mut total_cleaned = 0;

    total_cleaned += clean_expired_cache_files().unwrap_or(0);

    total_cleaned += clean_cache_by_size(max_size).unwrap_or(0);

    if total_cleaned > 0 {
        log::info!("封面缓存清理完成，共删除 {total_cleaned} 个文件");
    } else {
        log::debug!("封面缓存无需清理");
    }

    Ok(total_cleaned)
}
