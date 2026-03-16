//! 自动更新命令模块

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use futures::stream::StreamExt;
use sha2::{Digest, Sha256};
use tauri::Emitter;

const UPDATER_OWNER: &str = "JDBeWL";
const UPDATER_REPO: &str = "Mercurial-Player";
const UPDATER_ALLOWED_HOST: &str = "github.com";
const UPDATER_ALLOWED_ASSET_HOST: &str = "objects.githubusercontent.com";
const UPDATER_ALLOWED_ASSET_HOST_2: &str = "github-releases.githubusercontent.com";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFinishedPayload {
    pub installer_path: String,
    pub sha256: String,
}


/// 获取应用版本信息
#[tauri::command]
pub fn get_app_version(app: tauri::AppHandle) -> String {
    app.package_info().version.to_string()
}

/// 下载并安装更新
#[tauri::command]
pub async fn download_and_install_update(
    app: tauri::AppHandle,
    download_url: String,
    expected_sha256: String,
) -> Result<(), String> {
    if expected_sha256.trim().is_empty() {
        return Err("Expected SHA-256 is required".to_string());
    }

    tokio::spawn(async move {
        if let Err(e) = download_update(&app, &download_url, &expected_sha256).await {
            let _ = app.emit("update-error", &e);
            println!("Update download failed: {e}");
            let _ = app.emit("update-log", format!("Download failed: {e}"));
        }
    });

    Ok(())
}

/// 下载更新文件的实际实现
async fn download_update(
    app: &tauri::AppHandle,
    download_url: &str,
    expected_sha256: &str,
) -> Result<(), String> {
    use reqwest::Client;

    validate_download_url(download_url)?;

    // 首先尝试使用可执行文件同级的 updates 目录（便于程序唤起安装）；若不可写则回退到系统临时目录
    let cache_dir = (|| -> Result<PathBuf, String> {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let candidate = exe_dir.join("updates");
                if fs::create_dir_all(&candidate).is_ok() {
                    let test_file = candidate.join(".write_test");
                    if fs::File::create(&test_file).is_ok() {
                        let _ = fs::remove_file(&test_file);
                        return Ok(candidate);
                    }
                }
            }
        }

        let tmp = std::env::temp_dir().join("Mercurial Player").join("updates");
        fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create updates dir: {e}"))?;
        Ok(tmp)
    })()?;

    let dir_msg = format!("Using updates directory: {}", cache_dir.to_string_lossy());
    println!("{dir_msg}");
    let _ = app.emit("update-log", &dir_msg);

    let client = Client::new();
    let response = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Download request failed: HTTP {}", response.status()));
    }

    let total_size = response
        .content_length()
        .ok_or("Failed to get content length")?;

    let start_msg = format!("Starting download from: {download_url}");
    println!("{start_msg}");
    let _ = app.emit("update-log", &start_msg);
    let _ = app.emit("update-started", total_size);

    let filename = download_url
        .split('/')
        .next_back()
        .unwrap_or("installer.exe");
    validate_installer_filename(filename)?;

    let installer_path = cache_dir.join(filename);
    let save_msg = format!("Saving installer to: {}", installer_path.to_string_lossy());
    println!("{save_msg}");
    let _ = app.emit("update-log", &save_msg);

    let mut file = fs::File::create(&installer_path).map_err(|e| {
        let msg = format!("Failed to create file: {e}");
        let _ = app.emit("update-log", &msg);
        msg
    })?;

    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            let msg = format!("Download error: {e}");
            let _ = app.emit("update-log", &msg);
            msg
        })?;

        file.write_all(&chunk).map_err(|e| {
            let msg = format!("Write error: {e}");
            let _ = app.emit("update-log", &msg);
            msg
        })?;

        downloaded += chunk.len() as u64;
        let progress = ((downloaded as f64 / total_size as f64) * 100.0) as u32;
        let _ = app.emit("update-progress", progress);
    }

    #[cfg(windows)]
    {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let installer_sha256 = compute_file_sha256(&installer_path)?;
        if !installer_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err("Downloaded installer hash does not match expected checksum".to_string());
        }

        let payload = UpdateFinishedPayload {
            installer_path: installer_path.to_string_lossy().to_string(),
            sha256: installer_sha256,
        };

        let _ = app.emit("update-finished", payload);
        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err("Auto-update is Windows-only".to_string())
    }
}

fn validate_download_url(download_url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(download_url).map_err(|e| format!("Invalid download URL: {e}"))?;

    if parsed.scheme() != "https" {
        return Err("Only HTTPS download URL is allowed".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or("Download URL host is missing")?
        .to_ascii_lowercase();

    let path = parsed.path();
    let from_release_asset = path.starts_with(&format!("/{UPDATER_OWNER}/{UPDATER_REPO}/releases/download/"));

    let is_allowed_host = if host == UPDATER_ALLOWED_HOST {
        from_release_asset
    } else {
        host == UPDATER_ALLOWED_ASSET_HOST || host == UPDATER_ALLOWED_ASSET_HOST_2
    };

    if !is_allowed_host {
        return Err(format!("Download host/path not allowed: {host}{path}"));
    }

    Ok(())
}

fn validate_installer_filename(filename: &str) -> Result<(), String> {
    if filename.is_empty() {
        return Err("Installer filename is empty".to_string());
    }

    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return Err("Invalid installer filename".to_string());
    }

    let lower = filename.to_ascii_lowercase();
    if !(lower.ends_with(".exe") || lower.ends_with(".msi")) {
        return Err("Installer must be .exe or .msi".to_string());
    }

    Ok(())
}

fn ensure_installer_path_allowed(installer_path: &str) -> Result<PathBuf, String> {
    let input = PathBuf::from(installer_path);
    let canonical = fs::canonicalize(&input).map_err(|e| format!("Invalid installer path: {e}"))?;

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let mut allowed_dirs: Vec<PathBuf> = vec![std::env::temp_dir().join("Mercurial Player").join("updates")];
    if let Some(dir) = exe_dir {
        allowed_dirs.push(dir.join("updates"));
    }

    let mut allowed = false;
    for dir in allowed_dirs {
        if let Ok(canonical_dir) = fs::canonicalize(&dir)
            && canonical.starts_with(&canonical_dir)
        {
            allowed = true;
            break;
        }
    }

    if !allowed {
        return Err("Installer path is outside allowed updates directories".to_string());
    }

    if !canonical.is_file() {
        return Err("Installer path is not a file".to_string());
    }

    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or("Installer extension missing")?;

    if ext != "exe" && ext != "msi" {
        return Err("Installer extension must be .exe or .msi".to_string());
    }

    Ok(canonical)
}

fn compute_file_sha256(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;

    let mut file = fs::File::open(path).map_err(|e| format!("Failed to open file for hashing: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read file for hashing: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// 执行安装程序（由前端在用户确认后调用）
#[tauri::command]
pub fn run_installer(
    app: tauri::AppHandle,
    installer_path: String,
    expected_sha256: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        // 在开发模式下禁止直接执行安装，避免误操作
        if cfg!(debug_assertions) {
            return Err("Installer execution is disabled in dev builds. Build a release to run installer.".to_string());
        }

        let canonical_installer = ensure_installer_path_allowed(&installer_path)?;
        let actual_sha256 = compute_file_sha256(&canonical_installer)?;

        if !actual_sha256.eq_ignore_ascii_case(&expected_sha256) {
            return Err("Installer hash verification failed".to_string());
        }

        Command::new(&canonical_installer)
            .spawn()
            .map_err(|e| format!("Failed to execute installer: {e}"))?;

        let installer_display = canonical_installer.to_string_lossy().to_string();

        // 通知前端安装已启动
        let _ = app.emit("installer-started", &installer_display);
        let _ = app.emit("update-log", format!("Installer started: {installer_display}"));
        println!("Installer started: {installer_display}");

        // 异步退出应用（给安装程序启动时间）
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::process::exit(0);
        });

        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = expected_sha256;
        Err("Installer execution is Windows-only".to_string())
    }
}
