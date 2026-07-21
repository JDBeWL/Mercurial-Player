//! 音频元数据模块
//!
//! 提供音轨元数据结构和处理函数。

use lofty::picture::Picture;
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

// ============================================================================
// 元数据缓存模块
// ============================================================================

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
    std::env::temp_dir().join("mercurial-player").join(METADATA_CACHE_FILENAME)
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
        Ok(content) => {
            match serde_json::from_str::<MetadataCache>(&content) {
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
            }
        }
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
fn save_metadata_cache(cache: &MetadataCache) -> Result<(), String> {
    let cache_path = metadata_cache_path();
    
    // 确保父目录存在
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建缓存目录失败: {e}"))?;
    }

    let content = serde_json::to_string_pretty(cache)
        .map_err(|e| format!("序列化缓存失败: {e}"))?;
    
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
    let cache = get_memory_cache();
    let cached = cache.entries.get(path)?;
    
    // 检查文件是否被修改
    let current_modified = get_file_modified_time(Path::new(path))?;
    if current_modified != cached.modified_time {
        log::debug!("文件已修改，缓存失效: {path}");
        return None;
    }
    
    log::debug!("缓存命中: {path}");
    
    Some(cached.metadata.clone())
}

/// 将元数据保存到缓存（立即写入磁盘，适合单条保存）
pub fn save_metadata_to_cache(path: &str, metadata: &TrackMetadata) {
    save_metadata_to_memory_cache(path, metadata);
    
    if let Err(e) = flush_memory_cache() {
        log::warn!("保存元数据缓存到磁盘失败: {e}");
    } else {
        log::debug!("元数据已缓存: {path}");
    }
}

/// 清理元数据缓存中不存在的文件
pub fn clean_metadata_cache() -> Result<usize, String> {
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
pub fn clear_metadata_cache() -> Result<(), String> {
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
    
    let total_size = cache.entries.values()
        .map(|e| size_of_val(&e.metadata) as u64)
        .sum();
    
    (entry_count, total_size)
}

// 全局自定义缓存路径
static CUSTOM_CACHE_PATH: Mutex<Option<String>> = Mutex::new(None);

// 全局内存缓存，用于批量保存
static MEMORY_CACHE: Mutex<Option<MetadataCache>> = Mutex::new(None);

/// 获取内存缓存（如果不存在则加载）
fn get_memory_cache() -> MetadataCache {
    if let Ok(lock) = MEMORY_CACHE.lock() {
        if let Some(cache) = lock.as_ref() {
            return cache.clone();
        }
    }
    let cache = load_metadata_cache();
    if let Ok(mut lock) = MEMORY_CACHE.lock() {
        *lock = Some(cache.clone());
    }
    cache
}

/// 将内存缓存持久化到磁盘
fn flush_memory_cache() -> Result<(), String> {
    if let Ok(lock) = MEMORY_CACHE.lock() {
        if let Some(cache) = lock.as_ref() {
            return save_metadata_cache(cache);
        }
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
        
        if let Ok(mut lock) = MEMORY_CACHE.lock() {
            if lock.is_none() {
                *lock = Some(load_metadata_cache());
            }
            if let Some(cache) = lock.as_mut() {
                cache.entries.insert(path.to_string(), cached);
                log::debug!("元数据已加入内存缓存: {path}");
            }
        }
    }
}

/// 批量保存内存缓存到磁盘
pub fn flush_metadata_cache() -> Result<(), String> {
    flush_memory_cache()
}

/// 设置自定义封面缓存路径
pub fn set_cover_cache_path(path: Option<String>) {
    if let Ok(mut cache_path) = CUSTOM_CACHE_PATH.lock() {
        *cache_path = path;
    }
}

/// 获取自定义封面缓存路径
pub fn get_cover_cache_path_setting() -> Option<String> {
    CUSTOM_CACHE_PATH.lock().ok().and_then(|p| p.clone())
}

/// 单个音轨的元数据
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackMetadata {
    pub path: String,
    pub name: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<f64>,
    pub cover_path: Option<String>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    pub format: Option<String>,
}

/// 包含多个音轨的播放列表
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub name: String,
    pub files: Vec<TrackMetadata>,
}

impl Playlist {
    #[must_use]
    pub const fn new(name: String) -> Self {
        Self { name, files: Vec::new() }
    }

    pub fn add_track(&mut self, track: TrackMetadata) {
        self.files.push(track);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

fn cover_cache_dir() -> PathBuf {
    // 优先使用自定义缓存路径
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join("cover-cache");
    }
    // 默认使用系统临时目录
    std::env::temp_dir().join("mercurial-player").join("cover-cache")
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
fn clean_expired_cache_files() -> Result<usize, String> {
    let cache_dir = cover_cache_dir();
    
    if !cache_dir.exists() {
        return Ok(0);
    }

    let mut cleaned_count = 0;
    let current_time = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("获取系统时间失败: {e}"))?
        .as_secs();

    let entries = fs::read_dir(&cache_dir)
        .map_err(|e| format!("读取缓存目录失败: {e}"))?;

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
fn get_cache_files_sorted() -> Result<Vec<CacheFileInfo>, String> {
    let cache_dir = cover_cache_dir();
    let mut files = Vec::new();

    if !cache_dir.exists() {
        return Ok(files);
    }

    let entries = fs::read_dir(&cache_dir)
        .map_err(|e| format!("读取缓存目录失败: {e}"))?;

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
fn clean_cache_by_size(max_cache_size_mb: u64) -> Result<usize, String> {
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
pub fn clean_cover_cache(max_cache_size_mb: Option<u64>) -> Result<usize, String> {
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

fn cover_extension_from_mime(mime: Option<&str>) -> &'static str {
    match mime {
        Some("image/png") => "png",
        Some("image/gif") => "gif",
        Some("image/webp") => "webp",
        Some("image/bmp") => "bmp",
        _ => "jpg",
    }
}

/// 获取封面数据的哈希值，用于识别相同的封面
fn get_picture_hash(picture: &Picture) -> u64 {
    let mut hasher = DefaultHasher::new();
    picture.data().hash(&mut hasher);
    hasher.finish()
}

fn get_cover_cache_path(_audio_path: &Path, picture: &Picture) -> Result<PathBuf, String> {
    // 使用封面数据哈希作为缓存键，相同封面的不同歌曲会共享缓存
    let hash = get_picture_hash(picture);
    let ext = cover_extension_from_mime(picture.mime_type().map(lofty::picture::MimeType::as_str));
    let cache_dir = cover_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| format!("无法创建封面缓存目录: {e}"))?;
    Ok(cache_dir.join(format!("{hash}.{ext}")))
}

fn extract_cover_to_cache(audio_path: &Path, picture: &Picture) -> Result<String, String> {
    let cache_file = get_cover_cache_path(audio_path, picture)?;
    log::debug!("Cache file path: {}", cache_file.display());

    if cache_file.exists() {
        log::debug!("Cache file already exists");
    } else {
        log::debug!("Cache file does not exist, writing new file");
        fs::write(&cache_file, picture.data()).map_err(|e| format!("写入封面缓存失败: {e}"))?;
        log::debug!("Cache file written successfully");
    }

    Ok(cache_file.to_string_lossy().to_string())
}

/// 获取音轨的元数据信息（内部函数）
///
/// 默认轻量模式：不提取封面，降低扫描负担，避免前端长时间卡顿。
/// 优先从缓存读取，如果文件未修改则直接使用缓存。
pub fn get_track_metadata_internal(path: &str) -> Result<TrackMetadata, String> {
    // 首先尝试从缓存获取
    if let Some(cached) = get_metadata_from_cache(path) {
        log::debug!("使用缓存的元数据: {path}");
        return Ok(cached);
    }
    
    // 缓存未命中，提取元数据
    let metadata = get_track_metadata_with_options(path, false)?;
    
    // 保存到内存缓存（不立即写入磁盘，批量保存更高效）
    save_metadata_to_memory_cache(path, &metadata);
    
    Ok(metadata)
}

/// 获取音轨元数据（包含封面路径）
pub fn get_track_metadata_with_cover(path: &str) -> Result<TrackMetadata, String> {
    // 首先尝试从缓存获取
    if let Some(mut cached) = get_metadata_from_cache(path) {
        log::debug!("使用缓存的元数据: {path}");
        
        // 如果缓存中已有封面路径且封面文件存在，直接返回
        if let Some(ref cover_path) = cached.cover_path {
            if Path::new(cover_path).exists() {
                log::debug!("缓存的封面文件存在，直接使用: {cover_path}");
                return Ok(cached);
            }
            log::debug!("缓存的封面文件不存在，需要重新提取: {cover_path}");
            cached.cover_path = None;
        }
        
        // 缓存中没有封面或封面文件不存在，补充提取封面
        let file_path = Path::new(path);
        if let Ok(tagged_file) = Probe::open(file_path).and_then(|f| f.read()) {
            if let Some(tag) = tagged_file.primary_tag() {
                if let Some(picture) = tag.pictures().first() {
                    cached.cover_path = extract_cover_to_cache(file_path, picture).ok();
                }
            }
        }
        
        // 更新缓存（包含封面路径）
        save_metadata_to_cache(path, &cached);
        return Ok(cached);
    }
    
    // 缓存未命中，提取元数据（不包含封面）
    let mut metadata = get_track_metadata_with_options(path, false)?;
    
    // 提取封面路径
    let file_path = Path::new(path);
    if let Ok(tagged_file) = Probe::open(file_path).and_then(|f| f.read()) {
        if let Some(tag) = tagged_file.primary_tag() {
            if let Some(picture) = tag.pictures().first() {
                metadata.cover_path = extract_cover_to_cache(file_path, picture).ok();
            }
        }
    }
    
    // 保存到缓存（包含封面路径）
    save_metadata_to_cache(path, &metadata);
    
    Ok(metadata)
}

/// 获取音轨元数据的统一实现
fn get_track_metadata_with_options(path: &str, include_cover: bool) -> Result<TrackMetadata, String> {
    let file_path = Path::new(path);

    let tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs_f64();

    let format = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_uppercase);

    let mut metadata = TrackMetadata {
        path: path.replace('/', "\\"),
        name: file_path.file_name().unwrap_or_default().to_string_lossy().to_string(),
        duration: if duration > 0.0 { Some(duration) } else { None },
        bitrate: properties.audio_bitrate(),
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        bit_depth: properties.bit_depth(),
        format,
        ..Default::default()
    };

    if let Some(tag) = tagged_file.primary_tag() {
        metadata.title = tag.title().map(|s| s.to_string());
        metadata.artist = tag.artist().map(|s| s.to_string());
        metadata.album = tag.album().map(|s| s.to_string());

        if include_cover {
            if let Some(picture) = tag.pictures().first() {
                metadata.cover_path = extract_cover_to_cache(file_path, picture).ok();
            }
        }
    }

    if metadata.title.is_none() || metadata.title.as_deref() == Some("") {
        metadata.title = Some(metadata.name.clone());
    }

    Ok(metadata)
}

/// 获取音频封面缓存路径（按需提取）
pub fn get_track_cover_path_internal(path: &str) -> Result<Option<String>, String> {
    let file_path = Path::new(path);
    log::debug!("Getting cover for: {path}");
    
    let tagged_file = Probe::open(file_path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .read()
        .map_err(|e| format!("无法读取文件: {e}"))?;

    let primary_tag = tagged_file.primary_tag();
    log::debug!("Primary tag exists: {}", primary_tag.is_some());
    
    let has_picture = primary_tag.map(|tag| !tag.pictures().is_empty()).unwrap_or(false);
    log::debug!("Has picture: {has_picture}");

    let cover = primary_tag
        .and_then(|tag| tag.pictures().first())
        .map(|picture| extract_cover_to_cache(file_path, picture))
        .transpose()?;

    log::debug!("Cover result: {cover:?}");
    Ok(cover)
}

/// 提取音频文件的封面并保存到指定路径
pub fn extract_cover_internal(audio_path: &str, output_path: &str) -> Result<String, String> {
    let file_path = Path::new(audio_path);

    let tagged_file = Probe::open(file_path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .read()
        .map_err(|e| format!("无法读取文件: {e}"))?;

    let tag = tagged_file
        .primary_tag()
        .ok_or_else(|| "文件没有标签信息".to_string())?;

    let picture = tag
        .pictures()
        .first()
        .ok_or_else(|| "文件没有封面图片".to_string())?;

    let extension = cover_extension_from_mime(picture.mime_type().map(lofty::picture::MimeType::as_str));

    let output = Path::new(output_path);
    let final_path = if output.extension().is_none() {
        output.with_extension(extension)
    } else {
        output.to_path_buf()
    };

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目录: {e}"))?;
    }

    fs::write(&final_path, picture.data()).map_err(|e| format!("无法写入文件: {e}"))?;

    Ok(final_path.to_string_lossy().to_string())
}
