//! 应用更新管理模块
//! 
//! 处理自动更新检查、下载和安装

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use reqwest::Client;

/// 更新管理器
pub struct UpdateManager {
    cache_dir: PathBuf,
}

impl UpdateManager {
    /// 创建新的更新管理器
    pub fn new(app: &AppHandle) -> tauri::Result<Self> {
        let cache_dir = app
            .path()
            .cache_dir()
            .map_err(|e| tauri::Error::Io(e))?
            .join("updates");

        fs::create_dir_all(&cache_dir).ok();

        Ok(Self { cache_dir })
    }

    /// 获取应用版本
    pub async fn get_app_version(app: &AppHandle) -> String {
        app.package_info().version.to_string()
    }

    /// 下载和安装更新
    pub async fn download_and_install(
        app: &AppHandle,
        download_url: &str,
    ) -> Result<(), String> {
        let client = Client::new();
        let response = client
            .get(download_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download: {}", e))?;

        let total_size = response
            .content_length()
            .ok_or("Failed to get content length")?;

        // 获取文件名
        let filename = download_url
            .split('/')
            .last()
            .unwrap_or("installer.exe");

        let installer_path = self.cache_dir.join(filename);

        // 下载文件
        let mut file = fs::File::create(&installer_path)
            .map_err(|e| format!("Failed to create file: {}", e))?;

        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();

        use futures::stream::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;
            file.write_all(&chunk)
                .map_err(|e| format!("Write error: {}", e))?;

            downloaded += chunk.len() as u64;
            let progress = ((downloaded as f64 / total_size as f64) * 100.0) as u32;

            // 发送进度更新
            let _ = app.emit("update-progress", progress);
        }

        // 执行安装程序
        #[cfg(windows)]
        {
            use std::process::Command;

            Command::new(&installer_path)
                .arg("/S") // 静默安装
                .spawn()
                .map_err(|e| format!("Failed to execute installer: {}", e))?;

            // 等待一秒后退出应用
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::process::exit(0);
        }

        #[cfg(not(windows))]
        {
            return Err("Auto-update is Windows-only".to_string());
        }
    }

    /// 清理旧的安装程序
    pub fn cleanup(&self) -> Result<(), String> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)
                .map_err(|e| format!("Cleanup failed: {}", e))?;
        }
        Ok(())
    }
}
