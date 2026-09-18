//! Lrclib 歌词来源

use crate::error::AppError;
use crate::lyrics::{LyricCandidate, LyricQuery, LyricsData};
use crate::media::http_client::{get, read_response_text, send_with_retry};
use serde::Deserialize;

/// Lrclib 条目（响应字段为 camelCase，需 rename）
#[derive(Debug, Deserialize)]
struct LrclibItem {
    #[serde(default)]
    #[allow(dead_code)]
    #[serde(rename = "id")]
    pub id_raw: i64,
    #[serde(default)]
    #[serde(rename = "trackName")]
    pub track_name: String,
    #[serde(default)]
    #[serde(rename = "artistName")]
    pub artist_name: String,
    #[serde(default)]
    #[serde(rename = "albumName")]
    pub album_name: String,
    /// 返回字段为秒
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    #[serde(rename = "syncedLyrics")]
    pub synced_lyrics: Option<String>,
    #[serde(default)]
    #[serde(rename = "plainLyrics")]
    pub plain_lyrics: Option<String>,
}

/// 从响应 JSON 解析条目；日志保留原始响应，便于调试
fn parse_items(text: &str) -> Vec<LrclibItem> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(v) = serde_json::from_str::<Vec<LrclibItem>>(text) {
        return v;
    }
    if let Ok(single) = serde_json::from_str::<LrclibItem>(text) {
        return vec![single];
    }
    Vec::new()
}

fn candidate_from_item(item: &LrclibItem) -> Option<LyricCandidate> {
    // 优先同步歌词，否则纯文本；两者皆空视为无可供歌词
    let lyric = item
        .synced_lyrics
        .as_deref()
        .or(item.plain_lyrics.as_deref())
        .unwrap_or("")
        .trim();
    if lyric.is_empty() {
        return None;
    }
    Some(LyricCandidate {
        id: item.id_raw.to_string(),
        title: item.track_name.clone(),
        artist: item.artist_name.clone(),
        album: item.album_name.clone(),
        duration_ms: (item.duration * 1000.0) as i64,
        bundle: LyricsData {
            lrc: lyric.to_string(),
            tlyric: String::new(),
            romalrc: String::new(),
            karaoke: String::new(),
        },
    })
}

/// 精确获取（method "get"）：按歌名+艺人+毫秒时长
async fn get_items(query: &LyricQuery) -> Result<Vec<LyricCandidate>, AppError> {
    let url = "https://lrclib.net/api/get";
    let mut req = get(url)?.header("User-Agent", "MercurialPlayer/0.1 (desktop tauri)");
    req = req.query(&[
        ("track_name", query.title.as_str()),
        ("artist_name", query.artist.as_str()),
        ("duration", &query.duration_ms.to_string()),
    ]);
    let resp = send_with_retry(req).await?;
    let status = resp.status();
    // 404 = 未找到，不算错误
    if status == 404 {
        return Ok(Vec::new());
    }
    let text = read_response_text(resp).await?;
    if !status.is_success() {
        return Ok(Vec::new());
    }
    Ok(parse_items(&text)
        .iter()
        .filter_map(candidate_from_item)
        .collect())
}

/// 模糊搜索（method "search"）：取前 limit 个候选
async fn search_items(query: &LyricQuery, limit: u32) -> Result<Vec<LyricCandidate>, AppError> {
    let url = "https://lrclib.net/api/search";
    let mut req = get(url)?.header("User-Agent", "MercurialPlayer/0.1 (desktop tauri)");
    req = req.query(&[(
        "q",
        format!("{}{}", query.title, {
            if query.artist.is_empty() {
                String::new()
            } else {
                format!(" {}", query.artist)
            }
        })
        .as_str(),
    )]);
    let resp = send_with_retry(req).await?;
    let status = resp.status();
    if !status.is_success() {
        return Ok(Vec::new());
    }
    let text = read_response_text(resp).await?;
    Ok(parse_items(&text)
        .iter()
        .filter_map(candidate_from_item)
        .take(limit as usize)
        .collect())
}

/// Lrclib 入口：按 method 分发
pub async fn search_candidates(
    method: &str,
    query: &LyricQuery,
    limit: u32,
) -> Result<Vec<LyricCandidate>, AppError> {
    // `method == "search"` 直接走模糊搜索；其余（默认）精确优先，未命中再回退模糊
    if method != "search" {
        let exact = get_items(query).await?;
        if !exact.is_empty() {
            return Ok(exact.into_iter().take(limit as usize).collect());
        }
    }
    search_items(query, limit).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_单个与数组() {
        let single = r#"{"id":1,"trackName":"T","artistName":"A","albumName":"AL","duration":100,"syncedLyrics":"[00:01.00]x","plainLyrics":"x"}"#;
        let items = parse_items(single);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].track_name, "T");
        assert_eq!(items[0].synced_lyrics.as_deref(), Some("[00:01.00]x"));

        let arr =
            r#"[{"id":1,"trackName":"T","duration":0},{"id":2,"trackName":"U","duration":0}]"#;
        assert_eq!(parse_items(arr).len(), 2);
    }

    #[test]
    fn candidate_空歌词返回none() {
        let item = LrclibItem {
            id_raw: 1,
            track_name: "T".into(),
            artist_name: String::new(),
            album_name: String::new(),
            duration: 0.0,
            synced_lyrics: None,
            plain_lyrics: None,
        };
        assert!(candidate_from_item(&item).is_none());
    }

    #[test]
    fn invalid_json_返回空() {
        assert!(parse_items("not json").is_empty());
    }
}
