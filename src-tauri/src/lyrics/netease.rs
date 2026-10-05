//! 网易云音乐歌词 provider
//!
//! 提供从网易云音乐搜索和获取歌词的功能

use crate::error::AppError;
use crate::http_client::{get, post, read_response_text, send_with_retry};
use crate::lyrics::LyricsData;
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
    yrc: Option<LyricContent>,
}

#[derive(Debug, Deserialize)]
struct LyricContent {
    lyric: Option<String>,
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

/// `mm:ss.xx` / `hh:mm:ss.xx` -> 毫秒
fn lrc_time_ms(token: &str) -> Option<i64> {
    let parts: Vec<&str> = token.split(':').collect();
    let secs: f64 = match parts.as_slice() {
        [m, s] => m.parse::<f64>().ok()? * 60.0 + s.parse::<f64>().ok()?,
        [h, m, s] => {
            h.parse::<f64>().ok()? * 3600.0
                + m.parse::<f64>().ok()? * 60.0
                + s.parse::<f64>().ok()?
        }
        _ => return None,
    };
    Some((secs * 1000.0).round() as i64)
}

/// 普通 LRC -> `[(起始ms, 文本)]`：一行多时间戳展开成多条，`[by:xxx]` 之类元数据行跳过
fn timed_lrc_lines(text: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times: Vec<i64> = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(close) = stripped.find(']') else {
                break;
            };
            let Some(ms) = lrc_time_ms(&stripped[..close]) else {
                break;
            };
            times.push(ms);
            rest = stripped[close + 1..].trim_start();
        }
        if times.is_empty() || rest.is_empty() {
            continue;
        }
        for ms in times {
            out.push((ms, rest.to_string()));
        }
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

/// 解析 yrc 行正文 `(字起始ms,字时长ms,标志)字…` -> 逐字单元
fn parse_yrc_words(content: &str) -> Vec<super::ass::Word> {
    let mut out: Vec<super::ass::Word> = Vec::new();
    let mut pos = 0usize;
    while content[pos..].starts_with('(') {
        let Some(close_rel) = content[pos..].find(')') else {
            break;
        };
        let close = pos + close_rel;
        let mut fields = content[pos + 1..close].split(',');
        let (Some(start_raw), Some(dur_raw)) = (fields.next(), fields.next()) else {
            break;
        };
        let (Ok(start_ms), Ok(dur_ms)) = (
            start_raw.trim().parse::<i64>(),
            dur_raw.trim().parse::<i64>(),
        ) else {
            break;
        };
        // 字文本取到下一个 `(` 之前，中间的空格属于正文
        let word_end = content[close + 1..]
            .find('(')
            .map(|i| close + 1 + i)
            .unwrap_or(content.len());
        let text = &content[close + 1..word_end];
        if !text.is_empty() {
            out.push(super::ass::Word {
                start_ms,
                end_ms: start_ms + dur_ms.max(10),
                text: text.to_string(),
            });
        }
        pos = word_end;
    }
    out
}

/// 网易逐字歌词 -> ASS：原文行用 `{\kf}` 逐字，译文/罗马音为普通行。
/// 一行只有一个字（如 `[0,1000](0,1000,0) 作曲 : X` 这类署名行）时退化为普通文本行，
/// 逐字行全部解析不出来则返回 None，让上层回落到 LRC 通路
fn yrc_to_karaoke_ass(yrc: &str, trans: &str, roma: &str, title: &str) -> Option<String> {
    struct Row {
        start_ms: i64,
        end_ms: i64,
        words: Vec<super::ass::Word>,
        text: String,
    }

    let mut rows: Vec<Row> = Vec::new();
    for raw in yrc.lines() {
        let Some(rest) = raw.trim().strip_prefix('[') else {
            continue;
        };
        let Some(close) = rest.find(']') else {
            continue;
        };
        let mut head = rest[..close].split(',');
        let (Some(start_raw), Some(dur_raw)) = (head.next(), head.next()) else {
            continue;
        };
        let (Ok(start_ms), Ok(dur_ms)) = (start_raw.parse::<i64>(), dur_raw.parse::<i64>()) else {
            continue;
        };
        let words = parse_yrc_words(&rest[close + 1..]);
        if words.is_empty() {
            continue;
        }
        let last_word_end = words.iter().map(|w| w.end_ms).max().unwrap_or(start_ms);
        rows.push(Row {
            start_ms,
            end_ms: (start_ms + dur_ms).max(last_word_end),
            text: words.iter().map(|w| w.text.as_str()).collect(),
            words,
        });
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort_by_key(|r| r.start_ms);

    let mut out = super::ass::ass_header(title);
    for (text, style) in [(trans, "ts"), (roma, "roma")] {
        let lines = timed_lrc_lines(text);
        for (i, (start_ms, plain)) in lines.iter().enumerate() {
            let end_ms = lines
                .get(i + 1)
                .map_or(start_ms + 5000, |(next, _)| *next)
                .max(*start_ms + 10);
            out.push_str(&super::ass::dialogue(*start_ms, end_ms, style, None, plain));
        }
    }
    for row in &rows {
        out.push_str(&super::ass::dialogue(
            row.start_ms,
            row.end_ms,
            "orig",
            Some(&row.words),
            &row.text,
        ));
    }
    Some(out)
}

/// 获取歌词 - 使用 Web API
pub async fn get_lyrics(song_id: &str) -> Result<LyricsData, AppError> {
    // 歌曲 ID 必须为纯数字，防止 URL 参数注入
    if song_id.is_empty() || !song_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("非法的歌曲 ID".to_string().into());
    }

    // get() 内部执行主机白名单校验。yv=-1 才会带回逐字（yrc）；kv=-1 试过，
    // 响应的 klyric 恒为空串，逐字一律走 yrc
    let url = format!("https://music.163.com/api/song/lyric?id={song_id}&lv=-1&tv=-1&rv=-1&yv=-1");

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

    let lrc = data.lrc.and_then(|l| l.lyric).unwrap_or_default();
    let tlyric = data.tlyric.and_then(|l| l.lyric).unwrap_or_default();
    let romalrc = data.romalrc.and_then(|l| l.lyric).unwrap_or_default();
    // 只有真拿到逐字才生成 ASS，否则留空让前端按 LRC 走 kind（原文/译文/罗马音）切换
    let karaoke = data
        .yrc
        .and_then(|y| y.lyric)
        .filter(|text| !text.trim().is_empty())
        .and_then(|text| yrc_to_karaoke_ass(&text, &tlyric, &romalrc, song_id))
        .unwrap_or_default();

    Ok(LyricsData {
        lrc,
        tlyric,
        romalrc,
        karaoke,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yrc_生成逐字_ass() {
        // 实测载荷形态：字起始是绝对毫秒，行尾 = 行起始 + 行时长
        let yrc = "[24810,3150](24810,630,0)这(25440,300,0)一(25740,390,0)路\n";
        let ass = yrc_to_karaoke_ass(yrc, "", "", "1").expect("应生成 ASS");
        assert!(ass.contains("Style: orig,Arial,20,"));
        assert!(ass.contains(
            "Dialogue: 0,00:00:24.81,00:00:27.96,orig,,0,0,0,,{\\kf63}这{\\kf30}一{\\kf39}路"
        ));
    }

    #[test]
    fn yrc_署名行退化为普通行() {
        let yrc = "[0,1000](0,1000,0) 作曲 : X\n";
        let ass = yrc_to_karaoke_ass(yrc, "", "", "1").expect("单字行也应产出");
        assert!(ass.contains(",orig,,0,0,0,, 作曲 : X"));
        assert!(!ass.contains("\\kf"));
    }

    #[test]
    fn yrc_无逐字时返回_none() {
        assert!(yrc_to_karaoke_ass("[24810,3150]没有字标记", "", "", "1").is_none());
        assert!(yrc_to_karaoke_ass("", "", "", "1").is_none());
    }

    #[test]
    fn 译文与罗马音成普通行() {
        let yrc = "[1000,2000](1000,900,0)あ(1900,1100,0)い\n";
        let ass = yrc_to_karaoke_ass(yrc, "[00:01.00]中文译文", "[00:01.00]romaji", "1").unwrap();
        assert!(ass.contains(",ts,,0,0,0,,中文译文"));
        assert!(ass.contains(",roma,,0,0,0,,romaji"));
    }

    #[test]
    fn 一行多时间戳展开且元数据跳过() {
        let lines = timed_lrc_lines("[00:10.00][00:20.50]副歌\n[by:tester]\n[00:05.00]前一句\n");
        assert_eq!(
            lines,
            vec![
                (5000, "前一句".to_string()),
                (10000, "副歌".to_string()),
                (20500, "副歌".to_string()),
            ]
        );
    }

    #[test]
    fn 时间戳解析毫秒() {
        assert_eq!(lrc_time_ms("01:30.5"), Some(90500));
        assert_eq!(lrc_time_ms("00:01:30.50"), Some(90500));
        assert_eq!(lrc_time_ms("abc"), None);
    }
}
