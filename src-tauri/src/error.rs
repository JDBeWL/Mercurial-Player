//! 应用统一错误类型
//!
//! 提供 [`AppError`] 作为 Tauri 命令层与核心模块（audio/config/plugins/updater）的
//! 统一错误类型；命令层通过 [`AppError::Serialize`] 实现序列化为 Display 字符串，
//! 与历史 `Result<T, String>` 在 IPC 上的表现一致，前端契约不变。
//!
//! 全部核心模块（media/system/taskbar/audio 内部实现）均已迁移到 `AppError`。
//! `From<String> for AppError` 与 `From<AppError> for String` 双向转换保证新旧
//! 签名互通：字符串错误经 `?` 自动进入 `Other` 变体（Display 原样输出，不改变
//! IPC 错误文案），旧 `Result<T, String>` 签名也可直接用 `?` 接收 `AppError`。
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

/// 从错误消息中的路径段提取末尾文件名(纯文本级,不依赖宿主平台)。
///
/// 不能用 `std::path::Path::file_name()`:Linux/macOS 上 `\` 不是路径分隔符,
/// `Path::new(r"D:\a\b\f.txt").file_name()` 会返回整段字符串导致脱敏失效;
/// Windows 盘符/UNC 前缀(反斜杠或正斜杠风格)统一按 `\` `/` 双分隔符切分。
fn file_name_in_segment(segment: &str) -> Option<&str> {
    // 取最后一个分隔符之后的部分;以分隔符结尾(如裸盘符 `C:\`)视为无文件名
    match segment.rfind(['\\', '/']) {
        Some(idx) if idx + 1 < segment.len() => Some(&segment[idx + 1..]),
        Some(_) => None,
        // 无任何分隔符(理论不会出现,因调用方已识别盘符/UNC 前缀)
        None => Some(segment),
    }
}

/// 抹去错误信息中的绝对路径(Windows 盘符路径与 UNC 路径)。
///
/// `Display` 文案会经 IPC 原样回传前端，用户可见的错误不应暴露本机目录
/// 结构；完整路径仅保存在 `From` 转换处的 `log::debug!` 中。路径替换为
/// 保留文件名的形式，便于用户定位问题文件。
#[must_use]
fn sanitize_path_in_message(msg: &str) -> String {
    const PATH_END: &[u8] = b" \t\r\n\"'()<>,;";
    let bytes = msg.as_bytes();
    let mut out = String::with_capacity(msg.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        // Windows 盘符路径 (X:\ 或 X:/) 或 UNC 路径 (\\server\share)
        let is_drive = b.is_ascii_alphabetic()
            && i + 2 < bytes.len()
            && bytes[i + 1] == b':'
            && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/');
        let is_unc = b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'\\';
        if is_drive || is_unc {
            let mut j = i + if is_unc { 2 } else { 3 };
            while j < bytes.len() && !PATH_END.contains(&bytes[j]) {
                j += 1;
            }
            let path = &msg[i..j];
            match file_name_in_segment(path) {
                Some(name) if !name.is_empty() => out.push_str(name),
                _ => out.push_str("<路径>"),
            }
            i = j;
        } else {
            // 非 ASCII 字节按 char 边界推进
            let ch_len = msg[i..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&msg[i..i + ch_len]);
            i += ch_len;
        }
    }
    out
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Display 文案经 IPC 回传前端:OS 错误原文可能含绝对路径,统一脱敏
            Self::Io(e) => write!(f, "IO 错误: {}", sanitize_path_in_message(&e.to_string())),
            Self::Serde(e) => {
                write!(
                    f,
                    "序列化错误: {}",
                    sanitize_path_in_message(&e.to_string())
                )
            }
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
        // OS 错误原文可能含绝对路径,完整信息仅供诊断日志,Display 侧已脱敏
        log::debug!("完整 IO 错误(仅供诊断,可能含路径): {e}");
        Self::Io(e)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        log::debug!("完整序列化错误(仅供诊断): {e}");
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

// ── 音频库错误:让 `?` 直接归入 Audio,避免 `.to_string().into()` 落入 Other ──
//
// 注:cpal 0.15+ 已无统一的 `cpal::Error`,错误按操作拆分
// (BuildStreamError/StreamError/DevicesError/...);rodio 0.22 在根路径
// 导出 `PlayError`/`DeviceSinkError`(无 `StreamError`,其由 `PlayError` 携带)。

macro_rules! impl_audio_from {
    ($($ty:ty => $desc:literal),+ $(,)?) => {
        $(
            impl From<$ty> for AppError {
                fn from(err: $ty) -> Self {
                    // 显式具名参数绑定,规避宏 hygiene 下 `{e}` 找不到调用侧变量的情况
                    Self::Audio(format!($desc, e = err))
                }
            }
        )+
    };
}

impl_audio_from!(
    cpal::BuildStreamError => "打开音频输出流失败: {e}",
    cpal::StreamError => "音频流运行错误: {e}",
    cpal::PlayStreamError => "音频流播放错误: {e}",
    cpal::PauseStreamError => "音频流暂停错误: {e}",
    cpal::DefaultStreamConfigError => "获取音频设备默认配置失败: {e}",
    cpal::SupportedStreamConfigsError => "查询音频设备支持配置失败: {e}",
    cpal::DevicesError => "枚举音频设备失败: {e}",
    cpal::DeviceNameError => "读取音频设备名称失败: {e}",
    cpal::DeviceIdError => "解析音频设备标识失败: {e}",
    rodio::PlayError => "播放流创建错误: {e}",
    rodio::DeviceSinkError => "创建设备音频输出失败: {e}",
);

#[cfg(windows)]
impl From<windows_core::Error> for AppError {
    fn from(e: windows_core::Error) -> Self {
        Self::Audio(format!("WASAPI/COM 错误: {e}"))
    }
}

// ── 向下兼容：AppError -> String，让旧 Result<T, String> 代码可用 ? ─────

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_path_in_message;
    use crate::error::AppError;

    #[test]
    fn sanitize_removes_drive_paths_and_keeps_filename() {
        let msg = "IO 错误: 无法打开 D:\\Users\\me\\Music\\song.mp3 (os error 2)";
        assert_eq!(
            sanitize_path_in_message(msg),
            "IO 错误: 无法打开 song.mp3 (os error 2)"
        );
    }

    #[test]
    fn sanitize_handles_forward_slash_and_unc_paths() {
        assert_eq!(
            sanitize_path_in_message("read C:/tmp/cover.png failed"),
            "read cover.png failed"
        );
        assert_eq!(
            sanitize_path_in_message(r"access \\?\D:\a\b\f.txt denied"),
            "access f.txt denied"
        );
        assert_eq!(
            sanitize_path_in_message(r"share \\server\share\dir\f.lrc missing"),
            "share f.lrc missing"
        );
    }

    #[test]
    fn sanitize_leaves_plain_messages_and_chinese_text_intact() {
        let msg = "No such file or directory (os error 2)";
        assert_eq!(sanitize_path_in_message(msg), msg);
        let zh = "无法识别的音频格式";
        assert_eq!(sanitize_path_in_message(zh), zh);
        // 盘符后不带斜杠的裸字母不应被误判为路径
        let version = "版本 C: 更新说明";
        assert_eq!(sanitize_path_in_message(version), version);
    }

    #[test]
    fn io_display_no_longer_leaks_absolute_path() {
        let err = std::io::Error::other("failed to read D:\\secret\\folder\\track.flac");
        let text = AppError::Io(err).to_string();
        assert!(!text.contains("D:\\secret\\folder"), "泄漏绝对路径: {text}");
        assert!(text.contains("track.flac"), "应保留文件名便于定位: {text}");
    }
}
