//! 文件系统操作模块：目录读取、文件检查等。
//! 路径双形态：桌面端为本地绝对路径；Android 为 SAF `content://` URI（分区存储下绝对路径不可用），
//! 打开文件经 [`crate::android::saf`] 的 fd 桥完成。

use super::metadata::{Playlist, TrackMetadata, flush_metadata_cache, get_track_metadata_internal};
use crate::android::saf;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::security::{has_allowed_extension, is_sensitive_path};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use walkdir::{DirEntry, WalkDir};

/// 支持的音频文件扩展名
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "aac"];

/// 允许读写的歌词文件扩展名
const LYRICS_EXTENSIONS: [&str; 3] = ["lrc", "ass", "srt"];

/// 校验歌词文件路径：仅允许歌词扩展名，且不允许访问敏感目录
/// （Android 上 SAF content URI 放行词法检查，仅校验扩展名）
fn validate_lyrics_path(path: &str) -> Result<(), AppError> {
    if !has_allowed_extension(path, &LYRICS_EXTENSIONS) {
        return Err("仅允许读写歌词文件（.lrc/.ass/.srt）".to_string().into());
    }
    if !saf::is_content_uri(path) && is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }
    Ok(())
}

/// 单次目录扫描的最大递归深度（防止对深层目录树的 DoS 式扫描）
const MAX_SCAN_DEPTH: usize = 10;

/// 校验媒体路径：词法敏感目录检查 + canonicalize 复查（防 symlink/junction 逃逸），用于所有
/// 接受前端 path 的媒体命令入口；canonicalize 失败（路径不存在）视为不安全。
/// Android SAF 的 `content://` URI 不做本地文件系统校验，直接放行。
pub fn validate_media_path(path: &str) -> Result<(), AppError> {
    if saf::is_content_uri(path) {
        return Ok(());
    }

    // 先对原始输入做词法检查
    if is_sensitive_path(path) {
        return Err(AppError::Path("安全限制：不允许访问敏感目录".to_string()));
    }

    // 规范化路径
    let canonical = fs::canonicalize(path)
        .map_err(|_| AppError::Path("无法解析路径，请确保路径存在".to_string()))?;

    // canonicalize 可能解析出输入中未显现的敏感位置（如 junction/symlink），需复查
    let mut canonical_str = canonical.to_string_lossy().to_string();
    if let Some(stripped) = canonical_str.strip_prefix(r"\\?\") {
        canonical_str = stripped.to_string();
    }
    if is_sensitive_path(&canonical_str) {
        return Err(AppError::Path("安全限制：不允许访问敏感目录".to_string()));
    }

    Ok(())
}

/// 读取指定目录中的子目录列表
///
/// Android SAF content URI 下不枚举子目录（前端改用系统目录选择器逐层授权），返回空列表。
pub fn read_dir(path: &str) -> Result<Vec<String>, AppError> {
    if saf::is_content_uri(path) {
        return Ok(Vec::new());
    }
    validate_media_path(path)?;
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string().into());
    }

    fs::read_dir(dir)
        .map_err(|e| AppError::msg(e.to_string()))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path().to_str().map(String::from))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Failed to convert paths".to_string().into())
}

/// 获取指定目录中的所有音频文件，并创建播放列表
///
/// Android SAF content URI 由 [`scan_content_tree`] 单独处理（`get_all_audio_files_from_dirs` 入口）。
pub fn get_audio_files_from_dir(path: &str) -> Result<Playlist, AppError> {
    if saf::is_content_uri(path) {
        let playlists = scan_content_tree(path, true)?;
        return playlists
            .into_iter()
            .next()
            .ok_or_else(|| "目录中没有音频文件".to_string().into());
    }
    validate_media_path(path)?;
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err("Provided path is not a directory".to_string().into());
    }

    let audio_files: Vec<_> = WalkDir::new(dir)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    let playlist_name = dir.file_name().map_or_else(
        || "Unknown".to_string(),
        |s| s.to_string_lossy().to_string(),
    );

    Ok(Playlist {
        name: playlist_name,
        files: tracks,
    })
}

/// 获取多个目录中的所有音频文件，并创建播放列表
pub fn get_all_audio_files_from_dirs(
    paths: &[String],
    config: &AppConfig,
) -> Result<Vec<Playlist>, AppError> {
    let mut all_playlists: Vec<Playlist> = Vec::new();

    for path in paths {
        if saf::is_content_uri(path) {
            // Android SAF 树：ContentResolver 查询式枚举 + 并行提元数据
            match scan_content_tree(path, config.playlist.folder_based_playlists) {
                Ok(playlists) => all_playlists.extend(playlists),
                Err(e) => log::warn!("Skipping content tree '{path}': {e}"),
            }
            continue;
        }

        // 校验每个目录（敏感目录/不存在则跳过，不中断批量扫描）
        if let Err(e) = validate_media_path(path) {
            log::warn!("Skipping path '{path}': {e}");
            continue;
        }
        let dir = Path::new(path);
        if !dir.is_dir() {
            log::warn!("Provided path is not a directory: {path}");
            continue;
        }

        if config.directory_scan.enable_subdirectory_scan && config.playlist.folder_based_playlists
        {
            let playlists =
                scan_with_folder_playlists(dir, config.directory_scan.max_depth as usize);
            all_playlists.extend(playlists);
        } else if let Some(playlist) =
            scan_single_playlist(dir, config.directory_scan.max_depth as usize)
        {
            all_playlists.push(playlist);
        }
    }

    // 扫描完成后，批量保存内存缓存到磁盘
    if let Err(e) = flush_metadata_cache() {
        log::warn!("批量保存元数据缓存失败: {e}");
    } else {
        log::info!("元数据缓存已批量保存到磁盘");
    }

    Ok(all_playlists)
}

/// 扫描单个 SAF content 树，与桌面端行为对齐：元数据用 rayon 并行提取（fd 桥要经 JNI，串行很慢），
/// 按相对树根的目录路径分组生成多个播放列表，列表名用可读名（`primary%3AMusic` → `Music`）。
fn scan_content_tree(tree_uri: &str, folder_based: bool) -> Result<Vec<Playlist>, AppError> {
    let entries = saf::list_audio_files(tree_uri)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let root_name = saf::display_name_from_tree_uri(tree_uri);

    // 并行提取元数据：SAF 打开走 fd 桥（每个线程各自 attach JVM，互不干扰）
    let tracks: Vec<(String, TrackMetadata)> = entries
        .par_iter()
        .filter_map(|entry| {
            match get_track_metadata_internal(&entry.uri) {
                Ok(mut track) => {
                    // 用 DocumentFile 给出的显示名兜底（URI 最后一段是 document id，不可读作文件名）
                    if track.name.is_empty() || track.name.contains(':') {
                        track.name.clone_from(&entry.name);
                    }
                    // 播放/封面临入统一以 URI 为准
                    track.path.clone_from(&entry.uri);
                    Some((entry.folder_path.clone(), track))
                }
                Err(e) => {
                    log::warn!("SAF 元数据提取失败 {}: {e}", entry.name);
                    None
                }
            }
        })
        .collect();

    if !folder_based {
        let mut playlist = Playlist::new(root_name);
        for (_, track) in tracks {
            playlist.add_track(track);
        }
        return Ok(if playlist.is_empty() {
            Vec::new()
        } else {
            vec![playlist]
        });
    }

    // 按文件夹分组：保持首次出现顺序（BTreeMap 会按字典序打乱用户的目录顺序）
    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<TrackMetadata>> = HashMap::new();
    for (folder_path, track) in tracks {
        let key = if folder_path.is_empty() {
            root_name.clone()
        } else {
            folder_path.clone()
        };
        if !grouped.contains_key(&key) {
            order.push(key.clone());
        }
        grouped.entry(key).or_default().push(track);
    }

    Ok(order
        .into_iter()
        .filter_map(|key| {
            let files = grouped.remove(&key)?;
            // 展示名只取最后一段，多级目录保持 "父/子" 的可读形式
            let display = key.rsplit('/').next().unwrap_or(&key).to_string();
            Some(Playlist {
                name: display,
                files,
            })
        })
        .collect())
}

/// 扫描目录并按文件夹创建播放列表（轻量模式，不读取封面）
fn scan_with_folder_playlists(dir: &Path, max_depth: usize) -> Vec<Playlist> {
    let audio_files: Vec<_> = WalkDir::new(dir)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks_with_folders: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let parent_dir = entry.path().parent().unwrap_or(dir);
            let folder_name = parent_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Unknown")
                .to_string();

            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map(|metadata| (folder_name, metadata))
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    let mut folder_playlists: HashMap<String, Playlist> = HashMap::new();
    for (folder_name, metadata) in tracks_with_folders {
        folder_playlists
            .entry(folder_name.clone())
            .or_insert_with(|| Playlist::new(folder_name))
            .add_track(metadata);
    }

    folder_playlists
        .into_values()
        .filter(|p| !p.is_empty())
        .collect()
}

/// 扫描目录创建单个播放列表（完整模式，包含封面）
fn scan_single_playlist(dir: &Path, max_depth: usize) -> Option<Playlist> {
    let playlist_name = dir.file_name().map_or_else(
        || "Unknown".to_string(),
        |s| s.to_string_lossy().to_string(),
    );

    let audio_files: Vec<_> = WalkDir::new(dir)
        .max_depth(max_depth)
        .into_iter()
        .filter_map(Result::ok)
        .filter(is_audio_file)
        .collect();

    let tracks: Vec<_> = audio_files
        .par_iter()
        .filter_map(|entry| {
            let file_path = entry.path().to_string_lossy().to_string();
            get_track_metadata_internal(&file_path)
                .map_err(|e| log::warn!("Failed to get metadata for file '{file_path}': {e}"))
                .ok()
        })
        .collect();

    if tracks.is_empty() {
        None
    } else {
        Some(Playlist {
            name: playlist_name,
            files: tracks,
        })
    }
}

/// 检查文件是否存在（敏感路径一律返回 false；content URI 由 SAF 侧判断，视为存在）
#[must_use]
pub fn check_file_exists_internal(path: &str) -> bool {
    if is_sensitive_path(path) && !saf::is_content_uri(path) {
        return false;
    }
    if saf::is_content_uri(path) {
        return true; // SAF 文件由 ContentResolver 授权后即可读；具体存在性在打开时校验
    }

    if Path::new(path).exists() {
        return true;
    }

    // 尝试另一种路径分隔符格式
    let alt_path = if path.contains('/') {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    };

    alt_path != path && Path::new(&alt_path).exists()
}

/// 读取歌词文件内容（content URI 走 fd 桥）
pub fn read_lyrics_file_internal(path: &str) -> Result<String, AppError> {
    validate_lyrics_path(path)?;
    let mut file = saf::open_media_file(path)?;
    let mut content = String::new();
    use std::io::Read;
    file.read_to_string(&mut content)
        .map_err(|e| format!("读取歌词文件失败: {e}"))?;
    Ok(content)
}

/// 写入歌词文件内容（Android 上通常写入应用沙箱，content URI 只读）
pub fn write_lyrics_file_internal(path: &str, content: &str) -> Result<(), AppError> {
    validate_lyrics_path(path)?;
    if saf::is_content_uri(path) {
        // 歌词写入没有接 SAF 的写 fd（saf::open_write_file），content URI 一律拒绝
        return Err("无法写入 SAF 管理的歌词文件".to_string().into());
    }

    // 确保父目录存在
    if let Some(parent) = Path::new(path).parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    fs::write(path, content).map_err(|e| format!("Failed to write file: {e}").into())
}

/// 检查是否为音频文件
fn is_audio_file(entry: &DirEntry) -> bool {
    entry
        .path()
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_media_path_rejects_sensitive_paths() {
        assert!(validate_media_path("C:\\Windows\\System32").is_err());
        assert!(validate_media_path("/etc").is_err());
        assert!(validate_media_path("C:\\Users\\a\\.ssh\\id_rsa").is_err());
        assert!(validate_media_path("D:\\Music\\.gnupg\\x").is_err());
    }

    #[test]
    fn validate_media_path_rejects_nonexistent() {
        assert!(validate_media_path("Z:\\definitely\\not\\exist").is_err());
    }

    #[test]
    fn validate_media_path_allows_normal_dir() {
        // 测试运行于 crate 根目录，属于普通工作目录；
        // 极端环境下（cwd 位于敏感目录）跳过断言
        let cwd = std::env::current_dir().expect("current dir");
        if !is_sensitive_path(&cwd.to_string_lossy()) {
            assert!(validate_media_path(&cwd.to_string_lossy()).is_ok());
        }
    }

    #[test]
    fn check_file_exists_blocks_sensitive_paths() {
        // 即使文件确实存在，敏感路径也一律返回 false
        if cfg!(windows) {
            assert!(!check_file_exists_internal(
                "C:\\Windows\\System32\\cmd.exe"
            ));
        }
        assert!(!check_file_exists_internal("/etc/passwd"));
        assert!(!check_file_exists_internal("C:\\Users\\a\\.ssh\\id_rsa"));
    }
}
