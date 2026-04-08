//! 哔哩哔哩搜索 API
//!
//! 提供从哔哩哔哩搜索视频的功能

use tauri_plugin_http::reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use crate::media::http_client::get_client;

/// 搜索结果中的视频信息
#[derive(Debug, Serialize)]
pub struct BilibiliVideoResult {
    pub bvid: String,
    pub title: String,
    pub author: String,
    pub duration: String,
    pub play_count: u64,
    pub url: String,
}

/// 哔哩哔哩API响应结构
#[derive(Debug, Deserialize)]
struct BilibiliSearchResponse {
    code: i32,
    data: Option<BilibiliSearchData>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BilibiliSearchData {
    result: Option<Vec<BilibiliVideoItem>>,
}

#[derive(Debug, Deserialize)]
struct BilibiliVideoItem {
    #[serde(default)]
    bvid: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    duration: String,
    #[serde(default)]
    play: u64,
}

/// 安全截取字符串前 N 个字符（避免在多字节字符中间截断）
fn safe_truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// 构建请求头 - 模拟浏览器访问哔哩哔哩
fn build_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        ),
    );
    
    headers.insert(
        "Referer",
        HeaderValue::from_static("https://www.bilibili.com/"),
    );
    
    headers.insert(
        "Origin",
        HeaderValue::from_static("https://www.bilibili.com"),
    );
    
    headers.insert(
        "Accept",
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
    );
    
    headers.insert(
        "Cookie",
        HeaderValue::from_static("buvid3=INFOC8C4-E12F-4C5C-A7B8-9E8D7F6A5B4C;"),
    );
    
    headers
}

/// 搜索哔哩哔哩视频
pub async fn search_videos(keyword: &str, page: u32, limit: u32) -> Result<Vec<BilibiliVideoResult>, String> {
    let client = get_client();
    
    // 使用哔哩哔哩搜索API
    let url = format!(
        "https://api.bilibili.com/x/web-interface/search/type?search_type=video&keyword={}&page={}&page_size={}&order=totalrank",
        urlencoding::encode(keyword),
        page,
        limit
    );

    let response = client
        .get(&url)
        .headers(build_headers())
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| format!("Read response failed: {e}"))?;
    
    if !status.is_success() {
        return Err(format!("HTTP error: {status} - {}", safe_truncate(&response_text, 200)));
    }

    let data: BilibiliSearchResponse = serde_json::from_str(&response_text)
        .map_err(|e| format!("Parse response failed: {e} - Response: {}", safe_truncate(&response_text, 200)))?;

    if data.code != 0 {
        let msg = data.message.unwrap_or_default();
        return Err(format!("Bilibili API error: code {} - {}", data.code, msg));
    }

    let videos: Vec<BilibiliVideoResult> = data.data
        .and_then(|d| d.result)
        .unwrap_or_default()
        .into_iter()
        .map(|v| BilibiliVideoResult {
            bvid: v.bvid.clone(),
            title: strip_html_tags(&v.title),
            author: v.author.clone(),
            duration: v.duration.clone(),
            play_count: v.play,
            url: format!("https://www.bilibili.com/video/{}", v.bvid),
        })
        .collect();

    Ok(videos)
}

/// 移除HTML标签
fn strip_html_tags(html: &str) -> String {
    let re = regex::Regex::new(r"<[^>]*>").unwrap();
    let result = re.replace_all(html, "");
    result.replace("&quot;", "\"")
         .replace("&amp;", "&")
         .replace("&lt;", "<")
         .replace("&gt;", ">")
         .replace("&nbsp;", " ")
}
