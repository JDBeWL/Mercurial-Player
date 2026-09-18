//! 酷狗音乐歌词来源

use crate::error::AppError;
use crate::lyrics::{LyricCandidate, LyricQuery, LyricsData};
use crate::media::http_client::{get, read_response_text, send_with_retry};
use base64::Engine;
use md5::{Digest, Md5};
use serde::Deserialize;
use std::fmt::Write as _;
use std::io::Read;
use tauri_plugin_http::reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};


const KRC_KEY: &[u8; 16] = b"@Gaw^2tGQ61-\xce\xd2ni";
const SIGN_SALT: &str = "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA";
const APP_ID: &str = "3116";
const CLIENT_VER: &str = "11070";

const SEARCH_URL: &str = "https://songsearch.kugou.com/song_search_v2";
const LYRIC_SEARCH_URL: &str = "https://lyrics.kugou.com/v1/search";
const LYRIC_DOWNLOAD_URL: &str = "https://lyrics.kugou.com/download";

/// `contenttype == 2` 表示 base64 的纯文本歌词
const CONTENT_TYPE_PLAIN: i64 = 2;

// ---------------------------------------------------------------- 响应结构

#[derive(Debug, Deserialize)]
struct WebSearchResponse {
    #[serde(default)]
    data: Option<WebSearchData>,
}

#[derive(Debug, Deserialize)]
struct WebSearchData {
    #[serde(default)]
    lists: Vec<WebSearchItem>,
}

#[derive(Debug, Deserialize)]
struct WebSearchItem {
    #[serde(default, rename = "FileHash")]
    file_hash: String,
    /// 即歌词接口的 `album_audio_id`
    #[serde(default, rename = "MixSongID")]
    mix_song_id: String,
    /// 秒
    #[serde(default, rename = "Duration")]
    duration: i64,
    #[serde(default, rename = "SongName")]
    song_name: String,
    #[serde(default, rename = "SingerName")]
    singer_name: String,
    #[serde(default, rename = "AlbumName")]
    album_name: String,
}

#[derive(Debug, Deserialize)]
struct CandidateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default)]
    id: String,
    #[serde(default)]
    accesskey: String,
}

#[derive(Debug, Deserialize)]
struct DownloadResponse {
    /// 2 = base64 纯文本歌词
    #[serde(default)]
    contenttype: i64,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct LanguageBlob {
    #[serde(default)]
    content: Vec<LanguageEntry>,
}

#[derive(Debug, Deserialize)]
struct LanguageEntry {
    #[serde(default, rename = "type")]
    kind: i64,
    #[serde(default, rename = "lyricContent")]
    lyric_content: Vec<Vec<String>>,
}

// 通用小工具

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn md5_hex(text: &str) -> String {
    format!("{:x}", Md5::digest(text.as_bytes()))
}

/// 搜索结果里的高亮标签 `<em>…</em>`
fn strip_em(text: &str) -> String {
    text.replace("<em>", "").replace("</em>", "")
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// 毫秒 → LRC 时间戳 `MM:SS.xx`（四舍五入到厘秒，小时并入分钟）
fn lrc_time(ms: i64) -> String {
    let total_cs = (ms.max(0) + 5) / 10;
    let (m, s, cs) = (total_cs / 6000, total_cs / 100 % 60, total_cs % 100);
    format!("{m:02}:{s:02}.{cs:02}")
}

/// `has_content`：剔除空行、`//` 以及 `X：` 歌手标签行
fn has_content(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "//" {
        return false;
    }
    let mut chars = trimmed.chars();
    !matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(c), Some('：'), None) if c.is_ascii_uppercase()
    )
}

// 请求
fn kg_headers(module: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(ua) = HeaderValue::from_str(&format!("Android14-1070-11070-201-0-{module}-wifi")) {
        headers.insert(USER_AGENT, ua);
    }
    // mid 只作为一次性设备标识，服务端不校验其来源（用当前毫秒时间戳的 md5）
    if let Ok(mid) = HeaderValue::from_str(&md5_hex(&now_ms().to_string())) {
        headers.insert(HeaderName::from_static("mid"), mid);
    }
    headers.insert(
        HeaderName::from_static("kg-rec"),
        HeaderValue::from_static("1"),
    );
    headers.insert(
        HeaderName::from_static("kg-rc"),
        HeaderValue::from_static("1"),
    );
    if let Ok(ts) = HeaderValue::from_str(&now_ms().to_string()) {
        headers.insert(HeaderName::from_static("kg-clienttimems"), ts);
    }
    headers
}

fn kg_signature(params: &[(&str, String)], data: &str) -> String {
    let mut sorted: Vec<&(&str, String)> = params.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let mut buf = String::from(SIGN_SALT);
    for (key, value) in sorted {
        let _ = write!(buf, "{key}={value}");
    }
    buf.push_str(data);
    buf.push_str(SIGN_SALT);
    md5_hex(&buf)
}

/// GET 成功时返回响应文本
async fn kg_get(url: &str, params: &[(&str, String)]) -> Option<String> {
    let mut all: Vec<(&str, String)> = vec![
        ("appid", APP_ID.to_string()),
        ("clientver", CLIENT_VER.to_string()),
    ];
    all.extend(params.iter().map(|(key, value)| (*key, value.clone())));

    let signature = kg_signature(&all, "");
    let mut query: Vec<(&str, &str)> = all.iter().map(|(k, v)| (*k, v.as_str())).collect();
    query.push(("signature", signature.as_str()));

    let request = get(url).ok()?.query(&query).headers(kg_headers("Lyric"));
    let response = send_with_retry(request).await.ok()?;
    let status = response.status();
    let text = read_response_text(response).await.ok()?;
    if !status.is_success() {
        return None;
    }
    Some(text)
}

/// 搜索歌曲
async fn search_songs(query: &LyricQuery, limit: u32) -> Vec<WebSearchItem> {
    let keyword = if query.artist.is_empty() {
        query.title.clone()
    } else {
        format!("{} {}", query.title, query.artist)
    };
    let Ok(request) = get(SEARCH_URL) else {
        return Vec::new();
    };
    let request = request.query(&[
        ("keyword", keyword.as_str()),
        ("page", "1"),
        ("pagesize", &limit.to_string()),
        ("platform", "WebFilter"),
        ("userid", "-1"),
        ("clientver", "2000"),
        ("tag", "em"),
    ]);
    let Ok(response) = send_with_retry(request).await else {
        return Vec::new();
    };
    let status = response.status();
    let text = read_response_text(response).await.unwrap_or_default();
    if !status.is_success() {
        return Vec::new();
    }
    serde_json::from_str::<WebSearchResponse>(&text)
        .ok()
        .and_then(|data| data.data)
        .map(|data| data.lists)
        .unwrap_or_default()
}

/// 歌词候选
async fn lyric_candidates(
    hash: &str,
    album_audio_id: &str,
    keyword: &str,
    duration_ms: i64,
) -> Vec<Candidate> {
    let params = [
        ("album_audio_id", album_audio_id.to_string()),
        ("duration", duration_ms.to_string()),
        ("hash", hash.to_string()),
        ("keyword", keyword.to_string()),
        ("lrctxt", "1".to_string()),
        ("man", "no".to_string()),
    ];
    let Some(text) = kg_get(LYRIC_SEARCH_URL, &params).await else {
        return Vec::new();
    };
    serde_json::from_str::<CandidateResponse>(&text)
        .map(|data| data.candidates)
        .unwrap_or_default()
}

/// 下载歌词载荷
async fn download_lyrics(id: &str, accesskey: &str) -> Option<DownloadResponse> {
    let params = [
        ("accesskey", accesskey.to_string()),
        ("charset", "utf8".to_string()),
        ("client", "mobi".to_string()),
        ("fmt", "krc".to_string()),
        ("id", id.to_string()),
        ("ver", "1".to_string()),
    ];
    let text = kg_get(LYRIC_DOWNLOAD_URL, &params).await?;
    serde_json::from_str::<DownloadResponse>(&text).ok()
}

// 解密
fn krc_decrypt(encrypted: &[u8]) -> Option<String> {
    let body = encrypted.get(4..)?;
    if body.is_empty() {
        return None;
    }
    let plain: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(i, byte)| byte ^ KRC_KEY[i % KRC_KEY.len()])
        .collect();
    let mut decoder = flate2::read::ZlibDecoder::new(&plain[..]);
    let mut text = String::new();
    decoder.read_to_string(&mut text).ok()?;
    Some(text.trim_start_matches('\u{feff}').to_string())
}

// 解析
#[derive(Debug, Clone, PartialEq, Eq)]
struct KrcLine {
    start_ms: i64,
    end_ms: i64,
    words: Vec<super::ass::Word>,
}

impl KrcLine {
    /// 整行文本（拼接逐字）
    fn text(&self) -> String {
        self.words.iter().map(|word| word.text.as_str()).collect()
    }

    /// 行内所有字都无文本
    fn is_blank(&self) -> bool {
        self.words.iter().all(|word| word.text.is_empty())
    }

    /// LRC 时间戳取 `words[0].start`
    fn lrc_begin_ms(&self) -> i64 {
        self.words
            .first()
            .map_or(self.start_ms, |word| word.start_ms)
    }
}

/// 解析结果
#[derive(Debug, Default)]
struct KrcParse {
    tags: Vec<(String, String)>,
    lines: Vec<KrcLine>,
    /// 译文，按 `lines` 下标对齐
    ts: Vec<Option<String>>,
    /// 罗马音，按 `lines` 下标对齐
    roma: Vec<Option<String>>,
}

/// 形如 `[行起始ms,行持续ms]内容`
fn parse_krc_line(line: &str) -> Option<(i64, i64, &str)> {
    let rest = line.strip_prefix('[')?;
    let head_end = rest.find(']')?;
    let (start, duration) = rest[..head_end].split_once(',')?;
    if !is_digits(start) || !is_digits(duration) {
        return None;
    }
    Some((
        start.parse().ok()?,
        duration.parse().ok()?,
        &rest[head_end + 1..],
    ))
}

/// 形如 `[key:value]` 的整行标签（value 内不含 `]`）
fn parse_krc_tag(line: &str) -> Option<(&str, &str)> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    if inner.contains(']') {
        return None;
    }
    let (key, value) = inner.split_once(':')?;
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((key, value))
}

fn find_word_marker(text: &str, from: usize) -> Option<(usize, usize, i64, i64)> {
    let mut cursor = from;
    while let Some(rel) = text.get(cursor..)?.find('<') {
        let lt = cursor + rel;
        if let Some(gt_rel) = text.get(lt..)?.find('>') {
            let gt = lt + gt_rel;
            let mut parts = text[lt + 1..gt].split(',');
            if let (Some(a), Some(b), Some(c), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
                && is_digits(a)
                && is_digits(b)
                && is_digits(c)
                && let (Ok(offset), Ok(duration)) = (a.parse::<i64>(), b.parse::<i64>())
            {
                return Some((lt, gt + 1, offset, duration));
            }
        }
        cursor = lt + 1;
    }
    None
}

/// 一行内容 → 逐字单元（文本取相邻标记之间的部分，保留空字以维持罗马音的下标对齐）
fn parse_krc_words(content: &str, line_start: i64) -> Vec<super::ass::Word> {
    let mut markers: Vec<(usize, usize, i64, i64)> = Vec::new();
    let mut from = 0;
    while let Some(marker) = find_word_marker(content, from) {
        from = marker.1;
        markers.push(marker);
    }
    markers
        .iter()
        .enumerate()
        .map(|(i, (_, marker_end, offset, duration))| {
            let text_end = markers.get(i + 1).map_or(content.len(), |next| next.0);
            super::ass::Word {
                start_ms: line_start + offset,
                end_ms: line_start + offset + duration,
                text: content.get(*marker_end..text_end).unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// `[language:<base64 JSON>]`：`type=1` 译文、`type=0` 罗马音
fn apply_language(blob: &str, parsed: &mut KrcParse) {
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(blob) else {
        return;
    };
    let Ok(blob) = serde_json::from_slice::<LanguageBlob>(&raw) else {
        return;
    };
    let count = parsed.lines.len();
    parsed.ts = vec![None; count];
    parsed.roma = vec![None; count];
    for entry in &blob.content {
        match entry.kind {
            // 译文：按行一一对应
            1 => {
                for (i, row) in entry.lyric_content.iter().enumerate().take(count) {
                    if parsed.ts[i].is_none()
                        && let Some(text) = row.first()
                    {
                        parsed.ts[i] = Some(text.clone());
                    }
                }
            }
            // 罗马音：逐字对齐，下标跳过「整行无文本」的行
            0 => {
                let mut blank_count = 0usize;
                for i in 0..count {
                    if parsed.lines[i].is_blank() {
                        blank_count += 1;
                        continue;
                    }
                    let Some(index) = i.checked_sub(blank_count) else {
                        continue;
                    };
                    let Some(row) = entry.lyric_content.get(index) else {
                        continue;
                    };
                    if parsed.roma[i].is_none() {
                        parsed.roma[i] = Some(
                            parsed.lines[i]
                                .words
                                .iter()
                                .enumerate()
                                .map(|(j, _)| row.get(j).cloned().unwrap_or_default())
                                .collect(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// 解析实际的内容
fn parse_krc(text: &str) -> KrcParse {
    let mut parsed = KrcParse::default();
    let mut language: Option<String> = None;

    for raw in text.lines() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if !line.starts_with('[') {
            continue;
        }
        if let Some((key, value)) = parse_krc_tag(line) {
            if key == "language" {
                language = Some(value.trim().to_string());
            }
            parsed.tags.push((key.to_string(), value.to_string()));
            continue;
        }
        let Some((start_ms, duration_ms, content)) = parse_krc_line(line) else {
            continue;
        };
        let mut words = parse_krc_words(content, start_ms);
        if words.is_empty() {
            // 无逐字标记的行退化为整行单字
            words.push(super::ass::Word {
                start_ms,
                end_ms: start_ms + duration_ms,
                text: content.to_string(),
            });
        }
        parsed.lines.push(KrcLine {
            start_ms,
            end_ms: start_ms + duration_ms,
            words,
        });
    }

    if let Some(blob) = language {
        apply_language(&blob, &mut parsed);
    }
    parsed
}

// 文本产出

impl KrcParse {
    /// 行级 LRC
    fn to_lrc(&self) -> String {
        let mut out = String::new();
        for (key, value) in &self.tags {
            if !value.is_empty()
                && matches!(key.as_str(), "al" | "ar" | "au" | "by" | "offset" | "ti")
            {
                let _ = writeln!(out, "[{key}:{value}]");
            }
        }
        for line in &self.lines {
            let text = line.text();
            if !has_content(&text) {
                continue;
            }
            let _ = writeln!(out, "[{}]{}", lrc_time(line.lrc_begin_ms()), text);
        }
        out.trim_end().to_string()
    }

    /// 译文 LRC（时间戳与原文行对齐）
    fn to_tlyric(&self) -> String {
        self.to_aligned_lrc(&self.ts)
    }

    /// 罗马音 LRC（时间戳与原文行对齐）
    fn to_romalrc(&self) -> String {
        self.to_aligned_lrc(&self.roma)
    }

    fn to_aligned_lrc(&self, side: &[Option<String>]) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            let Some(Some(text)) = side.get(i) else {
                continue;
            };
            if !has_content(text) {
                continue;
            }
            let _ = writeln!(out, "[{}]{}", lrc_time(line.lrc_begin_ms()), text.trim());
        }
        out.trim_end().to_string()
    }

    /// 原文逐字 + 译文/罗马音普通行；没有任何逐字行时返回 `None`
    fn to_ass(&self, title: &str) -> Option<String> {
        if !self.lines.iter().any(|line| line.words.len() > 1) {
            return None;
        }
        let mut out = super::ass::ass_header(title);
        for (side, style) in [(&self.ts, "ts"), (&self.roma, "roma")] {
            for (i, line) in self.lines.iter().enumerate() {
                let Some(Some(text)) = side.get(i) else {
                    continue;
                };
                if !has_content(text) {
                    continue;
                }
                out.push_str(&super::ass::dialogue(
                    line.start_ms,
                    line.end_ms,
                    style,
                    None,
                    text.trim(),
                ));
            }
        }
        for line in &self.lines {
            let text = line.text();
            if !has_content(&text) {
                continue;
            }
            // 单字行由 `dialogue` 内部退化为普通文本
            out.push_str(&super::ass::dialogue(
                line.start_ms,
                line.end_ms,
                "orig",
                Some(&line.words),
                &text,
            ));
        }
        Some(out)
    }
}

// 入口

/// 取一整首的候选 bundle：搜索歌词候选
async fn fetch_bundle(item: &WebSearchItem) -> Option<LyricsData> {
    if item.file_hash.is_empty() || item.mix_song_id.is_empty() {
        return None;
    }
    let title = strip_em(&item.song_name);
    let artist = strip_em(&item.singer_name);
    let keyword = format!("{artist} - {title}");
    let duration_ms = item.duration * 1000;

    let candidates =
        lyric_candidates(&item.file_hash, &item.mix_song_id, &keyword, duration_ms).await;
    let best = candidates
        .iter()
        .find(|candidate| !candidate.id.is_empty() && !candidate.accesskey.is_empty())?;

    let data = download_lyrics(&best.id, &best.accesskey).await?;
    let payload: String = data
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if payload.is_empty() {
        return None;
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&payload)
        .ok()?;

    // contenttype == 2：base64 的纯文本歌词，没有逐字与翻译
    if data.contenttype == CONTENT_TYPE_PLAIN {
        let text = String::from_utf8_lossy(&raw)
            .trim_start_matches('\u{feff}')
            .to_string();
        if !has_content(&text) {
            return None;
        }
        return Some(LyricsData {
            lrc: text,
            tlyric: String::new(),
            romalrc: String::new(),
            karaoke: String::new(),
        });
    }

    let krc = krc_decrypt(&raw)?;
    let parsed = parse_krc(&krc);
    let lrc = parsed.to_lrc();
    if !has_content(&lrc) {
        return None;
    }
    Some(LyricsData {
        lrc,
        tlyric: parsed.to_tlyric(),
        romalrc: parsed.to_romalrc(),
        karaoke: parsed.to_ass(&title).unwrap_or_default(),
    })
}

/// 酷狗音乐入口
pub async fn search_candidates(
    query: &LyricQuery,
    limit: u32,
) -> Result<Vec<LyricCandidate>, AppError> {
    let items = search_songs(query, limit).await;
    let mut out: Vec<LyricCandidate> = Vec::new();
    for item in items {
        let Some(bundle) = fetch_bundle(&item).await else {
            continue;
        };
        out.push(LyricCandidate {
            id: item.file_hash.clone(),
            title: strip_em(&item.song_name),
            artist: strip_em(&item.singer_name),
            album: strip_em(&item.album_name),
            duration_ms: item.duration * 1000,
            bundle,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KRC_SAMPLE: &str = "[ti:Lemon]\n\
        [ar:米津玄師]\n\
        [al:]\n\
        [offset:0]\n\
        [889,1527]<0,551,0>夢<551,368,0>な<919,153,0>ら<1072,456,0>ば\n\
        [2417,3755]<0,352,0>ど<352,392,0>れ\n\
        [1679,1550]作词：米津玄師\n";

    #[test]
    fn 十六字节() {
        const EXPECTED: [u8; 16] = [
            0x40, 0x47, 0x61, 0x77, 0x5e, 0x32, 0x74, 0x47, 0x51, 0x36, 0x31, 0x2d, 0xce, 0xd2,
            0x6e, 0x69,
        ];
        assert_eq!(*KRC_KEY, EXPECTED);
    }

    #[test]
    fn 与服务端一致() {
        let params = [
            ("appid", "3116".to_string()),
            ("clientver", "11070".to_string()),
            ("album_audio_id", "119438544".to_string()),
            ("duration", "255000".to_string()),
            ("hash", "DA60F11F69AFFC7107ADEDE9DD17352A".to_string()),
            ("keyword", "米津玄師 - Lemon".to_string()),
            ("lrctxt", "1".to_string()),
            ("man", "no".to_string()),
        ];
        assert_eq!(
            kg_signature(&params, ""),
            "d0bdac54abab0672c170de1561a4ba68"
        );
    }

    #[test]
    fn krc解密往返() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write as _;

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(KRC_SAMPLE.as_bytes()).unwrap();
        let deflated = encoder.finish().unwrap();
        let mut payload = b"krc1".to_vec();
        payload.extend(
            deflated
                .iter()
                .enumerate()
                .map(|(i, byte)| byte ^ KRC_KEY[i % 16]),
        );
        let plain = krc_decrypt(&payload).expect("应能解密");
        assert_eq!(plain, KRC_SAMPLE);
    }

    #[test]
    fn krc解密拒绝过短或损坏载荷() {
        assert!(krc_decrypt(b"krc1").is_none());
        assert!(krc_decrypt(b"krc1\x00\x01\x02").is_none());
    }

    #[test]
    fn 解析行与逐字() {
        let parsed = parse_krc(KRC_SAMPLE);
        assert_eq!(parsed.lines.len(), 3);
        assert_eq!(parsed.tags.len(), 4); // ti/ar/al/offset

        let first = &parsed.lines[0];
        assert_eq!((first.start_ms, first.end_ms), (889, 889 + 1527));
        assert_eq!(first.text(), "夢ならば");
        assert_eq!(first.words.len(), 4);
        // 字偏移相对行首，绝对化为行内毫秒
        assert_eq!(
            (first.words[1].start_ms, first.words[1].end_ms),
            (889 + 551, 889 + 551 + 368)
        );
        assert_eq!(first.words[3].text, "ば");

        // 无逐字标记的行退化为整行单字
        let third = &parsed.lines[2];
        assert_eq!(third.words.len(), 1);
        assert_eq!(third.words[0].text, "作词：米津玄師");
    }

    #[test]
    fn 解析语言标签的译文() {
        let json = r#"{"content":[{"lyricContent":[["如果能做梦的话"],["那该有多好"],["作词：米津玄師"]],"type":1,"language":0}]}"#;
        let krc = format!(
            "{KRC_SAMPLE}[language:{}]\n",
            base64::engine::general_purpose::STANDARD.encode(json)
        );
        let parsed = parse_krc(&krc);
        assert_eq!(parsed.ts.len(), 3);
        assert_eq!(parsed.ts[0].as_deref(), Some("如果能做梦的话"));
        assert_eq!(parsed.ts[2].as_deref(), Some("作词：米津玄師"));
        assert_eq!(
            parsed.to_tlyric(),
            "[00:00.89]如果能做梦的话\n[00:02.42]那该有多好\n[00:01.68]作词：米津玄師"
        );
        assert!(parsed.roma.iter().all(Option::is_none));
    }

    #[test]
    fn 解析语言标签的罗马音跳过空行() {
        // type=0 的下标跳过「整行无文本」的行
        let json = r#"{"content":[{"lyricContent":[["yu","me"],["to"]],"type":0,"language":0}]}"#;
        let krc = format!(
            "[0,1000]\n[1000,1000]<0,500,0>夢<500,500,0>な\n[2000,1000]<0,900,0>ど\n[language:{}]\n",
            base64::engine::general_purpose::STANDARD.encode(json)
        );
        let parsed = parse_krc(&krc);
        assert_eq!(parsed.lines.len(), 3);
        assert!(parsed.lines[0].is_blank());
        assert_eq!(parsed.roma[0], None);
        assert_eq!(parsed.roma[1].as_deref(), Some("yume"));
        assert_eq!(parsed.roma[2].as_deref(), Some("to"));
    }

    #[test]
    fn 生成行级lrc与逐字ass() {
        let json = r#"{"content":[{"lyricContent":[["如果能做梦的话"],["那该有多好"],["作词：米津玄師"]],"type":1,"language":0}]}"#;
        let krc = format!(
            "{KRC_SAMPLE}[language:{}]\n",
            base64::engine::general_purpose::STANDARD.encode(json)
        );
        let parsed = parse_krc(&krc);

        let lrc = parsed.to_lrc();
        assert!(lrc.starts_with("[ti:Lemon]\n[ar:米津玄師]\n[offset:0]\n"));
        assert!(lrc.contains("[00:00.89]夢ならば"));
        assert!(!lrc.contains("<0,551,0>"));
        // 无内容的标签值为空 → 不输出
        assert!(!lrc.contains("[al:]"));

        let ass = parsed.to_ass("Lemon").expect("应生成逐字 ASS");
        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("Dialogue: 0,00:00:00.89,00:00:02.42,orig,,0,0,0,,{\\kf55}夢"));
        assert!(ass.contains(",ts,,0,0,0,,如果能做梦的话\n"));
        // 单字行不带 \kf
        assert!(ass.contains(",orig,,0,0,0,,作词：米津玄師\n"));
    }

    #[test]
    fn 无逐字时不生成ass() {
        let parsed = parse_krc("[0,1000]只有一行\n[1000,1000]第二行\n");
        assert!(parsed.to_ass("x").is_none());
    }

    #[test]
    fn 时间戳格式化为两位厘秒() {
        assert_eq!(lrc_time(0), "00:00.00");
        assert_eq!(lrc_time(889), "00:00.89");
        assert_eq!(lrc_time(61_234), "01:01.23");
        assert_eq!(lrc_time(3_600_000), "60:00.00"); // 小时并入分钟
        assert_eq!(lrc_time(-5), "00:00.00");
    }

    #[test]
    fn 内容判定剔除空行与歌手标签() {
        assert!(has_content("夢ならば"));
        assert!(!has_content("   "));
        assert!(!has_content("//"));
        assert!(!has_content("J："));
        assert!(has_content("J：x"));
    }

    #[test]
    fn 搜索结果去除高亮标签() {
        assert_eq!(strip_em("<em>Lemon</em>"), "Lemon");
        assert_eq!(strip_em("<em>米津玄師</em>"), "米津玄師");
        assert_eq!(strip_em("STRAY SHEEP"), "STRAY SHEEP");
    }

    #[test]
    fn 行标记识别严格三段数字() {
        // 正文里的括号数字不算字标记
        assert!(find_word_marker("(1,2)", 0).is_none());
        assert!(find_word_marker("<1,2>", 0).is_none());
        assert!(find_word_marker("<1,2,0>", 0).is_some());
        assert!(find_word_marker("a<1,2,3>b", 0).is_some_and(|m| (m.0, m.1) == (1, 8)));
    }
}
