//! 音轨元数据提取。

use crate::error::AppError;
use crate::security::is_sensitive_path;
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::cache::{
    get_metadata_from_cache, save_metadata_to_cache, save_metadata_to_memory_cache,
};
use super::cover::extract_cover_to_cache;

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
        Self {
            name,
            files: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: TrackMetadata) {
        self.files.push(track);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// 获取音轨的元数据信息（内部函数）
///
/// 默认轻量模式：不提取封面，降低扫描负担，避免前端长时间卡顿。
/// 优先从缓存读取，如果文件未修改则直接使用缓存。
pub fn get_track_metadata_internal(path: &str) -> Result<TrackMetadata, AppError> {
    if is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }

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
pub fn get_track_metadata_with_cover(path: &str) -> Result<TrackMetadata, AppError> {
    if is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }

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
fn get_track_metadata_with_options(
    path: &str,
    include_cover: bool,
) -> Result<TrackMetadata, AppError> {
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
        name: file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
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
