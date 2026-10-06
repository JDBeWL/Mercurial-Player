//! 多来源歌词获取：netease / lrclib / qq / kugou 各 provider 与共用的返回类型。

pub mod ass;
pub mod commands;
pub mod kugou;
pub mod lrclib;
pub mod netease;
pub mod qq;
pub mod qrc;

use crate::error::AppError;
use serde::Serialize;

/// 歌词 bundle 统一复用返回类型（原文/翻译/罗马音三字段）
///
/// 定义在 lyrics 而非某个 provider：四个来源与前端契约都共用这一形状。
#[derive(Debug, Clone, Serialize)]
pub struct LyricsData {
    pub lrc: String,
    pub tlyric: String,
    pub romalrc: String,
    /// ASS 逐字歌词（原文逐字 + 译文/罗马音普通行），空表示无逐字
    #[serde(skip_serializing_if = "String::is_empty")]
    pub karaoke: String,
}

/// 歌词搜索查询参数
#[derive(Debug, Clone)]
pub struct LyricQuery {
    pub title: String,
    pub artist: String,
    /// 时长(毫秒)，用于候选命中打分；可为 0 表示未知
    pub duration_ms: i64,
}

/// 一个候选歌词（内嵌 bundle，供前端即时预览）
#[derive(Debug, Clone, Serialize)]
pub struct LyricCandidate {
    /// 平台内标识(id/hash/songmid 等)
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub bundle: LyricsData,
}

/// 已知的歌词来源 id
const KNOWN_PROVIDERS: &[&str] = &["netease", "lrclib", "qq", "kugou"];

/// 第三方载荷解压后的字节数上限。网络侧只限到 `http_client::MAX_RESPONSE_SIZE`(5MB)，
/// 而 zlib 对构造数据可以膨胀上千倍，不设上限就是"一个坏响应换一次 OOM"；
/// 真实歌词解压后是几十 KB 量级，4MB 已远超正常范围
pub(crate) const MAX_DECOMPRESSED_BYTES: usize = 4 * 1024 * 1024;

/// 校验 provider 标识（防注入，仅放行已登记来源）
pub fn is_valid_provider(provider: &str) -> bool {
    KNOWN_PROVIDERS.contains(&provider)
}

/// 按 provider + method 分发获取候选歌词；best-effort，单来源失败只返回空列表
pub async fn search_candidates(
    provider: &str,
    _method: &str,
    query: &LyricQuery,
    limit: u32,
) -> Result<Vec<LyricCandidate>, AppError> {
    if !is_valid_provider(provider) {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 5);
    match provider {
        "netease" => netease_candidates(query, limit).await,
        "lrclib" => lrclib::search_candidates(_method, query, limit).await,
        "qq" => qq::search_candidates(query, limit).await,
        "kugou" => kugou::search_candidates(query, limit).await,
        _ => Ok(Vec::new()),
    }
}

/// 搜索 -> 对前 N 个取歌词
async fn netease_candidates(
    query: &LyricQuery,
    limit: u32,
) -> Result<Vec<LyricCandidate>, AppError> {
    let keyword = if query.artist.is_empty() {
        query.title.clone()
    } else {
        format!("{} {}", query.title, query.artist)
    };
    let songs = netease::search_songs(&keyword, limit, 0).await?;
    // 候选逐字请求并行发出：串行时一个卡住的接口就能把整轮拖到 timeout×N（前端表现为
    // "加载歌词中"转几十秒）。join_all 保持搜索结果的原顺序
    let fetched = futures_util::future::join_all(songs.into_iter().map(|song| async move {
        let Ok(bundle) = netease::get_lyrics(&song.id).await else {
            log::debug!("网易歌词候选 {} 获取失败，跳过", song.id);
            return None;
        };
        if bundle.lrc.trim().is_empty() {
            return None;
        }
        Some(LyricCandidate {
            id: song.id,
            title: song.name,
            artist: song.artist,
            album: song.album,
            duration_ms: song.duration,
            bundle,
        })
    }))
    .await;
    Ok(fetched.into_iter().flatten().collect())
}
