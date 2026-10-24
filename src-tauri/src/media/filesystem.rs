//! 文件系统操作：目录读取、媒体/歌词文件读写。
//!
//! 桌面端是绝对路径，Android 是 SAF `content://` URI；打开文件经 `crate::android::saf` 的 fd 桥。

use super::metadata::{Playlist, TrackMetadata, flush_metadata_cache, get_track_metadata_internal};
use crate::android::saf;
use crate::config::AppConfig;
use crate::error::AppError;
use crate::security::{
    has_allowed_extension, is_sensitive_path, is_simple_filename, is_within_music_dirs,
};
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// 支持的音频扩展名：只收 symphonia 能解开的容器（没有 opus/ape/wma）。
/// 同一白名单在 Kotlin `SafBridge` 与前端 `FileUtils` 各有一份，由镜像测试守住一致。
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "m4a", "aac", "aiff", "aif", "caf",
];

/// 允许读写的歌词文件扩展名
const LYRICS_EXTENSIONS: [&str; 3] = ["lrc", "ass", "srt"];

/// 歌词单文件大小上限：歌词文本通常远小于此，上限挡住误指向的大文件被整读/整写进内存
const MAX_LYRICS_BYTES: u64 = 5 * 1024 * 1024;

/// 校验歌词文件路径：扩展名白名单 -> 敏感目录黑名单 -> 已登记音乐目录
/// （`is_within_music_dirs` 内部 canonicalize 复查，挡住 symlink/junction 逃逸与未授权目录）。
/// SAF URI 代表用户授权，只校验扩展名。
///
/// `file_must_exist=false`（写入新建歌词）时文件可能尚不存在，改为 canonicalize 其父目录；
/// 文件名本身要求是单一组件（`is_simple_filename`），防 `..`、分隔符、盘符混入最后一段。
fn validate_lyrics_path(
    path: &str,
    music_dirs: &[String],
    file_must_exist: bool,
) -> Result<(), AppError> {
    if !has_allowed_extension(path, &LYRICS_EXTENSIONS) {
        return Err("仅允许读写歌词文件（.lrc/.ass/.srt）".to_string().into());
    }
    if saf::is_content_uri(path) {
        return Ok(());
    }
    if is_sensitive_path(path) {
        return Err("安全限制：不允许访问敏感目录".to_string().into());
    }

    let target = Path::new(path);
    let _file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| is_simple_filename(n))
        .ok_or_else(|| AppError::Path("非法的歌词文件名".to_string()))?;

    if file_must_exist {
        // 文件本体 canonicalize：文件级 symlink 指向音乐目录外也会被解析出来
        if !is_within_music_dirs(path, music_dirs) {
            return Err("安全限制：歌词文件必须位于已添加的音乐目录内"
                .to_string()
                .into());
        }
    } else {
        let Some(parent) = target.parent() else {
            return Err(AppError::Path("歌词路径缺少父目录".to_string()));
        };
        if !is_within_music_dirs(&parent.to_string_lossy(), music_dirs) {
            return Err("安全限制：歌词文件必须位于已添加的音乐目录内"
                .to_string()
                .into());
        }
    }
    Ok(())
}

/// 单次目录扫描的最大递归深度（防止对深层目录树的 DoS 式扫描）
const MAX_SCAN_DEPTH: usize = 10;

/// 校验媒体路径：先词法查敏感目录，canonicalize 后复查；接受前端 path 的命令入口都过这里，SAF URI 直接放行
///
/// 复查是因为 canonicalize 会解析出输入里看不出的敏感位置（junction / symlink 逃逸）；
/// Windows 的 canonicalize 还会加 `\\?\` 前缀，须剥掉才能和黑名单前缀对上。解析失败（路径不存在）视为不安全。
pub fn validate_media_path(path: &str) -> Result<(), AppError> {
    if saf::is_content_uri(path) {
        return Ok(());
    }

    if is_sensitive_path(path) {
        return Err(AppError::Path("安全限制：不允许访问敏感目录".to_string()));
    }

    let canonical = fs::canonicalize(path)
        .map_err(|_| AppError::Path("无法解析路径，请确保路径存在".to_string()))?;

    let mut canonical_str = canonical.to_string_lossy().to_string();
    if let Some(stripped) = canonical_str.strip_prefix(r"\\?\") {
        canonical_str = stripped.to_string();
    }
    if is_sensitive_path(&canonical_str) {
        return Err(AppError::Path("安全限制：不允许访问敏感目录".to_string()));
    }

    Ok(())
}

/// 读取指定目录中的子目录列表；Android SAF content URI 下不枚举（前端改用系统目录选择器逐层授权）
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

/// 获取指定目录中的所有音频文件，并创建播放列表；SAF content URI 走 `scan_content_tree`
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
            match scan_content_tree(path, config.playlist.folder_based_playlists) {
                Ok(playlists) => all_playlists.extend(playlists),
                Err(e) => log::warn!("Skipping content tree '{path}': {e}"),
            }
            continue;
        }

        // 单个目录校验失败（敏感目录/不存在）只跳过，不中断整批扫描
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

    if let Err(e) = flush_metadata_cache() {
        log::warn!("批量保存元数据缓存失败: {e}");
    } else {
        log::info!("元数据缓存已批量保存到磁盘");
    }

    Ok(all_playlists)
}

/// 扫描单个 SAF content 树，行为与桌面端对齐：按相对树根的目录路径分组生成多个播放列表
///
/// 元数据提取走 rayon 并行（SAF fd 桥要经 JNI，逐首串行很慢）；列表名取可读名（`primary%3AMusic` -> `Music`）。
fn scan_content_tree(tree_uri: &str, folder_based: bool) -> Result<Vec<Playlist>, AppError> {
    let entries = saf::list_audio_files(tree_uri)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let root_name = saf::display_name_from_tree_uri(tree_uri);

    // 每个线程各自 attach JVM 走 fd 桥，互不干扰
    let tracks: Vec<(String, TrackMetadata)> = entries
        .par_iter()
        .filter_map(|entry| {
            match get_track_metadata_internal(&entry.uri) {
                Ok(mut track) => {
                    // URI 最后一段是 document id，读不出文件名，只能用 DocumentFile 给的显示名兜底
                    if track.name.is_empty() || track.name.contains(':') {
                        track.name.clone_from(&entry.name);
                    }
                    // 播放/封面的入参统一以 URI 为准
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
            let display = key.rsplit('/').next().unwrap_or(&key).to_string();
            Some(Playlist {
                name: display,
                files,
            })
        })
        .collect())
}

/// 扫描目录，按每个文件的父目录分组成多个播放列表
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

/// 扫描目录，所有曲目汇总成一个以目录名命名的播放列表
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

/// 检查文件是否存在（敏感路径一律 false）
#[must_use]
pub fn check_file_exists_internal(path: &str) -> bool {
    if is_sensitive_path(path) && !saf::is_content_uri(path) {
        return false;
    }
    if saf::is_content_uri(path) {
        return true; // ContentResolver 授权后即可读；存在性在打开时校验
    }

    if Path::new(path).exists() {
        return true;
    }

    let alt_path = if path.contains('/') {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    };

    alt_path != path && Path::new(&alt_path).exists()
}

/// 读取歌词文件内容（content URI 走 fd 桥）；单文件大小受 [`MAX_LYRICS_BYTES`] 限制
pub fn read_lyrics_file_internal(path: &str, music_dirs: &[String]) -> Result<String, AppError> {
    use std::io::Read;

    validate_lyrics_path(path, music_dirs, true)?;
    let mut file = saf::open_media_file(path)?;
    // take(上限+1)：超限文件读出上限+1 字节即可判定，不把整个文件读进内存
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_LYRICS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取歌词文件失败: {e}"))?;
    if bytes.len() as u64 > MAX_LYRICS_BYTES {
        return Err("歌词文件过大，拒绝读取".to_string().into());
    }
    String::from_utf8(bytes).map_err(|e| format!("歌词文件不是有效的 UTF-8 文本: {e}").into())
}

/// 写入歌词文件内容（Android 上落到应用沙箱）：先写同目录临时文件再 rename 原子替换，
/// 半写入的内容不会覆盖原文件；rename 替换的是目录项本身，也不会跟随目标位置的 symlink
pub fn write_lyrics_file_internal(
    path: &str,
    content: &str,
    music_dirs: &[String],
) -> Result<(), AppError> {
    validate_lyrics_path(path, music_dirs, false)?;
    if saf::is_content_uri(path) {
        // 刻意没接 SAF 的写 fd（saf::open_write_file），content URI 一律拒绝
        return Err("无法写入 SAF 管理的歌词文件".to_string().into());
    }
    if content.len() as u64 > MAX_LYRICS_BYTES {
        return Err("歌词内容过大，拒绝写入".to_string().into());
    }

    // validate_lyrics_path 已 canonicalize 父目录：父目录必须已存在，因此无需也不应
    // 在这里 create_dir_all（新建目录本身就是一次未受控写）
    let target = Path::new(path);

    // 临时文件必须与目标同目录，rename 才落在同一文件系统上；命名带 pid+纳秒避免并发冲突
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp_path = PathBuf::from(format!("{path}.{}.{}.tmp", std::process::id(), nanos));

    if let Err(e) = fs::write(&tmp_path, content) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write file: {e}").into());
    }
    fs::rename(&tmp_path, target).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to replace lyrics file: {e}")
    })?;
    Ok(())
}

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

    /// 建一个带纳秒标签的独立临时目录，避免并行测试互相踩。
    /// 放 target/ 而非系统临时目录：macOS 的 $TMPDIR 在 /var 下，会被
    /// is_sensitive_path 的 /var 前缀判为敏感目录，歌词读写校验直接拒绝
    fn make_lyrics_test_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("mp_lyrics_{tag}_{nanos}"));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn validate_lyrics_path_requires_authorized_dir() {
        let base = make_lyrics_test_dir("auth");
        let dirs = vec![base.to_string_lossy().to_string()];
        let inside = base.join("song.lrc");
        fs::write(&inside, "[00:00.00] test").expect("write inside");
        // 未登记但非敏感的兄弟路径：确保被拒是因为目录未授权，而不是命中敏感黑名单
        let outside = base.parent().unwrap().join(format!(
            "mp_lyrics_outside_{}.lrc",
            base.file_name().unwrap().to_string_lossy()
        ));
        fs::write(&outside, "x").expect("write outside");

        // 已登记目录内的既有文件可读
        assert!(validate_lyrics_path(&inside.to_string_lossy(), &dirs, true).is_ok());
        // 未登记目录的文件：读和写都拒绝
        assert!(validate_lyrics_path(&outside.to_string_lossy(), &dirs, true).is_err());
        assert!(validate_lyrics_path(&outside.to_string_lossy(), &dirs, false).is_err());
        // 音乐目录内的新文件（尚不存在）：按父目录判定，允许写
        let new_file = base.join("new-song.lrc");
        assert!(validate_lyrics_path(&new_file.to_string_lossy(), &dirs, false).is_ok());

        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_file(&outside);
    }

    #[test]
    fn validate_lyrics_path_rejects_bad_extension_and_sensitive_dir() {
        let base = make_lyrics_test_dir("ext");
        let dirs = vec![base.to_string_lossy().to_string()];
        let txt = base.join("song.txt");
        fs::write(&txt, "x").expect("write");

        assert!(validate_lyrics_path(&txt.to_string_lossy(), &dirs, true).is_err());
        // 敏感目录即使在授权列表里也拒绝（黑名单优先）
        let sensitive = vec!["C:\\Windows\\System32".to_string()];
        let win_file = Path::new("C:\\Windows\\System32\\song.lrc");
        assert!(validate_lyrics_path(&win_file.to_string_lossy(), &sensitive, true).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn validate_lyrics_path_allows_content_uri_with_valid_extension() {
        // SAF URI 代表用户授权，只查扩展名；is_content_uri 仅 Android 返回 true
        #[cfg(target_os = "android")]
        {
            assert!(validate_lyrics_path("content://tree/x/song.lrc", &[], true).is_ok());
            assert!(validate_lyrics_path("content://tree/x/song.txt", &[], true).is_err());
        }
        // 非 Android 上 content URI 不作特判，按普通路径走授权目录校验
        #[cfg(not(target_os = "android"))]
        {
            assert!(validate_lyrics_path("content://tree/x/song.txt", &[], true).is_err());
        }
    }

    #[test]
    fn write_lyrics_replaces_atomically_and_leaves_no_tmp() {
        let base = make_lyrics_test_dir("write");
        let dirs = vec![base.to_string_lossy().to_string()];
        let target = base.join("song.lrc");
        fs::write(&target, "old").expect("seed");

        write_lyrics_file_internal(&target.to_string_lossy(), "new content", &dirs)
            .expect("write lyrics");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");

        // 无残留临时文件
        let leftovers = fs::read_dir(&base)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
            })
            .count();
        assert_eq!(leftovers, 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn write_lyrics_rejects_oversized_content() {
        let base = make_lyrics_test_dir("big_write");
        let dirs = vec![base.to_string_lossy().to_string()];
        let target = base.join("song.lrc");
        let big = "x".repeat((MAX_LYRICS_BYTES + 1) as usize);

        assert!(write_lyrics_file_internal(&target.to_string_lossy(), &big, &dirs).is_err());
        // 被拒后不应产生任何文件
        assert!(!target.exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn read_lyrics_rejects_oversized_file() {
        let base = make_lyrics_test_dir("big_read");
        let dirs = vec![base.to_string_lossy().to_string()];
        let target = base.join("song.lrc");
        fs::write(&target, vec![b'a'; (MAX_LYRICS_BYTES + 1) as usize]).expect("seed big file");

        assert!(read_lyrics_file_internal(&target.to_string_lossy(), &dirs).is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
