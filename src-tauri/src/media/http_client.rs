//! HTTP 客户端单例
//!
//! 提供可重用的 HTTP 客户端，避免重复创建连接
//!
//! 安全约束：`capabilities/*.json` 中 `http:default` 的 URL scope **只约束
//! 前端 JS** 经 tauri-plugin-http 绑定发起的 fetch。Rust 侧通过本模块持有的
//! `Client` 发出的请求不经过该 scope 校验（scope 是插件命令层的检查，不是
//! 网络层的拦截）。因此出网目标必须在本模块显式白名单化 —— 业务代码不要
//! 直接对任意 URL 发请求，请走 [`get`] / [`post`]。

use crate::error::AppError;
use std::sync::LazyLock;
use tauri_plugin_http::reqwest::{Client, RequestBuilder, Url};

/// 允许 Rust 侧 HTTP 客户端访问的目标主机
///
/// 新增出网域名时在此登记；若该请求将来也会从前端发起，还需在
/// `src-tauri/capabilities/default.json` 的 `http` scope 中同步添加。
const ALLOWED_HOSTS: &[&str] = &["music.163.com"];

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

/// 校验 URL 是否允许由 Rust 侧客户端访问（HTTPS + 主机白名单）
fn assert_url_allowed(url: &str) -> Result<(), AppError> {
    let parsed = Url::parse(url).map_err(|e| AppError::msg(format!("Invalid URL: {e}")))?;

    // 明文 HTTP 的响应可被中间人篡改，而这里的响应体会被反序列化后直接入库
    if parsed.scheme() != "https" {
        return Err(AppError::msg(
            "Rust 侧 HTTP 客户端仅允许 HTTPS 请求".to_string(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::msg("URL host is missing".to_string()))?
        .to_ascii_lowercase();

    let allowed = ALLOWED_HOSTS
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")));
    if !allowed {
        return Err(AppError::msg(format!(
            "Rust 侧 HTTP 客户端不允许访问该主机：{host}"
        )));
    }
    Ok(())
}

/// 发起受白名单约束的 GET 请求
pub fn get(url: &str) -> Result<RequestBuilder, AppError> {
    assert_url_allowed(url)?;
    Ok(client()?.get(url))
}

/// 发起受白名单约束的 POST 请求
pub fn post(url: &str) -> Result<RequestBuilder, AppError> {
    assert_url_allowed(url)?;
    Ok(client()?.post(url))
}

/// 获取全局 HTTP 客户端
///
/// 仅供本模块 [`get`] / [`post`] 使用：业务代码不应绕开主机白名单
/// 直接对任意 URL 发请求。
fn client() -> Result<&'static Client, AppError> {
    HTTP_CLIENT
        .as_ref()
        .map_err(|e| AppError::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 白名单主机放行() {
        assert!(assert_url_allowed("https://music.163.com/api/song/lyric?id=1").is_ok());
        assert!(assert_url_allowed("https://MUSIC.163.com/api/cloudsearch/pc").is_ok());
        assert!(assert_url_allowed("https://a.music.163.com/x").is_ok());
    }

    #[test]
    fn 非白名单主机拒绝() {
        assert!(assert_url_allowed("https://example.com/evil").is_err());
        assert!(assert_url_allowed("https://music.163.com.evil.net/").is_err());
        assert!(assert_url_allowed("https://evil-music.163.com.example.net/").is_err());
    }

    #[test]
    fn 非https拒绝() {
        assert!(assert_url_allowed("http://music.163.com/api").is_err());
        assert!(assert_url_allowed("file:///etc/passwd").is_err());
    }

    #[test]
    fn 非法url拒绝() {
        assert!(assert_url_allowed("not a url").is_err());
        assert!(assert_url_allowed("").is_err());
    }
}
