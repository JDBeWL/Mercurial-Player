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
        return Err(format!(
            "HTTP error: {status} - {}",
            safe_truncate(&response_text, 200)
        )
        .into());
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

/// 找出第一个字标记组 `(起始ms,时长ms[,标志])`；返回 (左括号位置, 右括号后一位, 起始, 时长)。
///
/// 正文里的半角括号不是分隔符（`Lemon (Live)`、`(啊)` 这类），必须跳过去找下一个数字组，
/// 否则从它开始整行后面的字全丢
fn find_yrc_group(s: &str) -> Option<(usize, usize, i64, i64)> {
    let mut from = 0usize;
    while let Some(rel) = s[from..].find('(') {
        let open = from + rel;
        let Some(close_rel) = s[open..].find(')') else {
            break; // 括号没闭合，剩下的整段都是正文
        };
        let close = open + close_rel;
        let mut fields = s[open + 1..close].split(',');
        if let (Some(a), Some(b)) = (fields.next(), fields.next()) {
            if let (Ok(start_ms), Ok(dur_ms)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                return Some((open, close + 1, start_ms, dur_ms));
            }
        }
        from = close + 1;
    }
    None
}

/// 解析 yrc 行正文 `(字起始ms,字时长ms,标志)字…` -> 逐字单元
fn parse_yrc_words(content: &str) -> Vec<super::ass::Word> {
    let mut out: Vec<super::ass::Word> = Vec::new();
    let mut rest = content;
    while let Some((_, after, start_ms, dur_ms)) = find_yrc_group(rest) {
        let tail = &rest[after..];
        // 字文本取到下一个字标记组之前：中间的空格与非法括号都属于正文
        let text_end = find_yrc_group(tail).map_or(tail.len(), |(next_open, _, _, _)| next_open);
        let text = &tail[..text_end];
        if !text.is_empty() {
            out.push(super::ass::Word {
                start_ms,
                end_ms: start_ms.saturating_add(dur_ms.max(10)),
                text: text.to_string(),
            });
        }
        rest = &tail[text_end..];
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
            end_ms: start_ms.saturating_add(dur_ms).max(last_word_end),
            text: words.iter().map(|w| w.text.as_str()).collect(),
            words,
        });
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort_by_key(|r| r.start_ms);

    let mut out = super::ass::ass_header(title);
    // 译文/罗马音复用原文行的起止，理由见 `ass::match_row_span`
    let spans: Vec<(i64, i64)> = rows.iter().map(|r| (r.start_ms, r.end_ms)).collect();
    for (text, style) in [(trans, "ts"), (roma, "roma")] {
        let lines = timed_lrc_lines(text);
        for (i, (start_ms, plain)) in lines.iter().enumerate() {
            let (s, e) =
                super::ass::match_row_span(&spans, *start_ms, super::ass::MATCH_ROW_TOLERANCE_MS)
                    .unwrap_or_else(|| {
                        let end = lines
                            .get(i + 1)
                            .map_or_else(|| start_ms.saturating_add(5000), |(next, _)| *next)
                            .max(start_ms.saturating_add(10));
                        (*start_ms, end)
                    });
            out.push_str(&super::ass::dialogue(s, e, style, None, plain));
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
        return Err(format!(
            "HTTP error: {status} - {}",
            safe_truncate(&response_text, 200)
        )
        .into());
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
        // 末字的 \kf 要一直铺到行尾（27960-25740=2220ms），否则前端累加出来的时间点
        // 比实际早，行尾那段拖音没有字在唱
        assert!(ass.contains(
            "Dialogue: 0,00:00:24.81,00:00:27.96,orig,,0,0,0,,{\\kf63}这{\\kf30}一{\\kf222}路"
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
        // 起止必须与匹配到的原文行一致：前端按 `start-end` 精确配对样式，
        // 行尾自己算就会让译文变成单独一行
        assert!(ass.contains("Dialogue: 0,00:00:01.00,00:00:03.00,ts,,0,0,0,,中文译文"));
        assert!(ass.contains("Dialogue: 0,00:00:01.00,00:00:03.00,roma,,0,0,0,,romaji"));
    }

    #[test]
    fn 译文匹配不到原文行时退回自身时间轴() {
        let yrc = "[1000,2000](1000,2000,0)あ\n";
        // 25000ms 处没有原文行(容差 300ms)，行尾取下一行起始
        let ass = yrc_to_karaoke_ass(yrc, "[00:25.00]独白\n[00:28.00]下一句", "", "1").unwrap();
        assert!(ass.contains("Dialogue: 0,00:00:25.00,00:00:28.00,ts,,0,0,0,,独白"));
    }

    #[test]
    fn yrc_正文含半角括号不截行() {
        // `(Live)` 不是字标记组：曾经在这里 break，整行后面的字全丢
        let yrc = "[1000,5000](1000,600,0)Lemon (Live) 现场(1600,3400,0)版\n";
        let words = parse_yrc_words("(1000,600,0)Lemon (Live) 现场(1600,3400,0)版");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Lemon (Live) 现场");
        assert_eq!(words[0].start_ms, 1000);
        assert_eq!(words[1].text, "版");
        assert_eq!(words[1].start_ms, 1600);
        let ass = yrc_to_karaoke_ass(yrc, "", "", "1").unwrap();
        assert!(ass.contains("Lemon (Live) 现场"));
        assert!(ass.ends_with("{\\kf60}Lemon (Live) 现场{\\kf440}版\n"));
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
