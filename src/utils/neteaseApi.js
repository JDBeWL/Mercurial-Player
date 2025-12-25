/**
 * 网易云音乐歌词API
 * 通过 Tauri 后端代理请求，避免 CORS 问题
 */

import { invoke } from '@tauri-apps/api/core';
import logger from './logger';
import errorHandler, { ErrorType, ErrorSeverity, handlePromise } from './errorHandler';

/**
 * 网易云音乐API类
 */
export class NeteaseAPI {
  /**
   * 搜索歌曲
   * @param {string} keyword - 搜索关键词
   * @param {number} limit - 返回数量
   * @param {number} offset - 偏移量
   * @returns {Promise<Array>} 搜索结果
   */
  async searchSongs(keyword, limit = 10, offset = 0) {
    const result = await handlePromise(
      invoke('netease_search_songs', {
        keyword,
        limit,
        offset
      }),
      {
        type: ErrorType.NETWORK,
        severity: ErrorSeverity.MEDIUM,
        context: { keyword, limit, offset, action: 'searchSongs' },
        showToUser: false,
        throw: false
      }
    );

    return result.success ? result.data : [];
  }

  /**
   * 获取歌词
   * @param {string} songId - 歌曲ID
   * @returns {Promise<Object>} 歌词数据
   */
  async getLyrics(songId) {
    const result = await handlePromise(
      invoke('netease_get_lyrics', { songId }),
      {
        type: ErrorType.NETWORK,
        severity: ErrorSeverity.MEDIUM,
        context: { songId, action: 'getLyrics' },
        showToUser: false,
        throw: false
      }
    );

    return result.success ? result.data : null;
  }

  /**
   * 搜索并获取最匹配的歌词
   * @param {string} title - 歌曲标题
   * @param {string} artist - 艺术家
   * @param {number} duration - 歌曲时长（毫秒）
   * @returns {Promise<Object|null>} 歌词数据
   */
  async searchAndGetLyrics(title, artist = '', duration = 0) {
    try {
      // 构建搜索关键词
      let keyword = title;
      if (artist) {
        keyword = `${title} ${artist}`;
      }

      // 搜索歌曲
      let songs = await this.searchSongs(keyword, 10);
      
      if (songs.length === 0) {
        // 如果没有结果，尝试只用标题搜索
        songs = await this.searchSongs(title, 10);
        if (songs.length === 0) {
          return null;
        }
      }

      // 找到最匹配的歌曲
      const bestMatch = this.findBestMatch(songs, title, artist, duration);
      
      if (!bestMatch) {
        // 如果没有找到匹配，使用第一个结果
        if (songs.length > 0) {
          return await this.getLyrics(songs[0].id);
        }
        return null;
      }

      // 获取歌词
      return await this.getLyrics(bestMatch.id);
    } catch (error) {
      errorHandler.handle(error, {
        type: ErrorType.NETWORK,
        severity: ErrorSeverity.LOW,
        context: { title, artist, duration, action: 'searchAndGetLyrics' },
        showToUser: false
      });
      return null;
    }
  }

  /**
   * 找到最匹配的歌曲
   */
  findBestMatch(songs, title, artist, duration) {
    if (songs.length === 0) return null;

    let bestScore = -1;
    let bestMatch = null;

    const normalizeStr = (str) => {
      return str.toLowerCase()
        .replace(/[\s\-_\.]/g, '')
        .replace(/[（(][^）)]*[）)]/g, '')
        .trim();
    };

    const normalizedTitle = normalizeStr(title);
    const normalizedArtist = normalizeStr(artist);

    for (const song of songs) {
      let score = 0;

      const songTitle = normalizeStr(song.name);
      if (songTitle === normalizedTitle) {
        score += 100;
      } else if (songTitle.includes(normalizedTitle) || normalizedTitle.includes(songTitle)) {
        score += 50;
      }

      if (artist) {
        const songArtist = normalizeStr(song.artist);
        if (songArtist === normalizedArtist) {
          score += 50;
        } else if (songArtist.includes(normalizedArtist) || normalizedArtist.includes(songArtist)) {
          score += 25;
        }
      }

      if (duration > 0 && song.duration > 0) {
        const durationDiff = Math.abs(duration - song.duration);
        if (durationDiff < 3000) {
          score += 30;
        } else if (durationDiff < 10000) {
          score += 15;
        }
      }

      if (score > bestScore) {
        bestScore = score;
        bestMatch = song;
      }
    }

    return bestScore >= 50 ? bestMatch : null;
  }

  /**
   * 合并原文和翻译歌词
   */
  mergeLyrics(lrc, tlyric) {
    if (!tlyric) return lrc;
    if (!lrc) return '';

    const parseLrc = (text) => {
      const lines = text.split('\n');
      const result = {};
      const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2,3})\]/g;

      for (const line of lines) {
        const matches = [...line.matchAll(timeRegex)];
        if (matches.length === 0) continue;

        const lineText = line.replace(timeRegex, '').trim();
        if (!lineText) continue;

        for (const match of matches) {
          const time = `${match[1]}:${match[2]}.${match[3]}`;
          result[time] = lineText;
        }
      }
      return result;
    };

    const origLyrics = parseLrc(lrc);
    const transLyrics = parseLrc(tlyric);

    const merged = [];
    const times = [...new Set([...Object.keys(origLyrics), ...Object.keys(transLyrics)])].sort();

    for (const time of times) {
      const orig = origLyrics[time] || '';
      const trans = transLyrics[time] || '';
      
      if (orig) {
        merged.push(`[${time}]${orig}`);
        if (trans && trans !== orig) {
          merged.push(`[${time}]${trans}`);
        }
      }
    }

    return merged.join('\n');
  }
}

export const neteaseApi = new NeteaseAPI();
export default neteaseApi;
