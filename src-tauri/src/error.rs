//! 应用统一错误类型
//!
//! 提供 [`AppError`] 作为 Tauri 命令层与核心模块（audio/config/plugins/updater）的
//! 统一错误类型；命令层通过 [`AppError::Serialize`] 实现序列化为 Display 字符串，
//! 与历史 `Result<T, String>` 在 IPC 上的表现一致，前端契约不变。
//!
//! 尚未迁移的模块（media/system/taskbar 核心实现）仍返回 `Result<T, String>`，
//! 命令层通过 `From<String>` 自动转换；`From<AppError> for String` 保证反向兼容，
//! 新代码可在内部返回 `AppError` 并用 `?` 直接接入旧签名。
//!
//! ## 示例
//!
//! ```no_run
//! use mercurial_player::error::AppError;
//!
//! fn read_config(path: &str) -> Result<String, AppError> {
//!     std::fs::read_to_string(path).map_err(AppError::from) // io::Error -> AppError
//! }
//!
//! fn legacy_api() -> Result<String, String> {
//!     let content = read_config("config.json")?; // AppError -> String 自动转换
//!     Ok(content)
//! }
//! ```

use std::fmt;

// ── Tauri 命令层序列化 ─────────────────────────────────────────────────
//
// Tauri 要求 command 的错误类型实现 Serialize。这里序列化为 Display 字符串，
// 与历史 `Result<T, String>` 在 IPC 上的表现完全一致，前端契约不变；
// 未来若需要结构化错误（分类码 + 消息），只需改这一处。

impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// 应用统一错误类型
///
/// 覆盖项目中常见的错误分类，每个变体携带足够上下文以便诊断。
#[derive(Debug)]
pub enum AppError {
    /// 文件/目录 IO 错误
    Io(std::io::Error),
    /// JSON 序列化/反序列化错误
    Serde(serde_json::Error),
    /// Mutex/RwLock 中毒错误（理论上 `lock_or_log!` 已自动恢复，此变体用于显式传播）
    Lock(String),
    /// 配置加载/保存/解析错误
    Config(String),
    /// 音频解码/播放/设备错误
    Audio(String),
    /// 网络/API 请求错误（HTTP 状态码、解析失败、重试耗尽等）
    Network(String),
    /// 路径不安全、不存在或无权限
    Path(String),
    /// 插件加载/执行/卸载错误
    Plugin(String),
    /// 全文索引（Tantivy）操作错误
    Index(String),
    /// 其他未分类错误
    Other(String),
}

impl AppError {
    /// 从字符串消息快速构造 `Other` 变体
    #[must_use]
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Other(s.into())
    }

    /// 转为 `String`，与现有 `Result<T, String>` 兼容
    #[must_use]
    pub fn into_string(self) -> String {
        self.to_string()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO 错误: {e}"),
            Self::Serde(e) => write!(f, "序列化错误: {e}"),
            Self::Lock(s) => write!(f, "锁错误: {s}"),
            Self::Config(s) => write!(f, "配置错误: {s}"),
            Self::Audio(s) => write!(f, "音频错误: {s}"),
            Self::Network(s) => write!(f, "网络错误: {s}"),
            Self::Path(s) => write!(f, "路径错误: {s}"),
            Self::Plugin(s) => write!(f, "插件错误: {s}"),
            Self::Index(s) => write!(f, "索引错误: {s}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Serde(e) => Some(e),
            _ => None,
        }
    }
}

// ── From 转换：让 `?` 自动传播常见错误类型 ──────────────────────────────

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

// ── 向下兼容：AppError -> String，让旧 Result<T, String> 代码可用 ? ─────

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}
