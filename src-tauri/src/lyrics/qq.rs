//! QQ 音乐歌词来源

use crate::error::AppError;
use crate::lyrics::{LyricCandidate, LyricQuery, LyricsData};
use crate::media::http_client::{get, read_response_text, send_with_retry};
use serde::Deserialize;
use tauri_plugin_http::reqwest::header::{HeaderValue, REFERER, USER_AGENT};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Option<SearchData>,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    #[serde(default)]
    song: Option<SearchSongWrap>,
}

#[derive(Debug, Deserialize)]
struct SearchSongWrap {
    #[serde(default)]
    list: Vec<SearchSong>,
}

#[derive(Debug, Deserialize)]
struct SearchSong {
    #[serde(default)]
    songmid: String,
    /// 搜索结果里的曲名是平铺字段 `songname`（不是 `name`）
    #[serde(default)]
    songname: String,
    #[serde(default)]
    singer: Vec<Singer>,
    /// 专辑名同样是平铺字段 `albumname`（不是嵌套的 `album.name`）
    #[serde(default)]
    albumname: String,
    /// 时长(秒)
    #[serde(default)]
    interval: i64,
    /// 数字歌曲 ID，QRC 逐字歌词接口按它取数据
    #[serde(default)]
    songid: i64,
}

#[derive(Debug, Deserialize)]
struct Singer {
    #[serde(default, rename = "name")]
    name: String,
}

#[derive(Debug, Deserialize)]
struct LyricResponse {
    #[serde(default)]
    retcode: i64,
    #[serde(default)]
    lyric: String,
    #[serde(default)]
    trans: String,
    #[serde(default)]
    roman: String,
}

fn build_headers() -> tauri_plugin_http::reqwest::header::HeaderMap {
    let mut h = tauri_plugin_http::reqwest::header::HeaderMap::new();
    h.insert(USER_AGENT, HeaderValue::from_static(UA));
    h.insert(REFERER, HeaderValue::from_static("https://y.qq.com/"));
    h
}

/// 搜索歌曲，返回前 limit 个（含 songmid）
async fn search_songs(query: &LyricQuery, limit: u32) -> Result<Vec<SearchSong>, AppError> {
    let keyword = if query.artist.is_empty() {
        query.title.clone()
    } else {
        format!("{} {}", query.title, query.artist)
    };
    let url = "https://c.y.qq.com/soso/fcgi-bin/client_search_cp";
    let req = get(url)?.query(&[
        ("p", "1"),
        ("n", &limit.to_string()),
        ("w", keyword.as_str()),
        ("format", "json"),
    ]);
    let req = req.headers(build_headers());
    let resp = send_with_retry(req).await?;
    let status = resp.status();
    let text = read_response_text(resp).await?;
    if !status.is_success() {
        return Ok(Vec::new());
    }
    let data: SearchResponse = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(data
        .data
        .and_then(|d| d.song)
        .map(|s| s.list)
        .unwrap_or_default())
}

/// 时间戳（mm:ss.xx 或 hh:mm:ss.xx）→ 毫秒
fn qq_time_ms(tok: &str) -> Option<i64> {
    let parts: Vec<&str> = tok.split(':').collect();
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

/// 去掉 `<...>` 逐字标记，保留普通文字
fn strip_qq_word_times(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(lt) = rest.find('<') {
        if let Some(gt_rel) = rest[lt..].find('>') {
            let gt = lt + gt_rel;
            out.push_str(&rest[..lt]);
            rest = &rest[gt + 1..];
        } else {
            out.push_str(rest);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// 解析逐字内容：`<mm:ss.xx>词<mm:ss.xx>词` → [(字开始毫秒, 字文本)]
fn parse_qq_word_timings(content: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    let mut pos = 0usize;
    while pos < content.len() {
        let Some(lt_rel) = content[pos..].find('<') else {
            break;
        };
        let lt = pos + lt_rel;
        let Some(gt_rel) = content[lt..].find('>') else {
            break;
        };
        let gt = lt + gt_rel;
        let Some(t) = qq_time_ms(content[lt + 1..gt].trim()) else {
            break;
        };
        let word_end = content[gt + 1..]
            .find('<')
            .map(|i| gt + 1 + i)
            .unwrap_or(content.len());
        let word = content[gt + 1..word_end].to_string();
        if !word.is_empty() {
            out.push((t, word));
        }
        pos = word_end;
    }
    out
}

/// 解析普通逐行 LRC（trans/roma 用）→ [(start_ms, 文本)]
fn parse_plain_lines(text: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(idx) = rest.find(']') else { continue };
        let Some(start_ms) = qq_time_ms(&rest[..idx]) else {
            continue;
        };
        let plain = rest[idx + 1..].trim().to_string();
        if plain.is_empty() {
            continue;
        }
        out.push((start_ms, plain));
    }
    out.sort_by_key(|(t, _)| *t);
    out
}

/// 原文 orig 逐字 + ts/roma 普通行
fn qq_to_karaoke_ass(lyric: &str, trans: &str, roma: &str, title: &str) -> Option<String> {
    struct Row {
        start_ms: i64,
        segs: Vec<(i64, String)>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for raw in lyric.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(idx) = rest.find(']') else { continue };
        let Some(start_ms) = qq_time_ms(&rest[..idx]) else {
            continue;
        };
        let segs = parse_qq_word_timings(&rest[idx + 1..]);
        if segs.is_empty() {
            continue;
        }
        rows.push(Row { start_ms, segs });
    }
    if rows.is_empty() {
        return None;
    }
    rows.sort_by_key(|r| r.start_ms);

    let mut out = super::ass::ass_header(title);

    // 译文/罗马音普通行（非逐字）
    for (text, style) in [(trans, "ts"), (roma, "roma")] {
        if text.trim().is_empty() {
            continue;
        }
        let lines = parse_plain_lines(text);
        for (i, (start_ms, plain)) in lines.iter().enumerate() {
            let end_ms = if i + 1 < lines.len() {
                lines[i + 1].0
            } else {
                start_ms + 5000
            };
            out.push_str(&super::ass::dialogue(*start_ms, end_ms, style, None, plain));
        }
    }

    // 原文逐字行
    for (i, row) in rows.iter().enumerate() {
        let end_ms = if i + 1 < rows.len() {
            rows[i + 1].start_ms
        } else {
            row.start_ms + row.segs.last()?.0 + 5000
        };
        // 字结束时间由相邻字补出
        let words = super::ass::words_from_starts(&row.segs, end_ms);
        out.push_str(&super::ass::dialogue(
            row.start_ms,
            end_ms,
            "orig",
            Some(&words),
            "",
        ));
    }
    Some(out)
}

/// 取某一首歌的歌词
async fn get_lyrics(songmid: &str) -> Option<LyricsData> {
    let url = "https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg";
    let req = get(url)
        .ok()?
        .query(&[("format", "json"), ("songmid", songmid), ("nobase64", "1")])
        .headers(build_headers());
    let resp = send_with_retry(req).await.map_err(|_| ()).ok()?;
    let status = resp.status();
    let text = read_response_text(resp).await.map_err(|_| ()).ok()?;
    if !status.is_success() {
        return None;
    }
    let data: LyricResponse = serde_json::from_str(&text).map_err(|_| ()).ok()?;
    if data.retcode != 0 || data.lyric.trim().is_empty() {
        return None;
    }
    // 歌词可能带 `<mm:ss.xx>` 逐字标记：生成 ASS 逐字，同时产出干净 LRC
    let clean_lrc: String = data
        .lyric
        .lines()
        .map(strip_qq_word_times)
        .collect::<Vec<_>>()
        .join("\n");
    let karaoke = qq_to_karaoke_ass(&data.lyric, &data.trans, &data.roman, songmid);
    Some(LyricsData {
        lrc: clean_lrc,
        tlyric: data.trans,
        romalrc: data.roman,
        karaoke: karaoke.unwrap_or_default(),
    })
}

/// 取某一首歌的逐字歌词
#[allow(clippy::similar_names)]
async fn get_qrc_bundle(songmid: &str, songid: i64) -> Option<LyricsData> {
    let url = "https://c.y.qq.com/qqmusic/fcgi-bin/lyric_download.fcg";
    let req = get(url)
        .ok()?
        .query(&[
            ("version", "15"),
            ("miniversion", "82"),
            ("lrctype", "4"),
            ("musicid", &songid.to_string()),
            ("songmid", songmid),
        ])
        .headers(build_headers());
    let resp = send_with_retry(req).await.map_err(|_| ()).ok()?;
    let status = resp.status();
    let text = read_response_text(resp).await.map_err(|_| ()).ok()?;
    if !status.is_success() {
        return None;
    }

    let qrc = super::qrc::extract(&text)?;

    // 复用 qq.rs 既有逻辑产出干净 LRC 与 ASS 逐字
    let clean_lrc: String = qrc
        .lyric
        .lines()
        .map(strip_qq_word_times)
        .collect::<Vec<_>>()
        .join("\n");
    let romalrc: String = qrc
        .roma
        .lines()
        .map(strip_qq_word_times)
        .collect::<Vec<_>>()
        .join("\n");
    if clean_lrc.trim().is_empty() {
        return None;
    }
    let karaoke = qq_to_karaoke_ass(&qrc.lyric, &qrc.trans, &romalrc, songmid).unwrap_or_default();

    Some(LyricsData {
        lrc: clean_lrc,
        tlyric: qrc.trans,
        romalrc,
        karaoke,
    })
}

/// 读取 LRC 头部元信息（`[ti:]` / `[ar:]` / `[al:]`），搜索结果缺字段时用它兜底
fn lrc_meta(lrc: &str, key: &str) -> String {
    let prefix = format!("[{key}:");
    for line in lrc.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            if let Some(end) = rest.find(']') {
                return rest[..end].trim().to_string();
            }
        }
    }
    String::new()
}

/// QQ 音乐入口
pub async fn search_candidates(
    query: &LyricQuery,
    limit: u32,
) -> Result<Vec<LyricCandidate>, AppError> {
    let songs = search_songs(query, limit).await?;
    let mut out: Vec<LyricCandidate> = Vec::new();
    for song in songs {
        if song.songmid.is_empty() {
            continue;
        }
        // 优先取逐字；取不到再退回纯文本歌词接口
        let bundle = match get_qrc_bundle(&song.songmid, song.songid).await {
            Some(bundle) => bundle,
            None => match get_lyrics(&song.songmid).await {
                Some(bundle) => bundle,
                None => continue,
            },
        };
        let mut artist = song
            .singer
            .iter()
            .map(|s| s.name.as_str())
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        let mut title = song.songname.clone();
        let mut album = song.albumname.clone();
        // 搜索结果缺字段时用歌词头部兜底：否则候选卡片只剩一行
        if title.is_empty() {
            title = lrc_meta(&bundle.lrc, "ti");
        }
        if artist.is_empty() {
            artist = lrc_meta(&bundle.lrc, "ar");
        }
        if album.is_empty() {
            album = lrc_meta(&bundle.lrc, "al");
        }
        out.push(LyricCandidate {
            id: song.songmid,
            title,
            artist,
            album,
            duration_ms: song.interval * 1000,
            bundle,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 搜索响应的字段形状
    const SEARCH_SAMPLE: &str = r#"{"code":0,"data":{"song":{"list":[{"songmid":"0039MnYb0qxYhV","songname":"晴天","interval":269,"albumname":"叶惠美","singer":[{"id":4558,"mid":"0025NhlN2yWrP4","name":"周杰伦"}]}]}}}"#;

    #[test]
    fn 解析搜索结果平铺字段() {
        let parsed: SearchResponse = serde_json::from_str(SEARCH_SAMPLE).expect("应能解析");
        let list = parsed
            .data
            .and_then(|d| d.song)
            .map(|s| s.list)
            .unwrap_or_default();
        assert_eq!(list.len(), 1);

        let song = &list[0];
        assert_eq!(song.songmid, "0039MnYb0qxYhV");
        // 这两项早期读的是 name / album.name，会恒为空
        assert_eq!(song.songname, "晴天");
        assert_eq!(song.albumname, "叶惠美");
        assert_eq!(song.interval, 269);
        assert_eq!(song.singer.len(), 1);
        assert_eq!(song.singer[0].name, "周杰伦");
    }

    #[test]
    fn 歌词头部元信息兜底() {
        let lrc = "[ti:Lemon (《Unnatural》日剧主题曲)]\n[ar:米津玄師 (よねづ けんし)]\n[al:Lemon (柠檬)]\n[by:]\n[00:00.00]Lemon\n";
        assert_eq!(lrc_meta(lrc, "ti"), "Lemon (《Unnatural》日剧主题曲)");
        assert_eq!(lrc_meta(lrc, "ar"), "米津玄師 (よねづ けんし)");
        assert_eq!(lrc_meta(lrc, "al"), "Lemon (柠檬)");
        // 空值与非元信息行都返回空串，调用方据此判断"没有兜底信息"
        assert_eq!(lrc_meta(lrc, "by"), "");
        assert_eq!(lrc_meta(lrc, "xx"), "");
    }

    #[test]
    fn 逐字标记清理与时间解析() {
        assert_eq!(strip_qq_word_times("<00:01.00>你<00:01.50>好"), "你好");
        assert_eq!(strip_qq_word_times("无标记"), "无标记");
        assert_eq!(qq_time_ms("01:02.50"), Some(62500));
        assert_eq!(qq_time_ms("1:02:03.000"), Some(3_723_000));
        assert_eq!(qq_time_ms("abc"), None);
    }
}
