//! 在线歌词的 Tauri 命令

use crate::error::AppError;
use crate::lyrics::{LyricCandidate, LyricsData, netease};
use tauri::command;

/// 搜索网易云音乐歌曲
#[command]
pub async fn netease_search_songs(
    keyword: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<netease::SearchSongResult>, AppError> {
    netease::search_songs(&keyword, limit.unwrap_or(10), offset.unwrap_or(0)).await
}

/// 获取网易云音乐歌词
#[command]
pub async fn netease_get_lyrics(song_id: String) -> Result<LyricsData, AppError> {
    netease::get_lyrics(&song_id).await
}

/// 按来源+方法获取候选歌词（多来源系统统一入口）
#[command]
pub async fn lyrics_search_candidates(
    provider: String,
    method: String,
    title: String,
    artist: String,
    duration: i64,
    limit: Option<u32>,
) -> Result<Vec<LyricCandidate>, AppError> {
    let query = crate::lyrics::LyricQuery {
        title,
        artist,
        duration_ms: duration,
    };
    super::search_candidates(&provider, &method, &query, limit.unwrap_or(5)).await
}
