//! 在线歌词 Tauri 命令
//!
//! 网易云搜索/取词，以及多来源候选搜索的统一入口。

use crate::error::AppError;
use crate::lyrics::{LyricCandidate, LyricsData, netease};
use tauri::command;

#[command]
pub async fn netease_search_songs(
    keyword: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<netease::SearchSongResult>, AppError> {
    netease::search_songs(&keyword, limit.unwrap_or(10), offset.unwrap_or(0)).await
}

#[command]
pub async fn netease_get_lyrics(song_id: String) -> Result<LyricsData, AppError> {
    netease::get_lyrics(&song_id).await
}

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
