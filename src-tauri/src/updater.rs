//! 应用更新模块
//!
//! 检查/安装流程复用 tauri-plugin-updater（配置、签名、安装器行为一致），
//! 下载阶段替换为多线程分片下载（HTTP Range 并发请求），
//! 下载完成后按插件相同逻辑做 minisign 签名校验，再走插件原生安装。

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
    /// updater_download 下载并校验通过的安装包数据
    data: Mutex<Option<Vec<u8>>>,
}

impl PendingUpdate {
    /// 创建空状态
    pub fn new() -> Self {
        Self {
            update: Mutex::new(None),
            data: Mutex::new(None),
        }
    }
}

impl Default for PendingUpdate {
    fn default() -> Self {
        Self::new()
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
) -> Result<Option<UpdateInfo>, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("初始化更新器失败: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?;

    let mut update_guard = lock_or_log!(pending.update.lock());
    let mut data_guard = lock_or_log!(pending.data.lock());
    if let Some(update) = update {
        let info = UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
            current_version: update.current_version.clone(),
        };
        *update_guard = Some(update);
        // 版本信息已变化，丢弃旧的下载数据
        *data_guard = None;
        Ok(Some(info))
    } else {
        *update_guard = None;
        *data_guard = None;
        Ok(None)
    }
}

/// 下载更新
///
/// 多线程分片下载（服务器不支持 Range 时自动回退单线程），
/// 完成后校验 minisign 签名，通过后暂存数据供 [`updater_install`] 使用。
/// 进度通过 `updater://download-progress` 事件推送。
#[tauri::command]
pub async fn updater_download(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
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

    let buffer = if range_supported && total >= MIN_PARALLEL_SIZE {
        log::info!("更新包支持分片下载，总大小 {total} 字节，{PARALLEL_PARTS} 线程并发");
        download_parallel(&client, &update.download_url, total, &app).await?
    } else {
        log::info!("更新包不支持分片下载（大小 {total} 字节），回退单线程");
        download_single(&client, &update.download_url, &app).await?
    };

    // 与 tauri-plugin-updater 相同的 minisign 签名校验
    let pubkey = updater_pubkey(&app).ok_or("无法读取更新公钥配置")?;
    verify_signature(&buffer, &update.signature, &pubkey)?;

    log::info!("更新包下载完成并签名校验通过，共 {} 字节", buffer.len());
    *lock_or_log!(pending.data.lock()) = Some(buffer);
    Ok(())
}

/// 安装已下载的更新
///
/// 委托给 tauri-plugin-updater 原生安装流程
/// （Windows 下拉起安装器并退出进程，由安装器完成替换和重启）
#[tauri::command]
pub async fn updater_install(pending: State<'_, PendingUpdate>) -> Result<(), String> {
    let update = lock_or_log!(pending.update.lock())
        .clone()
        .ok_or("没有待安装的更新，请先检查更新")?;
    let data = lock_or_log!(pending.data.lock())
        .take()
        .ok_or("更新尚未下载完成")?;

    // 安装涉及写临时文件和拉起安装器，放到阻塞线程池执行，避免阻塞异步运行时
    let result = tauri::async_runtime::spawn_blocking(move || {
        match update.install(&data) {
            Ok(()) => Ok(()),
            // 安装失败时带回下载数据以便重试，避免重新下载
            Err(e) => Err((format!("安装更新失败: {e}"), data)),
        }
    })
    .await
    .map_err(|e| format!("安装任务执行失败: {e}"))?;

    match result {
        Ok(()) => Ok(()),
        Err((message, data)) => {
            *lock_or_log!(pending.data.lock()) = Some(data);
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

/// 多线程分片下载：将文件按 [`PARALLEL_PARTS`] 分片并发请求，完成后按序拼接
async fn download_parallel(
    client: &reqwest::Client,
    url: &reqwest::Url,
    total: u64,
    app: &AppHandle,
) -> Result<Vec<u8>, String> {
    let fetched = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let reporter =
        spawn_progress_reporter(app.clone(), Arc::clone(&fetched), Arc::clone(&done), total);

    let part_size = total.div_ceil(PARALLEL_PARTS as u64);

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
        handles.push(tauri::async_runtime::spawn(async move {
            download_range(&client, &url, start, end, &fetched)
                .await
                .map(|buf| (i, buf))
        }));
    }

    let download_result: Result<Vec<u8>, String> = async {
        let mut parts = Vec::with_capacity(handles.len());
        for handle in handles {
            let (index, buf) = handle
                .await
                .map_err(|e| format!("下载任务执行失败: {e}"))??;
            parts.push((index, buf));
        }

        parts.sort_by_key(|(index, _)| *index);
        let mut buffer = Vec::with_capacity(total as usize);
        for (_, buf) in parts {
            buffer.extend_from_slice(&buf);
        }
        if buffer.len() as u64 != total {
            return Err(format!("下载数据不完整: {}/{} 字节", buffer.len(), total));
        }
        Ok(buffer)
    }
    .await;

    // 无论成败都结束进度上报任务，避免泄漏
    done.store(true, Ordering::Relaxed);
    let _ = reporter.await;
    download_result
}

/// 下载单个分片（含 Range 请求头），并累加全局进度计数
async fn download_range(
    client: &reqwest::Client,
    url: &reqwest::Url,
    start: u64,
    end: u64,
    fetched: &AtomicU64,
) -> Result<Vec<u8>, String> {
    let expected = end.saturating_sub(start).saturating_add(1) as usize;
    let response = client
        .get(url.clone())
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|e| format!("分片请求失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("分片下载失败，状态码: {}", response.status()));
    }

    let mut buffer = Vec::with_capacity(expected);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("分片下载中断: {e}"))?;
        fetched.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        buffer.extend_from_slice(&chunk);
    }

    if buffer.len() != expected {
        return Err(format!(
            "分片数据不完整: 期望 {expected} 字节，实际 {} 字节",
            buffer.len()
        ));
    }
    Ok(buffer)
}

/// 单线程流式下载（服务器不支持 Range 或文件过小时的回退路径）
async fn download_single(
    client: &reqwest::Client,
    url: &reqwest::Url,
    app: &AppHandle,
) -> Result<Vec<u8>, String> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|e| format!("下载请求失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("下载失败，状态码: {}", response.status()));
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

    let download_result: Result<Vec<u8>, String> = async {
        let mut buffer = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
            fetched.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            buffer.extend_from_slice(&chunk);
        }
        if total > 0 && buffer.len() as u64 != total {
            return Err(format!("下载数据不完整: {}/{} 字节", buffer.len(), total));
        }
        Ok(buffer)
    }
    .await;

    done.store(true, Ordering::Relaxed);
    let _ = reporter.await;
    download_result
}

/// minisign 签名校验（与 tauri-plugin-updater 内部实现一致：
/// 公钥与签名均为 base64 编码的 minisign 格式）
fn verify_signature(data: &[u8], release_signature: &str, pub_key: &str) -> Result<(), String> {
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
fn base64_decode_to_string(input: &str) -> Result<String, String> {
    let decoded = BASE64_STANDARD.decode(input).map_err(|e| e.to_string())?;
    String::from_utf8(decoded).map_err(|e| e.to_string())
}
