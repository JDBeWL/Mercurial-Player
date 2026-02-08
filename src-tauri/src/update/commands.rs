//! 自动更新命令模块

use tauri::Emitter;

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
) -> Result<(), String> {
    tokio::spawn(async move {
        if let Err(e) = download_update(&app, &download_url).await {
            let _ = app.emit("update-error", &e);
            // 也打印和发出日志，方便排查
            println!("Update download failed: {e}");
            let _ = app.emit("update-log", format!("Download failed: {e}"));
        }
    });

    Ok(())
}

/// 下载更新文件的实际实现
async fn download_update(app: &tauri::AppHandle, download_url: &str) -> Result<(), String> {
    use reqwest::Client;
    use std::fs;
    use std::io::Write;

    // 首先尝试使用可执行文件同级的 updates 目录（便于程序唤起安装）；若不可写则回退到系统临时目录
    let cache_dir = (|| -> Result<std::path::PathBuf, String> {
        // 获取 exe 所在目录
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let candidate = exe_dir.join("updates");
                if fs::create_dir_all(&candidate).is_ok() {
                    // 检测写入权限
                    let test_file = candidate.join(".write_test");
                    if fs::File::create(&test_file).is_ok() {
                        let _ = fs::remove_file(&test_file);
                        return Ok(candidate);
                    }
                    // 无写权限，回退到 temp
                }
            }
        }
        // 回退到 Temp 目录
        let tmp = std::env::temp_dir().join("Mercurial Player").join("updates");
        fs::create_dir_all(&tmp).map_err(|e| format!("Failed to create updates dir: {e}"))?;
        Ok(tmp)
    })()?;

    // 通知最终使用的目录
    let dir_msg = format!("Using updates directory: {}", cache_dir.to_string_lossy());
    println!("{dir_msg}");
    let _ = app.emit("update-log", &dir_msg);

    // 下载文件
    let client = Client::new();
    let response = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    let total_size = response
        .content_length()
        .ok_or("Failed to get content length")?;

    // 记录并发出开始下载的日志
    let start_msg = format!("Starting download from: {download_url}");
    println!("{start_msg}");
    let _ = app.emit("update-log", &start_msg);

    // 发送 'started' 事件，方便前端开启进度条
    let _ = app.emit("update-started", total_size);

    // 获取文件名
    let filename = download_url
        .split('/')
        .next_back()
        .unwrap_or("installer.exe");

    let installer_path = cache_dir.join(filename);

    // 告知保存路径
    let save_msg = format!("Saving installer to: {}", installer_path.to_string_lossy());
    println!("{save_msg}");
    let _ = app.emit("update-log", &save_msg);

    // 下载并保存文件
    let mut file = fs::File::create(&installer_path)
        .map_err(|e| {
            let msg = format!("Failed to create file: {e}");
            let _ = app.emit("update-log", &msg);
            msg
        })?;

    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();
    use futures::stream::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            let msg = format!("Download error: {e}");
            let _ = app.emit("update-log", &msg);
            msg
        })?;
        file.write_all(&chunk)
            .map_err(|e| {
                let msg = format!("Write error: {e}");
                let _ = app.emit("update-log", &msg);
                msg
            })?;

        downloaded += chunk.len() as u64;
        let progress = ((downloaded as f64 / total_size as f64) * 100.0) as u32;

        // 发送进度事件
        let _ = app.emit("update-progress", progress);
    }

    // 完成下载后，返回安装程序路径给前端；实际执行安装由前端在确认后调用单独命令执行
    #[cfg(windows)]
    {
        // 给文件系统刷新的时间
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // 通知前端下载完成，并返回安装程序路径
        let _ = app.emit("update-finished", installer_path.to_string_lossy().to_string());
        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err("Auto-update is Windows-only".to_string())
    }
}

/// 执行安装程序（由前端在用户确认后调用）
#[tauri::command]
pub fn run_installer(app: tauri::AppHandle, installer_path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        // 在开发模式下禁止直接执行安装，避免误操作
        if cfg!(debug_assertions) {
            return Err("Installer execution is disabled in dev builds. Build a release to run installer.".to_string());
        }

        Command::new(&installer_path)
            .spawn()
            .map_err(|e| format!("Failed to execute installer: {e}"))?;

        // 通知前端安装已启动
        let _ = app.emit("installer-started", &installer_path);
        let _ = app.emit("update-log", format!("Installer started: {installer_path}"));
        println!("Installer started: {installer_path}");

        // 异步退出应用（给安装程序启动时间）
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::process::exit(0);
        });

        Ok(())
    }

    #[cfg(not(windows))]
    {
        Err("Installer execution is Windows-only".to_string())
    }
}
