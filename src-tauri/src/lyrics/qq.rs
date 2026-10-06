//! QQ 音乐歌词来源

use crate::error::AppError;
use crate::http_client::{get, read_response_text, send_with_retry};
use crate::lyrics::{LyricCandidate, LyricQuery, LyricsData};
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

/// 时间戳（mm:ss.xx 或 hh:mm:ss.xx）-> 毫秒
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

/// 找出第一个时间标记 `<mm:ss.xx>`，返回 (左尖括号位置, 右尖括号后一位, 起始毫秒)。
///
/// 正文里的其它 `<...>`（`<b>` 之类）不是分隔符，必须跳过去找下一个，否则从它开始整行后面
/// 的字全丢
fn find_qq_mark(s: &str) -> Option<(usize, usize, i64)> {
    let mut from = 0usize;
    while let Some(rel) = s[from..].find('<') {
        let lt = from + rel;
        let Some(gt_rel) = s[lt..].find('>') else {
            break; // 没闭合，剩下的整段都是正文
        };
        let gt = lt + gt_rel;
        if let Some(t) = qq_time_ms(s[lt + 1..gt].trim()) {
            return Some((lt, gt + 1, t));
        }
        from = gt + 1;
    }
    None
}

/// 解析逐字内容：`<mm:ss.xx>词<mm:ss.xx>词` -> [(字开始毫秒, 字文本)]
fn parse_qq_word_timings(content: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    let mut rest = content;
    while let Some((_, after, t)) = find_qq_mark(rest) {
        let tail = &rest[after..];
        // 字文本取到下一个时间标记之前，中间的非时间尖括号属于正文
        let text_end = find_qq_mark(tail).map_or(tail.len(), |(next_lt, _, _)| next_lt);
        let word = &tail[..text_end];
        if !word.is_empty() {
            out.push((t, word.to_string()));
        }
        rest = &tail[text_end..];
    }
    out
}

/// 解析普通逐行 LRC（trans/roma 用）-> [(start_ms, 文本)]
fn parse_plain_lines(text: &str) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        if rest.is_empty() {
            continue;
        }
        // 行首可以有连续多个时间戳（`[00:10.00][00:20.50]副歌`），要逐个登记；只取第一个
        // 会把后面的时间戳当成正文显示出来，后面的那次出现也整个丢掉
        let mut stamps: Vec<i64> = Vec::new();
        while let Some(close) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
            match qq_time_ms(rest[1..=close].trim()) {
                Some(ms) => {
                    stamps.push(ms);
                    rest = &rest[close + 2..];
                }
                None => break,
            }
        }
        if stamps.is_empty() {
            continue;
        }
        let plain = rest.trim().to_string();
        if plain.is_empty() {
            continue;
        }
        out.extend(stamps.into_iter().map(|ms| (ms, plain.clone())));
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

    // 每行的结束 = 下一行的起始；末行按最后一个字的时间 + 5s。注意 `segs.0` 已经是绝对
    // 毫秒（`qq_time_ms` 的口径），再叠一次行起始会让末行残留约两倍曲长
    let ends: Vec<i64> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            rows.get(i + 1).map_or_else(
                || {
                    let last_start = row.segs.last().map_or(row.start_ms, |seg| seg.0);
                    last_start.max(row.start_ms).saturating_add(5000)
                },
                |next| next.start_ms,
            )
        })
        .collect();

    let mut out = super::ass::ass_header(title);

    // 译文/罗马音普通行（非逐字）：起止复用匹配到的原文行，理由见 `ass::match_row_span`
    let spans: Vec<(i64, i64)> = rows
        .iter()
        .zip(&ends)
        .map(|(row, &end)| (row.start_ms, end))
        .collect();
    for (text, style) in [(trans, "ts"), (roma, "roma")] {
        if text.trim().is_empty() {
            continue;
        }
        let lines = parse_plain_lines(text);
        for (i, (start_ms, plain)) in lines.iter().enumerate() {
            let (s, e) =
                super::ass::match_row_span(&spans, *start_ms, super::ass::MATCH_ROW_TOLERANCE_MS)
                    .unwrap_or_else(|| {
                        let end = if i + 1 < lines.len() {
                            lines[i + 1].0
                        } else {
                            start_ms.saturating_add(5000)
                        };
                        (*start_ms, end.max(start_ms.saturating_add(10)))
                    });
            out.push_str(&super::ass::dialogue(s, e, style, None, plain));
        }
    }

    // 原文逐字行
    for (row, &end_ms) in rows.iter().zip(&ends) {
        // 字结束时间由相邻字补出
        let words = super::ass::words_from_starts(&row.segs, end_ms);
        // 单字标记的行会被 `dialogue` 退化成普通文本行，plain 必须给出正文而不是空串
        let plain: String = row.segs.iter().map(|(_, text)| text.as_str()).collect();
        out.push_str(&super::ass::dialogue(
            row.start_ms,
            end_ms,
            "orig",
            Some(&words),
            &plain,
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
    // 逐字接口对多数歌会失败、紧接着还要再请求一次纯文本歌词，串行就是 2N 次往返；
    // 并行后最坏只是一次超时。顺序仍按搜索结果原序
    let fetched = futures_util::future::join_all(
        songs
            .into_iter()
            .filter(|song| !song.songmid.is_empty())
            .map(|song| async move {
                // 优先取逐字；取不到再退回纯文本歌词接口
                let bundle = match get_qrc_bundle(&song.songmid, song.songid).await {
                    Some(bundle) => bundle,
                    None => get_lyrics(&song.songmid).await?,
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
                Some(LyricCandidate {
                    id: song.songmid,
                    title,
                    artist,
                    album,
                    duration_ms: song.interval * 1000,
                    bundle,
                })
            }),
    )
    .await;
    Ok(fetched.into_iter().flatten().collect())
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

    #[test]
    fn 逐字标记里的非时间尖括号不截行() {
        // `<b>` 不是时间标记：曾经在这里 break，整行后面的字全丢
        assert_eq!(
            parse_qq_word_timings("<00:01.00>La<b>La<00:02.00>Ba"),
            vec![(1000, "La<b>La".to_string()), (2000, "Ba".to_string())]
        );
    }

    #[test]
    fn 译文一行多时间戳要全部展开() {
        assert_eq!(
            parse_plain_lines("[00:10.00][00:20.50]副歌\n[by:tester]\n[00:05.00]前一句\n"),
            vec![
                (5000, "前一句".to_string()),
                (10000, "副歌".to_string()),
                (20500, "副歌".to_string()),
            ]
        );
    }

    #[test]
    fn 末行行尾按最后一个字() {
        // segs 的时间是绝对毫秒：末行行尾 = 最后一个字 + 5s，不能再叠一次行起始
        let ass = qq_to_karaoke_ass("[00:60.00]<01:00.00>词", "", "", "x").expect("应生成 ASS");
        assert!(ass.contains("Dialogue: 0,00:01:00.00,00:01:05.00,orig,,0,0,0,,词"));
    }
}
