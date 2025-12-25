//! 音频元数据模块
//!
//! 提供音轨元数据结构和处理函数。

use base64::{engine::general_purpose, Engine as _};
use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use serde::Serialize;
use std::path::Path;

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
    pub cover: Option<String>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    pub format: Option<String>,
}

impl TrackMetadata {
    #[must_use]
    #[allow(dead_code)]
    pub fn new(path: String, name: String) -> Self {
        Self { path, name, ..Default::default() }
    }

    #[allow(dead_code)]
    pub fn with_title(mut self, title: Option<String>) -> Self { self.title = title; self }
    #[allow(dead_code)]
    pub fn with_artist(mut self, artist: Option<String>) -> Self { self.artist = artist; self }
    #[allow(dead_code)]
    pub fn with_album(mut self, album: Option<String>) -> Self { self.album = album; self }
    #[allow(dead_code)]
    pub fn with_duration(mut self, duration: Option<f64>) -> Self { self.duration = duration; self }
    #[allow(dead_code)]
    pub fn with_cover(mut self, cover: Option<String>) -> Self { self.cover = cover; self }
    #[allow(dead_code)]
    pub fn with_bitrate(mut self, bitrate: Option<u32>) -> Self { self.bitrate = bitrate; self }
    #[allow(dead_code)]
    pub fn with_sample_rate(mut self, sample_rate: Option<u32>) -> Self { self.sample_rate = sample_rate; self }
    #[allow(dead_code)]
    pub fn with_channels(mut self, channels: Option<u8>) -> Self { self.channels = channels; self }
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
    pub fn new(name: String) -> Self {
        Self { name, files: Vec::new() }
    }

    pub fn add_track(&mut self, track: TrackMetadata) {
        self.files.push(track);
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn track_count(&self) -> usize {
        self.files.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[allow(dead_code)]
    pub fn get_track(&self, index: usize) -> Option<&TrackMetadata> {
        self.files.get(index)
    }

    #[allow(dead_code)]
    pub fn get_track_mut(&mut self, index: usize) -> Option<&mut TrackMetadata> {
        self.files.get_mut(index)
    }

    #[allow(dead_code)]
    pub fn remove_track(&mut self, index: usize) -> Option<TrackMetadata> {
        if index < self.files.len() { Some(self.files.remove(index)) } else { None }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.files.clear();
    }
}

/// 获取音轨的元数据信息（内部函数）
pub fn get_track_metadata_internal(path: &str) -> Result<TrackMetadata, String> {
    let file_path = Path::new(path);

    let tagged_file = Probe::open(file_path)
        .map_err(|e| e.to_string())?
        .read()
        .map_err(|e| e.to_string())?;

    let properties = tagged_file.properties();
    let duration = properties.duration().as_secs_f64();
    
    // 获取文件格式
    let format = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_uppercase());

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

        if let Some(picture) = tag.pictures().first() {
            let mime_type = picture.mime_type().map_or("image/jpeg", |m| m.as_str());
            let data = picture.data();
            metadata.cover = Some(format!("data:{mime_type};base64,{}", general_purpose::STANDARD.encode(data)));
        }
    }

    if metadata.title.is_none() || metadata.title.as_deref() == Some("") {
        metadata.title = Some(metadata.name.clone());
    }

    Ok(metadata)
}
