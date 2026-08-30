//! 前端日志落盘
//!
//! 前端 [`crate::utils/logger`] 通过 `write_log` 命令将日志写入磁盘:
//! - `mercurial-player.log`:当前这次运行的日志
//! - `mercurial-player-prev.log`:上一次运行的日志(启动时轮转)
//!
//! 日志目录为主程序同级的 `logs/` 文件夹(与 plugins/、screenshots/ 一致),
//! 便携式安装时随程序目录一起移动、删除。
//!
//! 轮转在 [`init_log_rotation`](setup 阶段)执行:若上次运行的日志存在,
//! 先删除旧 prev 再将当前日志重命名为 prev,随后开始写入新的当前日志。

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;
use tauri::command;

use crate::error::AppError;

/// 当前运行日志文件名
const CURRENT_LOG_FILE: &str = "mercurial-player.log";
/// 上一次运行日志文件名
const PREV_LOG_FILE: &str = "mercurial-player-prev.log";

/// 当前日志文件路径(setup 阶段初始化;未初始化时 write_log 静默丢弃)
static LOG_FILE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 主程序同级 logs/ 目录(与 plugins/、screenshots/ 的解析方式一致)
fn log_dir() -> Result<PathBuf, AppError> {
    let exe_path = std::env::current_exe().map_err(|e| AppError::msg(format!("无法获取可执行文件路径: {e}")))?;
    let exe_dir = exe_path.parent().ok_or("无法获取可执行文件目录")?;
    Ok(exe_dir.join("logs"))
}

/// setup 阶段调用:轮转日志并记录当前日志路径
pub fn init_log_rotation() {
    let log_dir = match log_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("Failed to resolve log dir, file logging disabled: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        log::warn!("Failed to create log dir {}, file logging disabled: {e}", log_dir.display());
        return;
    }

    let current = log_dir.join(CURRENT_LOG_FILE);
    let prev = log_dir.join(PREV_LOG_FILE);

    if current.exists() {
        // Windows 上 rename 不能覆盖已存在的目标文件,先移除旧 prev
        if prev.exists() {
            if let Err(e) = std::fs::remove_file(&prev) {
                log::warn!("Failed to remove old prev log: {e}");
            }
        }
        match std::fs::rename(&current, &prev) {
            Ok(()) => log::info!("Rotated previous log to {}", prev.display()),
            Err(e) => log::warn!("Failed to rotate log file: {e}"),
        }
    }

    let _ = LOG_FILE_PATH.set(current);
}

/// 前端日志条目(对应 src/types 的 LogData,忽略前端专有字段)
#[derive(Deserialize)]
pub struct FrontendLogEntry {
    timestamp: String,
    level: String,
    message: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    args: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    stack: Option<String>,
}

impl FrontendLogEntry {
    fn format_line(&self) -> String {
        let date = self.date.as_deref().unwrap_or("----/--/--");
        let mut line = format!("[{date} {}] [{}] {}", self.timestamp, self.level, self.message);
        if let Some(args) = &self.args {
            for arg in args {
                line.push(' ');
                line.push_str(&arg.to_string());
            }
        }
        if let Some(stack) = &self.stack {
            line.push('\n');
            line.push_str(stack);
        }
        line.push('\n');
        line
    }
}

/// 写入一条前端日志到当前运行日志文件
///
/// 参数名 `log_data` 对应前端 invoke 的 `logData`(Tauri 命令参数 JS 侧为 camelCase),
/// 前端 logger.ts 发送的就是 `logData`,命名必须一致,否则命令因参数缺失而失败。
#[command]
pub fn write_log(log_data: FrontendLogEntry) -> Result<(), AppError> {
    let Some(path) = LOG_FILE_PATH.get() else {
        return Ok(());
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(log_data.format_line().as_bytes())?;
    Ok(())
}
