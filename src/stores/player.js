import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import FileUtils from '../utils/fileUtils';
import LyricsParser from '../utils/lyricsParser';
import logger from '../utils/logger';
import { ErrorType, ErrorSeverity, handlePromise } from '../utils/errorHandler';
import { useConfigStore } from './config';

/**
 * 简单的 LRU 缓存实现
 */
class LRUCache {
  constructor(maxSize = 100, ttl = 60000) {
    this.maxSize = maxSize;
    this.ttl = ttl; // 过期时间（毫秒）
    this.cache = new Map();
  }

  get(key) {
    const item = this.cache.get(key);
    if (!item) return null;
    
    // 检查是否过期
    if (Date.now() - item.timestamp > this.ttl) {
      this.cache.delete(key);
      return null;
    }
    
    // 移到末尾（最近使用）
    this.cache.delete(key);
    this.cache.set(key, item);
    return item.value;
  }

  set(key, value) {
    // 如果已存在，先删除
    if (this.cache.has(key)) {
      this.cache.delete(key);
    }
    
    // 如果超过最大大小，删除最旧的
    while (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value;
      this.cache.delete(firstKey);
    }
    
    this.cache.set(key, {
      value,
      timestamp: Date.now()
    });
  }

  has(key) {
    return this.get(key) !== null;
  }

  delete(key) {
    this.cache.delete(key);
  }

  clear() {
    this.cache.clear();
  }

  get size() {
    return this.cache.size;
  }
}

export const usePlayerStore = defineStore('player', {
  state: () => ({
    // 当前播放状态
    currentTrack: null,
    playlist: [],
    isPlaying: false,
    currentTime: 0,
    duration: 0,
    volume: 1,

    // 重复模式设置
    repeatMode: 'none', // 'none', 'track', 'list'
    isShuffle: false,

    // 歌词
    lyrics: null,
    currentLyricIndex: -1,
    lyricsOffset: 0,

    // 音频信息
    audioInfo: {
      bitrate: null,
      sampleRate: null,
      channels: null,
      bitDepth: null,
      format: null
    },

    // 加载状态
    _isLoading: false,
    _statusPollId: null,
    lastTrackIndex: -1,

    // 缓存 - 使用 LRU 缓存，限制大小
    _fileExistsCache: null, // 延迟初始化
    _metadataCache: null,   // 延迟初始化
    
    // 清理定时器
    _cleanupTimerId: null,
    
    // 销毁标志，防止清理后继续调用后端
    _isDestroyed: false,
    
    // 播放结束事件监听器
    _trackEndedUnlisten: null,
    
    // 播放位置事件监听器
    _positionUnlisten: null,
  }),

  getters: {
    currentTrackIndex: (state) => {
      if (!state.currentTrack || state.playlist.length === 0) return -1;
      return state.playlist.findIndex(track => track.path === state.currentTrack.path);
    },
    hasNextTrack: (state) => {
      if (!state.currentTrack) return false;
      return state.playlist.length > 0;
    },
    hasPreviousTrack: (state) => {
      if (!state.currentTrack) return false;
      return state.playlist.length > 0;
    },
    currentLyric: (state) => {
      if (!state.lyrics || state.currentLyricIndex < 0 || state.currentLyricIndex >= state.lyrics.length) {
        return null;
      }
      return state.lyrics[state.currentLyricIndex];
    }
  },

  actions: {
    // --- 缓存管理 ---
    
    /**
     * 获取文件存在性缓存（延迟初始化）
     */
    _getFileExistsCache() {
      if (!this._fileExistsCache) {
        this._fileExistsCache = new LRUCache(200, 30000); // 最多200条，30秒过期
      }
      return this._fileExistsCache;
    },

    /**
     * 获取元数据缓存（延迟初始化）
     */
    _getMetadataCache() {
      if (!this._metadataCache) {
        this._metadataCache = new LRUCache(500, 300000); // 最多500条，5分钟过期
      }
      return this._metadataCache;
    },

    /**
     * 启动定期清理任务
     */
    _startCleanupTask() {
      if (this._cleanupTimerId) return;
      
      // 每5分钟清理一次过期缓存
      this._cleanupTimerId = setInterval(() => {
        this._cleanupCaches();
      }, 300000);
    },

    /**
     * 停止清理任务
     */
    _stopCleanupTask() {
      if (this._cleanupTimerId) {
        clearInterval(this._cleanupTimerId);
        this._cleanupTimerId = null;
      }
    },

    /**
     * 清理过期缓存 - 使用异步分块处理避免阻塞
     */
    async _cleanupCaches() {
      // LRU 缓存会在 get 时自动清理过期项
      // 使用分块处理避免大缓存时阻塞主线程
      const CHUNK_SIZE = 50;
      
      if (this._fileExistsCache) {
        const keys = Array.from(this._fileExistsCache.cache.keys());
        for (let i = 0; i < keys.length; i++) {
          this._fileExistsCache.get(keys[i]);
          if (i > 0 && i % CHUNK_SIZE === 0) {
            await new Promise(resolve => setTimeout(resolve, 0));
          }
        }
      }
      if (this._metadataCache) {
        const keys = Array.from(this._metadataCache.cache.keys());
        for (let i = 0; i < keys.length; i++) {
          this._metadataCache.get(keys[i]);
          if (i > 0 && i % CHUNK_SIZE === 0) {
            await new Promise(resolve => setTimeout(resolve, 0));
          }
        }
      }
      logger.debug('Cache cleanup completed');
    },

    // --- 初始化 ---
    async initAudio() {
      try {
        const configStore = useConfigStore();
        const savedVolume = configStore.audio.volume;
        if (typeof savedVolume === 'number' && savedVolume >= 0 && savedVolume <= 1) {
          this.volume = savedVolume;
          await invoke('set_volume', { volume: savedVolume });
        }
      } catch (err) {
        logger.error('Failed to load volume from config:', err);
      }
      
      // 监听播放结束事件（减少 IPC 轮询）
      this._setupTrackEndedListener();
      
      // 监听播放位置事件（从后端获取真实播放时间）
      this._setupPositionListener();
      
      // 启动定期清理任务
      this._startCleanupTask();
      
      logger.info('Player store initialized.');
    },
    
    /**
     * 设置播放结束事件监听
     */
    async _setupTrackEndedListener() {
      try {
        this._trackEndedUnlisten = await listen('track-ended', () => {
          if (this._isDestroyed) return;
          logger.debug('Received track-ended event');
          this._onEnded();
        });
      } catch (err) {
        logger.error('Failed to setup track-ended listener:', err);
      }
    },
    
    /**
     * 设置播放位置事件监听（从后端获取真实播放时间）
     */
    async _setupPositionListener() {
      try {
        this._positionUnlisten = await listen('playback-position', (event) => {
          if (this._isDestroyed || !this.isPlaying) return;
          const position = event.payload?.position;
          if (typeof position === 'number' && position >= 0) {
            this.currentTime = position;
          }
        });
      } catch (err) {
        logger.error('Failed to setup position listener:', err);
      }
    },

    // --- 核心行为 ---

    play() {
      if (this.currentTrack) {
        this.playTrack(this.currentTrack);
      } else if (this.playlist.length > 0) {
        this.playTrack(this.playlist[0]);
      }
    },

    async playTrack(track) {
      // 检查是否已销毁
      if (this._isDestroyed) return;
      
      if (!track || this._isLoading) return;

      let trackExists = await this._checkFileExists(track.path);
      if (!trackExists && track.path) {
        const altPath = track.path.includes('/') ? track.path.replace(/\//g, '\\') : track.path.replace(/\\/g, '/');
        if (altPath !== track.path) {
          track.path = altPath;
          trackExists = await this._checkFileExists(altPath);
        }
      }

      if (!trackExists) {
        logger.warn('Track file not found:', track.path);
        const currentTrackIndex = this.playlist.findIndex(t => t.path === track.path);
        if (this.playlist.length > 1 && currentTrackIndex < this.playlist.length - 1) {
          return this.nextTrack();
        } else {
          await this.resetPlayerState();
          return;
        }
      }

      this._isLoading = true;
      this.stopStatusPolling();

      // 获取元数据
      const metadataCache = this._getMetadataCache();
      let metadata = metadataCache.get(track.path);
      if (!metadata) {
        metadata = {
          title: track.title || FileUtils.getFileName(track.path),
          artist: track.artist || '',
          album: track.album || '',
          duration: track.duration || 0,
          bitrate: track.bitrate || null,
          sampleRate: track.sampleRate || null,
          channels: track.channels || null,
          bitDepth: track.bitDepth || null,
          format: track.format || null
        };
      }

      this.lastTrackIndex = this.currentTrackIndex;
      this.currentTrack = {
        ...track,
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        duration: metadata.duration
      };
      this.duration = metadata.duration || 0;
      this.currentTime = 0;
      this.lyrics = null;
      this.currentLyricIndex = -1;
      this.audioInfo = {
        bitrate: metadata.bitrate || null,
        sampleRate: metadata.sampleRate || null,
        channels: metadata.channels || null,
        bitDepth: metadata.bitDepth || null,
        format: metadata.format || null,
      };

      invoke('pause_track').catch(err => logger.debug("pause before play:", err));

      try {
        logger.info('Playing track:', track.path);
        
        const playPromise = invoke('play_track', { path: track.path });
        const timeoutPromise = new Promise((_, reject) => {
          setTimeout(() => reject(new Error('播放超时')), 5000);
        });

        const result = await handlePromise(
          Promise.race([playPromise, timeoutPromise]),
          {
            type: ErrorType.AUDIO_PLAYBACK_ERROR,
            severity: ErrorSeverity.HIGH,
            context: { trackPath: track.path, trackName: track.name },
            showToUser: true
          }
        );

        if (!result.success) {
          logger.error('Failed to play track:', result.error);
          this.isPlaying = false;
          this._isLoading = false;
          
          const currentIdx = this.playlist.findIndex(t => t.path === track.path);
          if (this.playlist.length > 1 && currentIdx < this.playlist.length - 1) {
            setTimeout(() => this.nextTrack(), 100);
          }
          return;
        }

        this.isPlaying = true;
        this.startStatusPolling();
        
        this.loadLyrics(track.path).catch(err => {
          logger.debug('Lyrics load error:', err);
        });
      } catch (error) {
        logger.error('Playback error:', error);
        this.isPlaying = false;
        
        const currentIdx = this.playlist.findIndex(t => t.path === track.path);
        if (this.playlist.length > 1 && currentIdx < this.playlist.length - 1) {
          setTimeout(() => this.nextTrack(), 100);
        }
      } finally {
        this._isLoading = false;
      }
    },

    pause() {
      if (!this.isPlaying) return;
      invoke('pause_track')
        .then(() => {
          this.isPlaying = false;
          this.stopStatusPolling();
        })
        .catch(err => logger.error("Failed to pause:", err));
    },

    resume() {
      if (this.isPlaying || !this.currentTrack) return;
      invoke('resume_track')
        .then(() => {
          this.isPlaying = true;
          this.startStatusPolling();
        })
        .catch(err => logger.error("Failed to resume:", err));
    },

    togglePlay() {
      if (this.isPlaying) {
        this.pause();
      } else if (this.currentTrack) {
        const isAtEnd = this.duration > 0 && this.currentTime >= this.duration - 0.5;
        if (isAtEnd) {
          this.play();
        } else {
          this.resume();
        }
      }
    },


    // --- 进度控制 ---

    startStatusPolling() {
      this.stopStatusPolling();
      
      // 播放位置现在由后端事件推送，轮询只用于检测播放结束
      const checkInterval = 500;
      
      const poll = async () => {
        // 检查是否已销毁
        if (this._isDestroyed) return;
        
        if (!this.isPlaying) {
          this.stopStatusPolling();
          return;
        }

        try {
          const timeToEnd = this.duration > 0 ? this.duration - this.currentTime : Infinity;
          
          // 只在接近结束时检查是否播放完毕
          if (timeToEnd < 1.0 && !this._isDestroyed) {
            const isFinished = await invoke('is_track_finished');
            if (this._isDestroyed) return;
            if (isFinished) {
              this._onEnded();
              return;
            }
          }
          
          this._statusPollId = setTimeout(poll, checkInterval);
        } catch (error) {
          if (this._isDestroyed) return;
          logger.error("Error checking track status:", error);
          this._statusPollId = setTimeout(poll, checkInterval);
        }
      };
      
      this._statusPollId = setTimeout(poll, checkInterval);
    },

    stopStatusPolling() {
      if (this._statusPollId) {
        clearTimeout(this._statusPollId);
        this._statusPollId = null;
      }
    },

    // --- 播放结束 ---

    async _onEnded() {
      // 检查是否已销毁
      if (this._isDestroyed) return;
      
      invoke('pause_track').catch(err => logger.debug("pause on ended:", err));

      if (this.repeatMode === 'track') {
        await this.playTrack(this.currentTrack);
      } else if (this.repeatMode === 'list') {
        const nextIndex = (this.currentTrackIndex + 1) % this.playlist.length;
        await this.playTrack(this.playlist[nextIndex]);
      } else if (this.currentTrackIndex < this.playlist.length - 1) {
        const nextIndex = this.currentTrackIndex + 1;
        await this.playTrack(this.playlist[nextIndex]);
      } else {
        this.isPlaying = false;
        this.stopStatusPolling();
        this.currentTime = this.duration;
        invoke('pause_track').catch(err => logger.debug("pause after playlist ended:", err));
      }
    },

    // --- 文件检查 ---

    async _checkFileExists(filePath) {
      if (!filePath) return false;

      const cache = this._getFileExistsCache();
      const cached = cache.get(filePath);
      if (cached !== null) {
        return cached;
      }

      try {
        const exists = await FileUtils.fileExists(filePath);
        cache.set(filePath, exists);
        return exists;
      } catch (error) {
        logger.error('Error checking file:', error);
        cache.set(filePath, false);
        return false;
      }
    },

    async resetPlayerState() {
      logger.info('Resetting player state');

      this.isPlaying = false;
      this.stopStatusPolling();

      this.currentTrack = null;
      this.playlist = [];
      this.currentTime = 0;
      this.duration = 0;
      this.lyrics = null;
      this.currentLyricIndex = -1;

      // 清除缓存
      if (this._fileExistsCache) {
        this._fileExistsCache.clear();
      }
      if (this._metadataCache) {
        this._metadataCache.clear();
      }

      try {
        await invoke('pause_track');
      } catch (error) {
        logger.error('Error stopping backend playback:', error);
      }
    },

    async nextTrack() {
      if (!this.currentTrack || this._isLoading) return;

      let nextIndex;
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          nextIndex = 0;
        } else {
          do {
            nextIndex = Math.floor(Math.random() * this.playlist.length);
          } while (nextIndex === this.currentTrackIndex);
        }
      } else {
        nextIndex = (this.currentTrackIndex + 1) % this.playlist.length;
      }

      await this.playTrack(this.playlist[nextIndex]);
    },

    async previousTrack() {
      if (!this.currentTrack || this._isLoading) return;

      let prevIndex;
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          prevIndex = 0;
        } else {
          do {
            prevIndex = Math.floor(Math.random() * this.playlist.length);
          } while (prevIndex === this.currentTrackIndex);
        }
      } else {
        prevIndex = this.currentTrackIndex - 1;
        if (prevIndex < 0) {
          prevIndex = this.playlist.length - 1;
        }
      }

      await this.playTrack(this.playlist[prevIndex]);
    },

    // --- 播放控制 ---

    seek(time) {
      if (!this.currentTrack) return;

      const newTime = Math.max(0, Math.min(time, this.duration));

      invoke('seek_track', { time: newTime })
        .then(() => {
          this.currentTime = newTime;
          if (!this.isPlaying) {
            this.resume();
          }
        })
        .catch(err => logger.error("Failed to seek:", err));
    },

    setVolume(volume) {
      const newVolume = Math.max(0, Math.min(1, volume));
      this.volume = newVolume;
      
      invoke('set_volume', { volume: newVolume })
        .then(() => {
          const configStore = useConfigStore();
          configStore.audio.volume = newVolume;
          configStore.saveConfigNow();
        })
        .catch(err => logger.error("Failed to set volume:", err));
    },

    toggleRepeat() {
      if (this.repeatMode === 'none') {
        this.repeatMode = 'list';
        this.isShuffle = false;
      } else if (this.repeatMode === 'list') {
        this.repeatMode = 'track';
      } else {
        this.repeatMode = 'none';
      }
    },

    toggleShuffle() {
      this.isShuffle = !this.isShuffle;
      if (this.isShuffle) {
        this.repeatMode = 'none';
      }
    },

    // --- 歌词偏移 ---
    
    setLyricsOffset(offset) {
      this.lyricsOffset = offset;
    },

    adjustLyricsOffset(delta) {
      this.lyricsOffset = Math.round((this.lyricsOffset + delta) * 10) / 10;
    },

    resetLyricsOffset() {
      this.lyricsOffset = 0;
    },

    // --- 数据加载 ---

    loadPlaylist(playlist) {
      this.playlist = playlist;
      if (playlist && playlist.length > 0) {
        const firstTrack = playlist[0];
        this.currentTrack = firstTrack;
        this.duration = firstTrack.duration || 0;
        this.audioInfo = {
          bitrate: firstTrack.bitrate || null,
          sampleRate: firstTrack.sampleRate || null,
          channels: firstTrack.channels || null,
          bitDepth: firstTrack.bitDepth || null,
          format: firstTrack.format || null,
        };
      } else {
        this.currentTrack = null;
        this.isPlaying = false;
        this.currentTime = 0;
        this.duration = 0;
        this.stopStatusPolling();
      }

      this._cachePlaylistMetadata(playlist);
    },

    /**
     * 缓存播放列表元数据 - 使用异步分块处理
     */
    async _cachePlaylistMetadata(playlist) {
      if (!playlist || playlist.length === 0) return;

      const cache = this._getMetadataCache();
      const CHUNK_SIZE = 50; // 每 50 个让出一次主线程
      let cached = 0;

      for (let i = 0; i < playlist.length; i++) {
        const track = playlist[i];
        if (!track.path || cache.has(track.path)) continue;

        cache.set(track.path, {
          title: track.displayTitle || track.title || track.name || FileUtils.getFileName(track.path),
          artist: track.displayArtist || track.artist || '',
          album: track.album || '',
          duration: track.duration || 0,
          bitrate: track.bitrate || null,
          sampleRate: track.sampleRate || null,
          channels: track.channels || null,
          bitDepth: track.bitDepth || null,
          format: track.format || null,
        });
        cached++;
        
        // 分块让出主线程
        if (cached > 0 && cached % CHUNK_SIZE === 0) {
          await new Promise(resolve => setTimeout(resolve, 0));
        }
      }

      logger.debug(`Cached metadata for ${cached} tracks`);
    },

    async loadLyrics(trackPath) {
      try {
        const lyricsPath = await FileUtils.findLyricsFile(trackPath);
        if (lyricsPath) {
          const lyricsContent = await FileUtils.readFile(lyricsPath);
          const format = FileUtils.getFileExtension(lyricsPath);
          this.lyrics = LyricsParser.parse(lyricsContent, format);
        } else {
          this.lyrics = null;
        }
      } catch (error) {
        logger.debug('No lyrics found or failed to load:', error);
        this.lyrics = null;
      }
    },

    // --- 清理 ---
    cleanup() {
      // 设置销毁标志，阻止所有后续的 invoke 调用
      this._isDestroyed = true;
      
      this.stopStatusPolling();
      this._stopCleanupTask();
      
      // 取消播放结束事件监听
      if (this._trackEndedUnlisten) {
        this._trackEndedUnlisten();
        this._trackEndedUnlisten = null;
      }
      
      // 取消播放位置事件监听
      if (this._positionUnlisten) {
        this._positionUnlisten();
        this._positionUnlisten = null;
      }
      
      // 使用 try-catch 包裹，避免在销毁时抛出错误
      try {
        invoke('pause_track').catch(() => {});
      } catch {
        // 忽略错误
      }

      if (this._fileExistsCache) {
        this._fileExistsCache.clear();
        this._fileExistsCache = null;
      }
      if (this._metadataCache) {
        this._metadataCache.clear();
        this._metadataCache = null;
      }
      
      logger.info('Player store cleaned up');
    }
  }
});
