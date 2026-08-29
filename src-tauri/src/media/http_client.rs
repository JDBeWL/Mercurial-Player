//! HTTP 客户端单例
//!
//! 提供可重用的 HTTP 客户端，避免重复创建连接

use crate::error::AppError;
use std::sync::LazyLock;
use tauri_plugin_http::reqwest::Client;

/// 全局 HTTP 客户端实例
///
/// 构建失败（如 TLS 后端初始化失败）时保存错误，
/// 由调用方决定如何处理，避免首次使用时 panic 导致整个应用崩溃
static HTTP_CLIENT: LazyLock<Result<Client, AppError>> = LazyLock::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}").into())
});

/// 获取全局 HTTP 客户端
pub fn get_client() -> Result<&'static Client, AppError> {
    HTTP_CLIENT
        .as_ref()
        .map_err(|e| AppError::msg(e.to_string()))
}
