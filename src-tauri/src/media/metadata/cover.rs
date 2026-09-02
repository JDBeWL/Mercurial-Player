//! 封面提取与落盘。

use crate::error::AppError;
use crate::security::{has_allowed_extension, is_sensitive_path};
use lofty::picture::Picture;
use lofty::prelude::TaggedFileExt;
use lofty::probe::Probe;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use super::cache::cover_cache_dir;

const COVER_OUTPUT_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

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

fn get_cover_cache_path(_audio_path: &Path, picture: &Picture) -> Result<PathBuf, AppError> {
    // 使用封面数据哈希作为缓存键，相同封面的不同歌曲会共享缓存
    let hash = get_picture_hash(picture);
    let ext = cover_extension_from_mime(picture.mime_type().map(lofty::picture::MimeType::as_str));
    let cache_dir = cover_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| format!("无法创建封面缓存目录: {e}"))?;
    Ok(cache_dir.join(format!("{hash}.{ext}")))
}

pub(super) fn extract_cover_to_cache(
    audio_path: &Path,
    picture: &Picture,
) -> Result<String, AppError> {
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

/// 获取音频封面缓存路径（按需提取）
pub fn get_track_cover_path_internal(path: &str) -> Result<Option<String>, AppError> {
    if is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }

    let file_path = Path::new(path);
    log::debug!("Getting cover for: {path}");

    let tagged_file = Probe::open(file_path)
        .map_err(|e| format!("无法打开文件: {e}"))?
        .read()
        .map_err(|e| format!("无法读取文件: {e}"))?;

    let primary_tag = tagged_file.primary_tag();
    log::debug!("Primary tag exists: {}", primary_tag.is_some());

    let has_picture = primary_tag
        .map(|tag| !tag.pictures().is_empty())
        .unwrap_or(false);
    log::debug!("Has picture: {has_picture}");

    let cover = primary_tag
        .and_then(|tag| tag.pictures().first())
        .map(|picture| extract_cover_to_cache(file_path, picture))
        .transpose()?;

    log::debug!("Cover result: {cover:?}");
    Ok(cover)
}

/// 提取音频文件的封面并保存到指定路径
pub fn extract_cover_internal(audio_path: &str, output_path: &str) -> Result<String, AppError> {
    if is_sensitive_path(audio_path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }
    // 校验输出路径：必须是图片扩展名，且不允许写入敏感目录。
    //
    // 这里不能写成「有扩展名才校验白名单」——那样无扩展名的路径会整体跳过
    // 扩展名检查，只剩 is_sensitive_path 兜底，仍可向用户目录写入任意字节
    // （例如把音频标签里的图片数据写到 AppData 下的无扩展名文件）。
    // has_allowed_extension 对无扩展名返回 false，因此可直接作为闸门。
    if !has_allowed_extension(output_path, &COVER_OUTPUT_EXTENSIONS) {
        return Err(format!(
            "封面输出路径必须是图片文件 ({})",
            COVER_OUTPUT_EXTENSIONS.join("/")
        )
        .into());
    }
    if is_sensitive_path(output_path) {
        return Err("安全限制：不允许写入敏感目录".to_string().into());
    }

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

    let extension =
        cover_extension_from_mime(picture.mime_type().map(lofty::picture::MimeType::as_str));

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
