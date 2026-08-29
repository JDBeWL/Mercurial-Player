//! 应用更新模块
//!
//! 检查/安装流程复用 tauri-plugin-updater（配置、签名、安装器行为一致），
//! 下载阶段替换为多线程分片下载（HTTP Range 并发请求），
//! 下载完成后按插件相同逻辑做 minisign 签名校验，再走插件原生安装。
use crate::error::AppError;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures_util::StreamExt;
use minisign_verify::{PublicKey, Signature};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_http::reqwest;
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// 并发分片数量
const PARALLEL_PARTS: usize = 8;
/// 启用并发下载的最小文件体积（过小的文件并发收益低）
const MIN_PARALLEL_SIZE: u64 = 4 * 1024 * 1024;
/// 下载进度事件发送间隔（毫秒）
const PROGRESS_INTERVAL_MS: u64 = 250;
/// 下载进度事件名（前端通过 @tauri-apps/api/event 监听）
const PROGRESS_EVENT: &str = "updater://download-progress";

/// 待处理更新的共享状态
pub struct PendingUpdate {
    /// updater_check 检查到的更新对象
    update: Mutex<Option<Update>>,
    /// updater_download 下载并校验通过的安装包临时文件路径。
    /// 只存路径不存数据：安装包可达上百 MB，全程驻留内存会带来数百 MB 常驻开销
    path: Mutex<Option<PathBuf>>,
}

impl PendingUpdate {
    /// 创建空状态
    pub fn new() -> Self {
        Self {
            update: Mutex::new(None),
            path: Mutex::new(None),
        }
    }
}

impl Default for PendingUpdate {
    fn default() -> Self {
        Self::new()
    }
}

/// 生成更新包临时文件路径（带 pid 与时间戳避免并发冲突）
fn unique_temp_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "mercurial-player-update-{}-{nanos}.part",
        std::process::id()
    ))
}

/// 删除临时文件（不存在则忽略，其余错误仅记录日志）
fn remove_temp_file(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("删除更新临时文件 {} 失败: {e}", path.display());
        }
    }
}

/// 丢弃 pending 中保存的临时文件路径并删除对应文件
fn clear_pending_path(guard: &mut Option<PathBuf>) {
    if let Some(old) = guard.take() {
        remove_temp_file(&old);
    }
}

/// updater_check 返回给前端的更新信息
#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    /// 新版本号
    pub version: String,
    /// 更新说明（Release Notes）
    pub notes: Option<String>,
    /// 发布日期
    pub date: Option<String>,
    /// 当前版本号
    pub current_version: String,
}

/// 下载进度事件负载
#[derive(Debug, Serialize, Clone)]
struct DownloadProgress {
    /// 已下载字节数
    downloaded: u64,
    /// 总字节数（未知时为 0）
    total: u64,
}

/// 检查更新
///
/// 通过 tauri.conf.json 中 plugins.updater 的 endpoints/pubkey 配置检查，
/// 结果保存在 [`PendingUpdate`] 中供后续下载使用
#[tauri::command]
pub async fn updater_check(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<Option<UpdateInfo>, AppError> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("初始化更新器失败: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?;

    let mut update_guard = lock_or_log!(pending.update.lock());
    let mut path_guard = lock_or_log!(pending.path.lock());
    if let Some(update) = update {
        let info = UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
            current_version: update.current_version.clone(),
        };
        *update_guard = Some(update);
        // 版本信息已变化，丢弃旧的下载文件
        clear_pending_path(&mut path_guard);
        Ok(Some(info))
    } else {
        *update_guard = None;
        clear_pending_path(&mut path_guard);
        Ok(None)
    }
}

/// 下载更新
///
/// 多线程分片下载（服务器不支持 Range 时自动回退单线程），流式写入临时文件，
/// 完成后校验 minisign 签名，通过后暂存文件路径供 [`updater_install`] 使用。
/// 进度通过 `updater://download-progress` 事件推送。
#[tauri::command]
pub async fn updater_download(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), AppError> {
    let update = lock_or_log!(pending.update.lock())
        .clone()
        .ok_or("没有可用的更新，请先检查更新")?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(format!("MercurialPlayer/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    // 探测：Range 请求 1 字节，确认服务器支持分片并获取总大小
    let probe = client
        .get(update.download_url.clone())
        .header(reqwest::header::RANGE, "bytes=0-0")
        .send()
        .await
        .map_err(|e| format!("连接更新服务器失败: {e}"))?;

    // Content-Range 形如 "bytes 0-0/12345678"
    let total = probe
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let range_supported = probe.status() == reqwest::StatusCode::PARTIAL_CONTENT && total > 0;
    drop(probe);

    // 下载全程流式写入临时文件，峰值内存只有单个 chunk，
    // 避免安装包（可达上百 MB）在下载期间整体驻留内存两份
    let temp_path = unique_temp_path();
    let download_result = if range_supported && total >= MIN_PARALLEL_SIZE {
        log::info!("更新包支持分片下载，总大小 {total} 字节，{PARALLEL_PARTS} 线程并发");
        download_parallel(&client, &update.download_url, total, &temp_path, &app).await
    } else {
        log::info!("更新包不支持分片下载（大小 {total} 字节），回退单线程");
        download_single(&client, &update.download_url, &temp_path, &app).await
    };

    let written = match download_result {
        Ok(len) => len,
        Err(e) => {
            remove_temp_file(&temp_path);
            return Err(e);
        }
    };

    // 与 tauri-plugin-updater 相同的 minisign 签名校验：
    // 读临时文件到内存校验后立即释放，随后只保留文件路径
    let Some(pubkey) = updater_pubkey(&app) else {
        remove_temp_file(&temp_path);
        return Err(AppError::Network("无法读取更新公钥配置".to_string()));
    };
    let signature = update.signature.clone();
    let verify_path = temp_path.clone();
    let verify_result = tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        let buffer = std::fs::read(&verify_path).map_err(|e| format!("读取下载文件失败: {e}"))?;
        verify_signature(&buffer, &signature, &pubkey)
    })
    .await
    .map_err(|e| AppError::msg(format!("校验任务执行失败: {e}")))
    .and_then(|inner| inner);

    if let Err(e) = verify_result {
        remove_temp_file(&temp_path);
        return Err(e);
    }

    log::info!("更新包下载完成并签名校验通过，共 {written} 字节");
    let mut path_guard = lock_or_log!(pending.path.lock());
    clear_pending_path(&mut path_guard);
    *path_guard = Some(temp_path);
    Ok(())
}

/// 安装已下载的更新
///
/// 委托给 tauri-plugin-updater 原生安装流程
/// （Windows 下拉起安装器并退出进程，由安装器完成替换和重启）
#[tauri::command]
pub async fn updater_install(pending: State<'_, PendingUpdate>) -> Result<(), AppError> {
    let update = lock_or_log!(pending.update.lock())
        .clone()
        .ok_or("没有待安装的更新，请先检查更新")?;
    let temp_path = lock_or_log!(pending.path.lock())
        .take()
        .ok_or("更新尚未下载完成")?;

    // 读文件和安装都涉及大文件 IO 与拉起安装器，放到阻塞线程池执行
    let install_path = temp_path.clone();
    let install_result = tauri::async_runtime::spawn_blocking(move || -> Result<(), AppError> {
        // tauri-plugin-updater 的 install 接口以字节为输入，此处读取是瞬时峰值
        let data = std::fs::read(&install_path).map_err(|e| format!("读取更新包文件失败: {e}"))?;
        update
            .install(&data)
            .map_err(|e| AppError::msg(format!("安装更新失败: {e}")))
    })
    .await
    .map_err(|e| AppError::msg(format!("安装任务执行失败: {e}")))
    .and_then(|inner| inner);

    match install_result {
        Ok(()) => {
            remove_temp_file(&temp_path);
            Ok(())
        }
        // 安装失败时保留临时文件以便重试，避免重新下载
        Err(message) => {
            *lock_or_log!(pending.path.lock()) = Some(temp_path);
            Err(message)
        }
    }
}

/// 从 tauri.conf.json 的 plugins.updater 配置中读取公钥
fn updater_pubkey(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()
        .map(String::from)
}

/// 启动进度上报任务：每 250ms 向前端推送一次已下载字节数，下载结束后自行退出
fn spawn_progress_reporter(
    app: AppHandle,
    fetched: Arc<AtomicU64>,
    done: Arc<AtomicBool>,
    total: u64,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(PROGRESS_INTERVAL_MS)).await;
            let downloaded = fetched.load(Ordering::Relaxed);
            if let Err(e) = app.emit(PROGRESS_EVENT, DownloadProgress { downloaded, total }) {
                log::warn!("发送下载进度事件失败: {e}");
            }
            if done.load(Ordering::Relaxed) {
                break;
            }
        }
    })
}

/// 多线程分片下载：将文件按 [`PARALLEL_PARTS`] 分片并发请求，
/// 各分片流式写入同一临时文件的对应偏移（峰值内存仅为单个 chunk），
/// 完成后校验落盘大小
async fn download_parallel(
    client: &reqwest::Client,
    url: &reqwest::Url,
    total: u64,
    path: &Path,
    app: &AppHandle,
) -> Result<u64, AppError> {
    // 先创建并截断目标文件；各分片任务各自持有写句柄，seek 到自己的偏移写入
    std::fs::File::create(path).map_err(|e| format!("创建临时文件失败: {e}"))?;

    let fetched = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let reporter =
        spawn_progress_reporter(app.clone(), Arc::clone(&fetched), Arc::clone(&done), total);

    let part_size = total.div_ceil(PARALLEL_PARTS as u64);
    let path_buf = path.to_path_buf();

    let mut handles = Vec::new();
    for i in 0..PARALLEL_PARTS {
        let start = part_size.saturating_mul(i as u64);
        if start >= total {
            break;
        }
        let end = part_size
            .saturating_mul((i as u64).saturating_add(1))
            .saturating_sub(1)
            .min(total.saturating_sub(1));

        let client = client.clone();
        let url = url.clone();
        let fetched = Arc::clone(&fetched);
        let path_buf = path_buf.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            download_range(&client, &url, start, end, path_buf, &fetched)
                .await
                .map(|written| (i, written))
        }));
    }

    let download_result: Result<u64, AppError> = async {
        let mut written = 0u64;
        for handle in handles {
            let (_, part_written) = handle
                .await
                .map_err(|e| AppError::msg(format!("下载任务执行失败: {e}")))??;
            written += part_written;
        }

        // 校验落盘大小（分片写入各自 seek 到不同偏移，总大小即完整性依据）
        let actual = std::fs::metadata(path)
            .map_err(|e| format!("校验下载文件失败: {e}"))?
            .len();
        if actual != total {
            return Err(AppError::Network(format!(
                "下载数据不完整: {actual}/{total} 字节"
            )));
        }
        Ok(written)
    }
    .await;

    // 无论成败都结束进度上报任务，避免泄漏
    done.store(true, Ordering::Relaxed);
    let _ = reporter.await;
    download_result
}

/// 下载单个分片（含 Range 请求头），流式写入临时文件的对应偏移，并累加全局进度计数
async fn download_range(
    client: &reqwest::Client,
    url: &reqwest::Url,
    start: u64,
    end: u64,
    path: PathBuf,
    fetched: &AtomicU64,
) -> Result<u64, AppError> {
    let expected = end.saturating_sub(start).saturating_add(1) as usize;
    let response = client
        .get(url.clone())
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|e| format!("分片请求失败: {e}"))?;

    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "分片下载失败，状态码: {}",
            response.status()
        )));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .await
        .map_err(|e| format!("打开分片写入文件失败: {e}"))?;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| format!("分片定位失败: {e}"))?;

    let mut written = 0usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("分片下载中断: {e}"))?;
        fetched.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入分片数据失败: {e}"))?;
        written += chunk.len();
    }
    file.flush()
        .await
        .map_err(|e| format!("刷新分片数据失败: {e}"))?;

    if written != expected {
        return Err(AppError::Network(format!(
            "分片数据不完整: 期望 {expected} 字节，实际 {written} 字节"
        )));
    }
    Ok(written as u64)
}

/// 单线程流式下载（服务器不支持 Range 或文件过小时的回退路径），写入临时文件
async fn download_single(
    client: &reqwest::Client,
    url: &reqwest::Url,
    path: &Path,
    app: &AppHandle,
) -> Result<u64, AppError> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;

    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "下载失败，状态码: {}",
            response.status()
        )));
    }

    let total = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let fetched = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let reporter =
        spawn_progress_reporter(app.clone(), Arc::clone(&fetched), Arc::clone(&done), total);

    let download_result: Result<u64, AppError> = async {
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|e| format!("创建临时文件失败: {e}"))?;

        let mut written = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
            fetched.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("写入下载数据失败: {e}"))?;
            written += chunk.len() as u64;
        }
        file.flush()
            .await
            .map_err(|e| format!("刷新下载数据失败: {e}"))?;

        if total > 0 && written != total {
            return Err(AppError::Network(format!(
                "下载数据不完整: {written}/{total} 字节"
            )));
        }
        Ok(written)
    }
    .await;

    done.store(true, Ordering::Relaxed);
    let _ = reporter.await;
    download_result
}

/// minisign 签名校验（与 tauri-plugin-updater 内部实现一致：
/// 公钥与签名均为 base64 编码的 minisign 格式）
fn verify_signature(data: &[u8], release_signature: &str, pub_key: &str) -> Result<(), AppError> {
    let pub_key_decoded =
        base64_decode_to_string(pub_key).map_err(|e| format!("更新公钥解码失败: {e}"))?;
    let public_key =
        PublicKey::decode(&pub_key_decoded).map_err(|e| format!("更新公钥解析失败: {e}"))?;

    let signature_decoded =
        base64_decode_to_string(release_signature).map_err(|e| format!("更新签名解码失败: {e}"))?;
    let signature =
        Signature::decode(&signature_decoded).map_err(|e| format!("更新签名解析失败: {e}"))?;

    public_key
        .verify(data, &signature, true)
        .map_err(|e| format!("更新签名校验失败: {e}"))?;
    Ok(())
}

/// base64 解码为 UTF-8 字符串
fn base64_decode_to_string(input: &str) -> Result<String, AppError> {
    let decoded = BASE64_STANDARD.decode(input).map_err(|e| e.to_string())?;
    String::from_utf8(decoded).map_err(|e| AppError::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode_to_string() {
        assert_eq!(base64_decode_to_string("aGVsbG8=").unwrap(), "hello");
        assert_eq!(base64_decode_to_string("").unwrap(), "");

        // 非 base64 字符
        assert!(base64_decode_to_string("!!!").is_err());
        // 合法 base64 但不是 UTF-8 (0xFF 单字节)
        assert!(base64_decode_to_string("/w==").is_err());
    }

    #[test]
    fn test_unique_temp_path_is_unique_and_nonexistent() {
        let a = unique_temp_path();
        let b = unique_temp_path();
        assert!(a != b, "两次调用应产生不同路径");
        assert!(!a.exists(), "路径不应已存在: {}", a.display());
        assert!(a.to_string_lossy().contains("mercurial-player-update-"));
    }

    #[test]
    fn test_remove_temp_file_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "mercurial-player-update-test-{}.tmp",
            std::process::id()
        ));
        std::fs::write(&path, b"data").unwrap();
        remove_temp_file(&path);
        assert!(!path.exists());
        // 不存在的路径: 不应 panic
        remove_temp_file(&path);
    }

    /// 从 tauri.conf.json 读取真实的 updater 公钥 (发布配置),
    /// 保证测试与生产使用同一密钥材料
    fn production_pubkey() -> String {
        let conf = include_str!("../tauri.conf.json");
        let value: serde_json::Value = serde_json::from_str(conf).unwrap();
        value["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("tauri.conf.json 应包含 updater pubkey")
            .to_string()
    }

    #[test]
    fn test_verify_signature_rejects_invalid_pubkey_base64() {
        let err = verify_signature(b"data", "c2ln", "not-base64!!").unwrap_err();
        assert!(err.to_string().contains("更新公钥解码失败"));
    }

    #[test]
    fn test_verify_signature_rejects_malformed_pubkey() {
        // 合法 base64 但不是 minisign 公钥二进制格式
        let garbage = BASE64_STANDARD.encode("garbage");
        let err = verify_signature(b"data", "c2ln", &garbage).unwrap_err();
        assert!(err.to_string().contains("更新公钥解析失败"));
    }

    #[test]
    fn test_verify_signature_rejects_invalid_signature_base64() {
        let err = verify_signature(b"data", "!!!not-base64!!!", &production_pubkey()).unwrap_err();
        assert!(err.to_string().contains("更新签名解码失败"));
    }

    #[test]
    fn test_verify_signature_rejects_malformed_signature() {
        // 真实公钥 + 合法 base64 但非 minisign 签名结构
        let garbage = BASE64_STANDARD.encode("garbage");
        let err = verify_signature(b"data", &garbage, &production_pubkey()).unwrap_err();
        assert!(err.to_string().contains("更新签名解析失败"));
    }
}
