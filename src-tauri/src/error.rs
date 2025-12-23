//! 错误处理模块
//!
//! 定义应用程序的自定义错误类型和结果类型。

use std::fmt;

/// 自定义错误类型，用于音乐播放器应用
#[derive(Debug)]
#[allow(dead_code)]
pub enum AppError {
    /// IO 相关错误
    Io(std::io::Error),
    /// 音频解码错误
    AudioDecoder(String),
    /// 文件不存在
    FileNotFound(String),
    /// 无效的文件路径
    InvalidPath(String),
    /// 配置相关错误
    Config(String),
    /// Tauri 相关错误
    Tauri(tauri::Error),
    /// JSON 序列化/反序列化错误
    Json(serde_json::Error),
    /// 其他通用错误
    Other(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "IO error: {err}"),
            Self::AudioDecoder(err) => write!(f, "Audio decoder error: {err}"),
            Self::FileNotFound(path) => write!(f, "File not found: {path}"),
            Self::InvalidPath(path) => write!(f, "Invalid file path: {path}"),
            Self::Config(err) => write!(f, "Configuration error: {err}"),
            Self::Tauri(err) => write!(f, "Tauri error: {err}"),
            Self::Json(err) => write!(f, "JSON error: {err}"),
            Self::Other(err) => write!(f, "Error: {err}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Tauri(err) => Some(err),
            Self::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        Self::Tauri(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        Self::Other(err)
    }
}

impl From<&str> for AppError {
    fn from(err: &str) -> Self {
        Self::Other(err.to_string())
    }
}

/// 应用结果类型
#[allow(dead_code)]
pub type AppResult<T> = Result<T, AppError>;
