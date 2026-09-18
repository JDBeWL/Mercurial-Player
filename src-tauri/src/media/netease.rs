//! 网易云音乐歌词 API
//!
//! 提供从网易云音乐搜索和获取歌词的功能

use crate::error::AppError;
use crate::media::http_client::{get, post, read_response_text, send_with_retry};
use serde::{Deserialize, Serialize};
use tauri_plugin_http::reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, HeaderMap, HeaderValue, REFERER, USER_AGENT,
};

/// 搜索/获取歌词中的歌曲信息
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
#[derive(Debug, Clone, Serialize)]
pub struct LyricsData {
    pub lrc: String,
    pub tlyric: String,
    pub romalrc: String,
    /// ASS 逐字歌词（原文逐字 + 译文/罗马音普通行），空表示无逐字
    #[serde(skip_serializing_if = "String::is_empty")]
    pub karaoke: String,
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
) -> Result<Vec<SearchSongResult>, AppError> {
    // 使用 cloudsearch API (post() 内部执行主机白名单校验)
    let url = "https://music.163.com/api/cloudsearch/pc";

    let params = [
        ("s", keyword),
        ("type", "1"),
        ("limit", &limit.to_string()),
        ("offset", &offset.to_string()),
    ];

    let response = send_with_retry(post(url)?.headers(build_headers()).form(&params)).await?;

    let status = response.status();
    let response_text = read_response_text(response).await?;

    if !status.is_success() {
        return Err(format!("HTTP error: {status} - {response_text}").into());
    }

    let data: CloudSearchResponse = serde_json::from_str(&response_text).map_err(|e| {
        format!(
            "Parse response failed: {e} - Response: {}",
            safe_truncate(&response_text, 200)
        )
    })?;

    if data.code != 200 {
        return Err(format!("API error: code {}", data.code).into());
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
pub async fn get_lyrics(song_id: &str) -> Result<LyricsData, AppError> {
    // 歌曲 ID 必须为纯数字，防止 URL 参数注入
    if song_id.is_empty() || !song_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("非法的歌曲 ID".to_string().into());
    }

    // get() 内部执行主机白名单校验
    let url = format!("https://music.163.com/api/song/lyric?id={song_id}&lv=-1&tv=-1&rv=-1&kv=-1");

    let response = send_with_retry(get(&url)?.headers(build_headers())).await?;

    let status = response.status();
    let response_text = read_response_text(response).await?;

    if !status.is_success() {
        return Err(format!("HTTP error: {status} - {response_text}").into());
    }

    let data: LyricResponse = serde_json::from_str(&response_text).map_err(|e| {
        format!(
            "Parse response failed: {e} - Response: {}",
            safe_truncate(&response_text, 200)
        )
    })?;

    if data.code != 200 {
        return Err(format!("API error: code {}", data.code).into());
    }

    Ok(LyricsData {
        lrc: data.lrc.and_then(|l| l.lyric).unwrap_or_default(),
        tlyric: data.tlyric.and_then(|l| l.lyric).unwrap_or_default(),
        romalrc: data.romalrc.and_then(|l| l.lyric).unwrap_or_default(),
        karaoke: String::new(),
    })
}
