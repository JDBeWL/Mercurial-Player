//! 网易云音乐歌词 API
//!
//! 提供从网易云音乐搜索和获取歌词的功能

use crate::media::http_client::get_client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri_plugin_http::reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, HeaderMap, HeaderValue, REFERER, USER_AGENT,
};

/// 最大重试次数
const MAX_RETRIES: u32 = 3;

/// 带指数退避的请求重试
/// 对网络错误（连接超时、DNS 失败等）重试，对 HTTP 错误状态码不重试
async fn send_with_retry(
    request_builder: tauri_plugin_http::reqwest::RequestBuilder,
) -> Result<tauri_plugin_http::reqwest::Response, String> {
    let mut last_err = String::new();
    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            tokio::time::sleep(delay).await;
            log::debug!("重试请求 (第 {attempt} 次)...");
        }
        match request_builder.try_clone().ok_or("请求不可重试")?.send().await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_err = format!("{e}");
                log::warn!("请求失败 (第 {} 次): {e}", attempt + 1);
            }
        }
    }
    Err(format!("请求失败，已重试 {MAX_RETRIES} 次: {last_err}"))
}

/// 搜索结果中的歌曲信息
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ArtistInfo {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AlbumInfo {
    #[serde(default)]
    pub name: String,
}

/// 歌词响应
#[derive(Debug, Deserialize)]
struct LyricResponse {
    code: i32,
    lrc: Option<LyricContent>,
    tlyric: Option<LyricContent>,
    romalrc: Option<LyricContent>,
}

#[derive(Debug, Deserialize)]
struct LyricContent {
    lyric: Option<String>,
}

/// 返回给前端的歌词数据
#[derive(Debug, Serialize)]
pub struct LyricsData {
    pub lrc: String,
    pub tlyric: String,
    pub romalrc: String,
}

/// 返回给前端的搜索结果
#[derive(Debug, Serialize)]
pub struct SearchSongResult {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: i64,
}

/// CloudSearch API 响应结构
#[derive(Debug, Deserialize)]
struct CloudSearchResponse {
    code: i32,
    result: Option<CloudSearchResult>,
}

#[derive(Debug, Deserialize)]
struct CloudSearchResult {
    songs: Option<Vec<CloudSearchSong>>,
}

#[derive(Debug, Deserialize)]
struct CloudSearchSong {
    id: i64,
    name: String,
    #[serde(default)]
    ar: Vec<ArtistInfo>,
    al: Option<AlbumInfo>,
    #[serde(default)]
    dt: i64,
}

/// 安全截取字符串前 N 个字符（避免在多字节字符中间截断）
fn safe_truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// 构建请求头 - 模拟浏览器访问网易云音乐网页
fn build_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    headers.insert(REFERER, HeaderValue::from_static("https://music.163.com/"));
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        ),
    );

    headers
}

/// 搜索歌曲 - 使用 Web API
pub async fn search_songs(
    keyword: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchSongResult>, String> {
    let client = get_client();

    // 使用 cloudsearch API（更稳定）
    let url = "https://music.163.com/api/cloudsearch/pc";

    let params = [
        ("s", keyword),
        ("type", "1"),
        ("limit", &limit.to_string()),
        ("offset", &offset.to_string()),
    ];

    let response = send_with_retry(
        client
            .post(url)
            .headers(build_headers())
            .form(&params),
    )
    .await?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Read response failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("HTTP error: {status} - {response_text}"));
    }

    let data: CloudSearchResponse = serde_json::from_str(&response_text).map_err(|e| {
        format!(
            "Parse response failed: {e} - Response: {}",
            safe_truncate(&response_text, 200)
        )
    })?;

    if data.code != 200 {
        return Err(format!("API error: code {}", data.code));
    }

    let songs: Vec<SearchSongResult> = data
        .result
        .and_then(|r| r.songs)
        .unwrap_or_default()
        .into_iter()
        .map(|s| SearchSongResult {
            id: s.id.to_string(),
            name: s.name,
            artist: s
                .ar
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join("/"),
            album: s.al.map(|a| a.name).unwrap_or_default(),
            duration: s.dt,
        })
        .collect();

    Ok(songs)
}

/// 获取歌词 - 使用 Web API
pub async fn get_lyrics(song_id: &str) -> Result<LyricsData, String> {
    let client = get_client();

    let url = format!("https://music.163.com/api/song/lyric?id={song_id}&lv=-1&tv=-1&rv=-1&kv=-1");

    let response = send_with_retry(client.get(&url).headers(build_headers())).await?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Read response failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("HTTP error: {status} - {response_text}"));
    }

    let data: LyricResponse = serde_json::from_str(&response_text).map_err(|e| {
        format!(
            "Parse response failed: {e} - Response: {}",
            safe_truncate(&response_text, 200)
        )
    })?;

    if data.code != 200 {
        return Err(format!("API error: code {}", data.code));
    }

    Ok(LyricsData {
        lrc: data.lrc.and_then(|l| l.lyric).unwrap_or_default(),
        tlyric: data.tlyric.and_then(|l| l.lyric).unwrap_or_default(),
        romalrc: data.romalrc.and_then(|l| l.lyric).unwrap_or_default(),
    })
}
