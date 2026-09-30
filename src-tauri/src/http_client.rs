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
use std::time::Duration;
use tauri_plugin_http::reqwest::Response;
use tauri_plugin_http::reqwest::{Client, RequestBuilder, Url};

/// 允许 Rust 侧 HTTP 客户端访问的目标主机
///
/// 新增出网域名时在此登记；若该请求将来也会从前端发起，还需在
/// `src-tauri/capabilities/default.json` 的 `http` scope 中同步添加。
///
/// 歌词多来源：网易云 / Lrclib / QQ 音乐 / 酷狗（酷狗仅登记可 HTTPS 访问的域名）。
const ALLOWED_HOSTS: &[&str] = &[
    // 网易云音乐
    "music.163.com",
    // Lrclib 歌词库
    "lrclib.net",
    // QQ 音乐
    "c.y.qq.com",
    "y.qq.com",
    // 酷狗音乐
    "songsearch.kugou.com",
    "lyrics.kugou.com",
];

/// 全局 HTTP 客户端实例
///
/// 构建失败（如 TLS 后端初始化失败）时保存错误，
/// 由调用方决定如何处理，避免首次使用时 panic 导致整个应用崩溃
static HTTP_CLIENT: LazyLock<Result<Client, AppError>> = LazyLock::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
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

/// 最大重试次数（块外部网络请求，如连接超时/DNS 失败）
const MAX_RETRIES: u32 = 3;

/// 带指数退避的请求重试：网络错误重试，HTTP 错误状态码不重试
pub(crate) async fn send_with_retry(request_builder: RequestBuilder) -> Result<Response, AppError> {
    let mut last_err = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            tokio::time::sleep(delay).await;
            log::debug!("重试请求 (第 {attempt} 次)...");
        }
        match request_builder
            .try_clone()
            .ok_or("请求不可重试")?
            .send()
            .await
        {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_err = format!("{e}");
                log::warn!("请求失败 (第 {} 次): {e}", attempt + 1);
            }
        }
    }
    Err(format!("请求失败，已重试 {MAX_RETRIES} 次: {last_err}").into())
}

/// 响应体最大大小（5MB），防止异常大响应导致内存耗尽
const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;

/// 读取响应体文本，带大小限制
pub(crate) async fn read_response_text(mut response: Response) -> Result<String, AppError> {
    // 优先根据 Content-Length 拒绝过大响应
    if let Some(len) = response.content_length()
        && len as usize > MAX_RESPONSE_SIZE
    {
        return Err(format!("响应过大: {len} 字节（上限 {MAX_RESPONSE_SIZE}）").into());
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Read response failed: {e}"))?
    {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_RESPONSE_SIZE {
            return Err(format!("响应超过大小限制（{MAX_RESPONSE_SIZE} 字节）").into());
        }
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 白名单主机放行() {
        assert!(assert_url_allowed("https://music.163.com/api/song/lyric?id=3425575022").is_ok());
        assert!(assert_url_allowed("https://MUSIC.163.com/api/cloudsearch/pc").is_ok());
        assert!(assert_url_allowed("https://a.music.163.com/x").is_ok());
        assert!(assert_url_allowed("https://lrclib.net/api/get").is_ok());
        assert!(assert_url_allowed("https://c.y.qq.com/soso/fcgi-bin/client_search_cp").is_ok());
        assert!(assert_url_allowed("https://y.qq.com/").is_ok());
        assert!(assert_url_allowed("https://songsearch.kugou.com/song_search_v2").is_ok());
        assert!(assert_url_allowed("https://lyrics.kugou.com/v1/search").is_ok());
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
