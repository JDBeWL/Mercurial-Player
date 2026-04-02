//! 音频元数据模块
//!
//! 提供音轨元数据结构和处理函数。

use lofty::picture::Picture;
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// 单个音轨的元数据
#[derive(Debug, Serialize, Default, Clone)]
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
    std::env::temp_dir().join("mercurial-player").join("cover-cache")
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

fn get_cover_cache_path(audio_path: &Path, picture: &Picture) -> Result<PathBuf, String> {
    let mut hasher = DefaultHasher::new();
    audio_path.to_string_lossy().hash(&mut hasher);

    let modified_secs = fs::metadata(audio_path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0_u64, |d| d.as_secs());
    modified_secs.hash(&mut hasher);

    let hash = hasher.finish();
    let ext = cover_extension_from_mime(picture.mime_type().map(lofty::picture::MimeType::as_str));
    let cache_dir = cover_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| format!("无法创建封面缓存目录: {e}"))?;
    Ok(cache_dir.join(format!("{hash}.{ext}")))
}

fn extract_cover_to_cache(audio_path: &Path, picture: &Picture) -> Result<String, String> {
    let cache_file = get_cover_cache_path(audio_path, picture)?;

    if !cache_file.exists() {
        fs::write(&cache_file, picture.data()).map_err(|e| format!("写入封面缓存失败: {e}"))?;
    }

    Ok(cache_file.to_string_lossy().to_string())
}

/// 获取音轨的元数据信息（内部函数）
///
/// 默认轻量模式：不提取封面，降低扫描负担，避免前端长时间卡顿。
pub fn get_track_metadata_internal(path: &str) -> Result<TrackMetadata, String> {
    get_track_metadata_with_options(path, false)
}

/// 获取音轨元数据（包含封面路径）
pub fn get_track_metadata_with_cover(path: &str) -> Result<TrackMetadata, String> {
    get_track_metadata_with_options(path, true)
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
    let tagged_file = Probe::open(file_path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .read()
        .map_err(|e| format!("无法读取文件: {e}"))?;

    let cover = tagged_file
        .primary_tag()
        .and_then(|tag| tag.pictures().first())
        .map(|picture| extract_cover_to_cache(file_path, picture))
        .transpose()?;

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
