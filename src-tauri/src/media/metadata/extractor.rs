//! 音轨元数据提取。

use crate::android_saf;
use crate::error::AppError;
use crate::security::is_sensitive_path;
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use serde::{Deserialize, Serialize};
use std::io::BufReader;
use std::path::Path;

use super::cache::{
    get_metadata_from_cache, save_metadata_to_cache, save_metadata_to_memory_cache,
};
use super::cover::extract_cover_to_cache;

/// 打开媒体文件并读取标签（本地路径 / Android SAF content URI）
pub(super) fn open_tagged_file(path: &str) -> Result<lofty::file::TaggedFile, AppError> {
    if !android_saf::is_content_uri(path) {
        // 本地路径按扩展名探测，不让内容嗅探改变桌面既有的格式判定
        return Probe::open(Path::new(path))
            .map_err(|e| e.to_string())
            .map_err(AppError::from)?
            .read()
            .map_err(|e| e.to_string())
            .map_err(AppError::from);
    }
    // content URI 没有扩展名可用；Probe 0.24+ 需显式 guess_file_type
    let file = android_saf::open_media_file(path)?;
    Probe::new(BufReader::new(file))
        .guess_file_type()
        .map_err(|e| e.to_string())
        .map_err(AppError::from)?
        .read()
        .map_err(|e| e.to_string())
        .map_err(AppError::from)
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
        if let Ok(tagged_file) = open_tagged_file(path) {
            if let Some(tag) = tagged_file.primary_tag() {
                if let Some(picture) = tag.pictures().first() {
                    cached.cover_path = extract_cover_to_cache(Path::new(path), picture).ok();
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
    if let Ok(tagged_file) = open_tagged_file(path) {
        if let Some(tag) = tagged_file.primary_tag() {
            if let Some(picture) = tag.pictures().first() {
                metadata.cover_path = extract_cover_to_cache(Path::new(path), picture).ok();
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
    let is_uri = android_saf::is_content_uri(path);
    let file_path = Path::new(path);

    let tagged_file = open_tagged_file(path)?;
    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs_f64();

    // 本地路径从扩展名推断格式；content URI 由 SAF 侧传入的显示名兜底（上层填写）
    let format = if is_uri {
        None
    } else {
        file_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_uppercase)
    };
    // name 兜底：本地路径取文件名；content URI 解码 document id 取文件名
    // （URI 末段是 URL 编码的 document id，直接展示会变成 primary%3AMusic%2F…）
    let name = if is_uri {
        android_saf::display_name_from_document_uri(path)
    } else {
        file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    let mut metadata = TrackMetadata {
        // 桌面端保持原有 `\` 风格路径（兼容旧缓存）；URI 原样保留
        path: if is_uri {
            path.to_string()
        } else {
            path.replace('/', "\\")
        },
        name,
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
                metadata.cover_path = extract_cover_to_cache(Path::new(path), picture).ok();
            }
        }
    }

    if metadata.title.is_none() || metadata.title.as_deref() == Some("") {
        metadata.title = Some(metadata.name.clone());
    }

    Ok(metadata)
}
