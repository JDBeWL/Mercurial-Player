//! HTTP 客户端单例
//!
//! 提供可重用的 HTTP 客户端，避免重复创建连接

use std::sync::LazyLock;
use reqwest::Client;

/// 全局 HTTP 客户端实例
static HTTP_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to create HTTP client")
});

/// 获取全局 HTTP 客户端
#[must_use]
pub fn get_client() -> &'static Client {
    &HTTP_CLIENT
}
