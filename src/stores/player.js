import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import FileUtils from '../utils/fileUtils';
import LyricsParser from '../utils/lyricsParser';
import logger from '../utils/logger';
import errorHandler, { ErrorType, ErrorSeverity, handlePromise } from '../utils/errorHandler';

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
    lyricsOffset: 0, // 歌词偏移（秒），正值表示歌词提前，负值表示歌词延后

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
    _statusPollId: null, // 状态轮询ID
    lastTrackIndex: -1, // 上一次播放的歌曲索引

    // 文件检查缓存和配置
    _fileExistsCache: new Map(), // 文件存在性缓存
    _lastFileCheckTime: 0, // 上次文件检查时间
    _fileCheckInterval: 5000, // 文件检查间隔：5秒
    _lastPlaylistCheckTime: 0, // 上次播放列表检查时间
    _playlistCheckInterval: 10000, // 播放列表检查间隔：10秒
  }),

  getters: {
    currentTrackIndex: (state) => {
      if (!state.currentTrack || state.playlist.length === 0) return -1;
      return state.playlist.findIndex(track => track.path === state.currentTrack.path);
    },
    hasNextTrack: (state) => {
      if (!state.currentTrack) return false;
      // 如果当前播放列表不为空，则有下一首歌曲
      return state.playlist.length > 0;
    },
    hasPreviousTrack: (state) => {
      if (!state.currentTrack) return false;
      // 如果当前播放列表不为空，则有上一首歌曲
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
    // --- 初始化 ---
    async initAudio() {
      // 初始化音频播放器
      // 从配置加载音量
      try {
        const { useConfigStore } = await import('./config');
        const configStore = useConfigStore();
        const savedVolume = configStore.audio.volume;
        if (typeof savedVolume === 'number' && savedVolume >= 0 && savedVolume <= 1) {
          this.volume = savedVolume;
          await invoke('set_volume', { volume: savedVolume });
        }
      } catch (err) {
        logger.error('Failed to load volume from config:', err);
      }
      logger.info('Player store initialized.');
    },

    // --- 核心行为 ---

    /**
     * 播放当前歌曲或播放列表中的第一首歌曲。
     * 如果当前没有歌曲，则尝试播放播放列表中的第一首歌曲。
     * 如果播放列表为空，则停止播放。
     */
    play() {
      if (this.currentTrack) {
        // 如果当前有歌曲，则重新播放当前歌曲（无论是否是列表最后一首）
        this.playTrack(this.currentTrack);
      } else if (this.playlist.length > 0) {
        // 如果没有当前歌曲但有播放列表，则播放第一首
        this.playTrack(this.playlist[0]);
      }
    },

    /**
     * 从后端加载播放列表
     * @param {object} track
     */
    async playTrack(track) {
      if (!track || this._isLoading) return;

      // 检查文件是否存在，如果不存在则尝试修复路径（使用缓存优化）
      let trackExists = await this.checkCurrentTrackExists();
      if (!trackExists && track.path) {
        // 尝试修复路径格式
        const altPath = track.path.includes('/') ? track.path.replace(/\//g, '\\') : track.path.replace(/\\/g, '/');
        if (altPath !== track.path) {
          track.path = altPath;
          // 强制重新检查（绕过缓存）
          const directExists = await FileUtils.fileExists(altPath);
          if (directExists) {
            this.setCachedFileExists(altPath, true);
            trackExists = true;
          } else {
            this.setCachedFileExists(altPath, false);
          }
        }
      }

      if (!trackExists) {
        logger.warn('Track file not found:', track.path);
        // 如果是播放列表中的歌曲，尝试播放下一首
        const currentTrackIndex = this.playlist.findIndex(t => t.path === track.path);
        if (this.playlist.length > 1 && currentTrackIndex < this.playlist.length - 1) {
          logger.debug('Attempting to play next track...');
          return this.nextTrack();
        } else {
          await this.resetPlayerState();
          return;
        }
      }

      this._isLoading = true;
      this.stopStatusPolling();

      // 先更新 UI 状态，让用户看到响应
      let metadata = {};
      if (this._metadataCache && this._metadataCache[track.path]) {
        metadata = this._metadataCache[track.path];
      } else {
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

      // 异步暂停当前播放，不等待
      invoke('pause_track').catch(err => logger.debug("pause before play:", err));

      try {
        logger.info('Playing track:', track.path);
        
        // 使用带超时的 Promise，防止后端卡住导致 UI 卡顿
        const playPromise = invoke('play_track', { path: track.path });
        const timeoutPromise = new Promise((_, reject) => {
          setTimeout(() => reject(new Error('播放超时')), 10000); // 10秒超时
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
          
          // 播放失败时自动跳到下一首
          const currentIdx = this.playlist.findIndex(t => t.path === track.path);
          if (this.playlist.length > 1 && currentIdx < this.playlist.length - 1) {
            logger.info('Auto-skipping to next track due to playback error');
            // 使用 setTimeout 避免递归调用栈过深
            setTimeout(() => this.nextTrack(), 100);
          }
          return;
        }

        logger.debug('Track play command sent successfully');
        this.isPlaying = true;
        this.startStatusPolling();
        
        // 异步加载歌词，不阻塞播放
        this.loadLyrics(track.path).catch(err => {
          logger.debug('Lyrics load error:', err);
        });
      } catch (error) {
        logger.error('Playback error:', error);
        this.isPlaying = false;
        
        // 播放失败时自动跳到下一首
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
        // 检查播放进度是否在结尾处，或者后端已经播放完成
        const isAtEnd = this.duration > 0 && this.currentTime >= this.duration - 0.5;

        // 如果已经播放完成或接近结尾，重新从头播放
        if (isAtEnd) {
          this.play(); // 重新从头播放
        } else {
          this.resume(); // 从当前位置恢复播放
        }
      }
    },

    // --- 进度控制 ---

    startStatusPolling() {
      this.stopStatusPolling(); // 停止之前的轮询
      
      // 使用动态轮询间隔：正常播放时 500ms，接近结尾时 100ms
      const normalInterval = 500; // ms
      const fastInterval = 100; // ms - 接近结尾时使用更快的轮询
      
      const poll = async () => {
        if (!this.isPlaying) {
          this.stopStatusPolling();
          return;
        }

        try {
          // 计算距离结尾的时间
          const timeToEnd = this.duration > 0 ? this.duration - this.currentTime : Infinity;
          const isNearEnd = timeToEnd < 2.0; // 距离结尾2秒内
          const currentInterval = isNearEnd ? fastInterval : normalInterval;

          if (isNearEnd && timeToEnd < 1.0) {
            // 只有在非常接近结尾时才检查后端状态
            const isFinished = await invoke('is_track_finished');
            if (isFinished) {
              this._onEnded();
              return;
            }
          }
          
          // 更新当前时间
          const newTime = this.currentTime + (currentInterval / 1000);

          if (this.duration > 0 && newTime >= this.duration) {
            this.currentTime = this.duration;
            // 不直接调用_onEnded，等待后端反馈
          } else {
            this.currentTime = newTime;
          }
          
          // 使用动态间隔调度下一次轮询
          this._statusPollId = setTimeout(poll, currentInterval);
        } catch (error) {
          logger.error("Error checking track status:", error);
          // 出错时使用基于时长的判断作为后备
          const newTime = this.currentTime + (normalInterval / 1000);
          if (this.duration > 0 && newTime >= this.duration) {
            this.currentTime = this.duration;
            this._onEnded();
            return;
          } else {
            this.currentTime = newTime;
          }
          // 出错后继续轮询
          this._statusPollId = setTimeout(poll, normalInterval);
        }
      };
      
      // 启动轮询
      this._statusPollId = setTimeout(poll, normalInterval);
    },

    stopStatusPolling() {
      if (this._statusPollId) {
        clearTimeout(this._statusPollId);
        this._statusPollId = null;
      }
    },

    // --- 播放结束 ---

    async _onEnded() {
      // 异步停止后端播放，不阻塞
      invoke('pause_track').catch(err => logger.debug("pause on ended:", err));

      // 异步检查播放列表文件
      const now = Date.now();
      const shouldCheckPlaylist = (now - this._lastPlaylistCheckTime > this._playlistCheckInterval) ||
        (this._lastPlaylistCheckTime === 0);

      if (shouldCheckPlaylist) {
        this.checkPlaylistFilesExist().then(exists => {
          if (!exists) {
            this.resetPlayerState();
          }
        });
      }

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

    /**
     * 检查当前播放列表的歌曲文件是否仍然存在（带缓存优化）
     */
    async checkPlaylistFilesExist() {
      if (!this.playlist || this.playlist.length === 0) return true;

      const now = Date.now();
      // 如果距离上次检查时间太短，返回缓存结果或默认true
      if (now - this._lastPlaylistCheckTime < this._playlistCheckInterval) {
        return true; // 短时间内假设文件仍然存在
      }

      try {
        let allFilesExist = true;
        // 只检查缓存中不存在的文件或者缓存过期的文件
        for (const track of this.playlist) {
          let exists = this.getCachedFileExists(track.path);

          if (exists === null) { // 缓存中没有，需要实际检查
            // 先检查原始路径
            exists = await FileUtils.fileExists(track.path);

            // 如果原始路径不存在，尝试另一种路径分隔符格式
            if (!exists && track.path) {
              const altPath = track.path.includes('/') ? track.path.replace(/\//g, '\\') : track.path.replace(/\\/g, '/');
              if (altPath !== track.path) {
                const altExists = await FileUtils.fileExists(altPath);
                if (altExists) {
                  track.path = altPath;
                  exists = true;
                }
              }
            }

            // 更新缓存
            this.setCachedFileExists(exists ? track.path : track.path, exists);
          }

          if (!exists) {
            logger.warn(`File not found in playlist: ${track.path}`);
            allFilesExist = false;
            break;
          }
        }

        this._lastPlaylistCheckTime = now;
        return allFilesExist;
      } catch (error) {
        logger.error('Error checking playlist files:', error);
        return false;
      }
    },

    /**
     * 检查当前播放的歌曲是否仍然存在（带缓存优化）
     */
    async checkCurrentTrackExists() {
      if (!this.currentTrack) return false;

      // 先检查缓存
      const cachedResult = this.getCachedFileExists(this.currentTrack.path);
      if (cachedResult !== null) {
        return cachedResult;
      }

      try {
        // 先检查原始路径
        let exists = await FileUtils.fileExists(this.currentTrack.path);
        if (exists) {
          this.setCachedFileExists(this.currentTrack.path, true);
          return true;
        }

        // 如果原始路径不存在，尝试另一种路径分隔符格式
        const altPath = this.currentTrack.path.includes('/')
          ? this.currentTrack.path.replace(/\//g, '\\')
          : this.currentTrack.path.replace(/\\/g, '/');

        if (altPath !== this.currentTrack.path) {
          exists = await FileUtils.fileExists(altPath);
          if (exists) {
            // 更新路径为有效的格式
            this.currentTrack.path = altPath;
            this.setCachedFileExists(altPath, true);
            return true;
          }
        }

        this.setCachedFileExists(this.currentTrack.path, false);
        return false;
      } catch (error) {
        logger.error('Error checking current track:', error);
        this.setCachedFileExists(this.currentTrack.path, false);
        return false;
      }
    },

    /**
     * 获取缓存的文件存在性
     * @param {string} filePath 文件路径
     * @returns {boolean|null} 缓存结果，null表示没有缓存
     */
    getCachedFileExists(filePath) {
      if (!filePath || !this._fileExistsCache) return null;

      const cached = this._fileExistsCache.get(filePath);
      if (!cached) return null;

      // 如果缓存超过30秒，认为过期
      const now = Date.now();
      if (now - cached.timestamp > 30000) {
        this._fileExistsCache.delete(filePath);
        return null;
      }

      return cached.exists;
    },

    /**
     * 设置文件存在性缓存
     * @param {string} filePath 文件路径
     * @param {boolean} exists 是否存在
     */
    setCachedFileExists(filePath, exists) {
      if (!filePath || !this._fileExistsCache) return;

      this._fileExistsCache.set(filePath, {
        exists: exists,
        timestamp: Date.now()
      });

      // 清理过期的缓存条目（保持缓存大小合理）
      this.cleanExpiredFileCache();
    },

    /**
     * 清理过期的文件缓存
     */
    cleanExpiredFileCache() {
      if (!this._fileExistsCache) return;

      const now = Date.now();
      const maxSize = 200; // 最大缓存条目数

      // 删除过期条目
      for (const [path, cached] of this._fileExistsCache.entries()) {
        if (now - cached.timestamp > 60000) { // 1分钟过期
          this._fileExistsCache.delete(path);
        }
      }

      // 如果还是太大，删除最旧的条目
      if (this._fileExistsCache.size > maxSize) {
        const entries = Array.from(this._fileExistsCache.entries())
          .sort((a, b) => a[1].timestamp - b[1].timestamp);

        const toDelete = entries.slice(0, this._fileExistsCache.size - maxSize);
        toDelete.forEach(([path]) => this._fileExistsCache.delete(path));
      }
    },

    /**
     * 重置播放器状态到初始状态（当播放列表被删除时）
     */
    async resetPlayerState() {
      logger.info('Resetting player state due to missing playlist files');

      // 停止播放
      this.isPlaying = false;
      this.stopStatusPolling();

      // 清空播放器状态
      this.currentTrack = null;
      this.playlist = [];
      this.currentTime = 0;
      this.duration = 0;
      this.lyrics = null;
      this.currentLyricIndex = -1;

      // 清除所有缓存
      if (this._metadataCache) {
        this._metadataCache = {};
      }
      if (this._fileExistsCache) {
        this._fileExistsCache.clear();
      }

      // 重置检查时间
      this._lastFileCheckTime = 0;
      this._lastPlaylistCheckTime = 0;

      // 停止后端播放
      try {
        await invoke('pause_track');
        } catch (error) {
          logger.error('Error stopping backend playback:', error);
        }
    },

    async nextTrack() {
      if (!this.currentTrack || this._isLoading) return;

      // 减少播放列表文件检查频率
      const now = Date.now();
      const shouldCheckPlaylist = (now - this._lastPlaylistCheckTime > this._playlistCheckInterval) ||
        (this._lastPlaylistCheckTime === 0);

      if (shouldCheckPlaylist) {
        // 异步检查，不阻塞
        this.checkPlaylistFilesExist().then(exists => {
          if (!exists) {
            this.resetPlayerState();
          }
        });
      }

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

      // 减少播放列表文件检查频率
      const now = Date.now();
      const shouldCheckPlaylist = (now - this._lastPlaylistCheckTime > this._playlistCheckInterval) ||
        (this._lastPlaylistCheckTime === 0);

      if (shouldCheckPlaylist) {
        // 异步检查，不阻塞
        this.checkPlaylistFilesExist().then(exists => {
          if (!exists) {
            this.resetPlayerState();
          }
        });
      }

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
          this.currentTime = newTime; // Update time after backend confirms
          if (!this.isPlaying) {
            this.resume();
          }
        })
        .catch(err => logger.error("Failed to seek:", err));
    },

    setVolume(volume) {
      const newVolume = Math.max(0, Math.min(1, volume));
      // 立即更新本地状态，避免滑柄抖动
      this.volume = newVolume;
      
      invoke('set_volume', { volume: newVolume })
        .then(async () => {
          // 保存音量到配置
          const { useConfigStore } = await import('./config');
          const configStore = useConfigStore();
          configStore.audio.volume = newVolume;
          configStore.saveConfigNow();
        })
        .catch(err => logger.error("Failed to set volume:", err));
    },

    toggleRepeat() {
      // 循环模式切换：none -> list -> track -> none
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
    
    /**
     * 设置歌词偏移（秒）
     * @param {number} offset - 偏移值，正值表示歌词提前，负值表示歌词延后
     */
    setLyricsOffset(offset) {
      this.lyricsOffset = offset;
    },

    /**
     * 调整歌词偏移
     * @param {number} delta - 调整量（秒）
     */
    adjustLyricsOffset(delta) {
      this.lyricsOffset = Math.round((this.lyricsOffset + delta) * 10) / 10; // 保留一位小数
    },

    /**
     * 重置歌词偏移
     */
    resetLyricsOffset() {
      this.lyricsOffset = 0;
    },

    // --- 数据加载 ---

    loadPlaylist(playlist) {
      this.playlist = playlist;
      if (playlist && playlist.length > 0) {
        // 设置当前播放的第一个但是不自动播放
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
        // 播放列表为空时，重置播放器状态
        this.currentTrack = null;
        this.isPlaying = false;
        this.currentTime = 0;
        this.duration = 0;
        this.stopStatusPolling();
      }

      // 预处理播放列表中所有文件的元数据
      this.preprocessPlaylistMetadata(playlist);
    },

    /**
     * 预处理播放列表中所有文件的元数据
     * 直接使用播放列表中已有的元数据（由 playlistManager 批量获取）
     * @param {Array} playlist - 播放列表
     */
    async preprocessPlaylistMetadata(playlist) {
      if (!playlist || playlist.length === 0) return;

      logger.debug(`预处理播放列表元数据，共 ${playlist.length} 个文件`);

      // 初始化元数据缓存（如果还没有）
      if (!this._metadataCache) {
        this._metadataCache = {};
      }

      // 处理每个轨道，直接使用已有的元数据
      for (const track of playlist) {
        const trackPath = track.path;

        // 如果缓存中已经有，跳过
        if (this._metadataCache[trackPath]) {
          continue;
        }

        // 使用播放列表中已有的数据填充缓存
        // playlistManager.processAudioFiles 已经批量获取了所有需要的信息
        this._metadataCache[trackPath] = {
          title: track.displayTitle || track.title || track.name || FileUtils.getFileName(trackPath),
          artist: track.displayArtist || track.artist || '',
          album: track.album || '',
          duration: track.duration || 0,
          bitrate: track.bitrate || null,
          sampleRate: track.sampleRate || null,
          channels: track.channels || null,
          bitDepth: track.bitDepth || null,
          format: track.format || null,
          isFromMetadata: track.isFromMetadata || false,
          lastUpdated: new Date().toISOString()
        };
      }

      logger.debug(`播放列表元数据预处理完成`);
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
      this.stopStatusPolling();
      // 停止后端播放
      invoke('pause_track');

      // 清理所有缓存
      if (this._fileExistsCache) {
        this._fileExistsCache.clear();
      }
      if (this._metadataCache) {
        this._metadataCache = {};
      }
    }
  }
});
