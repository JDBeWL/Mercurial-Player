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

/// 默认缓存根 `$TEMP/mercurial-player`：未设置自定义路径时的缓存根，
/// 权限判定（is_within_default_cache_root）与设置页展示也取它
pub(crate) fn default_cache_root() -> PathBuf {
    std::env::temp_dir().join("mercurial-player")
}

/// 缓存根目录（用户自定义优先，否则系统临时目录）：元数据、封面、BPM 共用，改一处管全部。
pub(crate) fn cache_dir() -> PathBuf {
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path);
    }
    default_cache_root()
}

/// 元数据缓存文件路径（缓存根目录下的 metadata-cache.json）
fn metadata_cache_path() -> PathBuf {
    cache_dir().join(METADATA_CACHE_FILENAME)
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
pub(crate) fn get_file_modified_time(path: &str) -> Option<u64> {
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

/// 把 BPM 分析结论回填进已有的元数据缓存条目。
///
/// 只改已有条目、不新建：分析任务手上只有路径与 BPM；未扫描过的文件等下次扫描
/// 由 `bpm::cached_value` 带出来。
pub fn patch_cached_bpm(path: &str, bpm: Option<f64>) {
    let mut guard = lock_or_log!(MEMORY_CACHE.write());
    if guard.is_none() {
        *guard = Some(load_metadata_cache());
    }
    if let Some(entry) = guard.as_mut().and_then(|cache| cache.entries.get_mut(path)) {
        entry.metadata.bpm = bpm;
    }
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
    // BPM 缓存的语义属于元数据缓存：留着会与已清空的扫描结果矛盾
    if let Err(e) = crate::media::bpm::clear_bpm_cache() {
        log::warn!("清除 BPM 分析缓存失败: {e}");
    }
    log::info!("元数据缓存已清除");
    Ok(())
}

/// 缓存统计 (条目数, 磁盘占用字节数)：占用取缓存文件大小，清理针对的就是这个文件
///
/// BPM 分析缓存（`bpm-cache.json`）同样计入：它由元数据缓存派生，分开统计只会让用户困惑。
pub fn get_metadata_cache_stats() -> (usize, u64) {
    let cache_path = metadata_cache_path();
    let bpm_path = cache_dir().join("bpm-cache.json");
    let total_size = fs::metadata(&cache_path).map(|m| m.len()).unwrap_or(0)
        + fs::metadata(&bpm_path).map(|m| m.len()).unwrap_or(0);
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

/// 封面缓存子目录名（自定义根目录下同样存在）
const COVER_CACHE_DIR_NAME: &str = "cover-cache";

/// 封面缓存目录（缓存根目录下的 cover-cache 子目录）
pub(super) fn cover_cache_dir() -> PathBuf {
    if let Some(custom_path) = get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join(COVER_CACHE_DIR_NAME);
    }
    default_cache_root().join(COVER_CACHE_DIR_NAME)
}

/// 缓存根内由应用管理的顶层文件清单，切换缓存路径时逐项搬到新根。
/// 新增 `resolve_cache_file` 调用方时必须把文件名补进来，否则换目录时它会被留在原地；
/// 封面缓存是目录，由 [`migrate_cache_root`] 单独递归搬运。
const CACHE_ARTIFACT_FILES: &[&str] = &[
    "metadata-cache.json", // METADATA_CACHE_FILENAME
    "bpm-cache.json",      // get_metadata_cache_stats 计入的 BPM 缓存
    "beat-cache.json",     // beat_cache::BEAT_CACHE_FILENAME
    "library-cache.json",  // 前端曲库扫描缓存（resolve_cache_file 的唯一调用方）
];

/// 单文件搬运。目标已存在视为重复内容（封面按内容哈希命名必然同名同内容）：
/// 删源保留目标——来回切换目录时 JSON 可能因此留下较旧的一份，重新扫描即可补全。
fn move_one_file(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        fs::remove_file(src).map_err(|e| format!("删除重复源文件失败: {e}"))?;
        return Ok(());
    }
    // 跨盘 rename 会失败，回退为复制+删除
    if fs::rename(src, dst).is_err() {
        fs::copy(src, dst).map_err(|e| format!("复制失败: {e}"))?;
        fs::remove_file(src).map_err(|e| format!("删除源文件失败: {e}"))?;
    }
    Ok(())
}

/// 目录递归搬运（封面缓存目前是平铺的，仍按递归实现以兼容未来子目录）
fn move_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {e}"))?;
    let entries = fs::read_dir(src).map_err(|e| format!("读取目录失败: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {e}"))?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            move_dir_contents(&path, &target)?;
            // 搬空才删得掉；非空说明个别文件失败，留待下次切换重试
            let _ = fs::remove_dir(&path);
        } else {
            move_one_file(&path, &target)?;
        }
    }
    Ok(())
}

/// 切换缓存根后，把旧根内的缓存工件搬到新根。
///
/// 单个工件失败只记日志跳过、不整体回滚：搬不过去的条目等价于缓存丢失，
/// 扫描后自动重建，不应阻断用户切换路径。调用方负责先落盘内存元数据缓存。
pub fn migrate_cache_root(old_root: &Path, new_root: &Path) {
    if !old_root.exists() {
        return;
    }
    if let Err(e) = fs::create_dir_all(new_root) {
        log::error!("创建新缓存目录失败 {}: {e}", new_root.display());
        return;
    }
    for name in CACHE_ARTIFACT_FILES {
        let src = old_root.join(name);
        if src.is_file() {
            if let Err(e) = move_one_file(&src, &new_root.join(name)) {
                log::warn!("迁移缓存文件失败 {}: {e}", src.display());
            }
        }
    }
    let old_covers = old_root.join(COVER_CACHE_DIR_NAME);
    if old_covers.is_dir() {
        if let Err(e) = move_dir_contents(&old_covers, &new_root.join(COVER_CACHE_DIR_NAME)) {
            log::warn!("迁移封面缓存目录失败: {e}");
        }
        // 搬空才删得掉；删不掉说明仍有残留文件，留待下次切换重试
        let _ = fs::remove_dir(&old_covers);
    }
    log::info!(
        "缓存根迁移完成: {} -> {}",
        old_root.display(),
        new_root.display()
    );
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
/// `max_cache_size_mb` 单位 MB，为 None 时取 `DEFAULT_MAX_CACHE_SIZE_MB`。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例独立的临时目录，结束后 best-effort 清理
    fn scratch_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mp-cache-test-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn migrate_moves_artifacts_and_cover_dir() {
        let old = scratch_root("old");
        let new = scratch_root("new");

        for name in CACHE_ARTIFACT_FILES {
            fs::write(old.join(name), name).unwrap();
        }
        fs::create_dir_all(old.join(COVER_CACHE_DIR_NAME)).unwrap();
        fs::write(old.join(COVER_CACHE_DIR_NAME).join("123.png"), "png").unwrap();
        // 不在清单内的文件不属于应用缓存，必须留在原地
        fs::write(old.join("user-file.txt"), "keep").unwrap();

        migrate_cache_root(&old, &new);

        for name in CACHE_ARTIFACT_FILES {
            assert!(new.join(name).is_file(), "{name} 应迁移到新根");
            assert!(!old.join(name).exists(), "{name} 应从旧根消失");
        }
        assert_eq!(
            fs::read_to_string(new.join(COVER_CACHE_DIR_NAME).join("123.png")).unwrap(),
            "png"
        );
        assert!(
            !old.join(COVER_CACHE_DIR_NAME).exists(),
            "搬空后的旧封面目录应删除"
        );
        assert!(old.join("user-file.txt").exists(), "未知文件必须留在原地");

        let _ = fs::remove_dir_all(&old);
        let _ = fs::remove_dir_all(&new);
    }

    #[test]
    fn migrate_without_old_root_is_noop() {
        let old = std::env::temp_dir().join("mp-cache-test-never-created");
        let new = scratch_root("new");

        migrate_cache_root(&old, &new);

        // 旧根不存在时不创建任何工件（新根目录本身允许被 create_dir_all 建出）
        for name in CACHE_ARTIFACT_FILES {
            assert!(!new.join(name).exists());
        }
        assert!(!new.join(COVER_CACHE_DIR_NAME).exists());

        let _ = fs::remove_dir_all(&new);
    }

    #[test]
    fn migrate_dedupes_when_destination_already_has_the_file() {
        let old = scratch_root("old");
        let new = scratch_root("new");

        // 目标已有同名文件（如上次使用该目录留下的），源文件按重复内容处理掉
        fs::write(old.join("metadata-cache.json"), "src").unwrap();
        fs::write(new.join("metadata-cache.json"), "dst").unwrap();

        migrate_cache_root(&old, &new);

        assert!(new.join("metadata-cache.json").is_file());
        assert!(!old.join("metadata-cache.json").exists());

        let _ = fs::remove_dir_all(&old);
        let _ = fs::remove_dir_all(&new);
    }
}
