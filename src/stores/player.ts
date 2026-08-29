import { defineStore } from 'pinia'
import { markRaw } from 'vue'
import i18n from '@/i18n'
import { invoke } from '@tauri-apps/api/core'
import { type UnlistenFn } from '@tauri-apps/api/event'
import FileUtils from '../utils/fileUtils'
import LyricsParser from '../utils/lyricsParser'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity } from '../utils/errorHandler'
import { classifyAudioInvokeError } from '../utils/audioErrorClassifier'
import { useConfigStore } from './config'
import {
  setupTrackEndedListener,
  setupPositionListener,
  setupTaskbarListeners,
  setupGlobalShortcuts,
  setupDeviceListeners,
  unregisterGlobalShortcuts,
} from './playerListeners'
import {
  generateShuffleOrder,
  isShuffleOrderValid,
  getNextShuffleIndex,
  getPreviousShuffleIndex,
} from './shuffle'
import { PlayerCacheManager } from './playerCache'
import { saveLastSessionNow, resumeLastSession } from './playerSession'

// ============================================================================
// 常量
// ============================================================================

/// play_track IPC 的超时(毫秒),超时视为后端无响应并报错
const PLAY_TRACK_TIMEOUT_MS = 5000
/// 播放结束自动跳到下一首的延迟(毫秒),给 UI 留出状态刷新窗口
const AUTO_NEXT_TRACK_DELAY_MS = 100

import {
  addTrackNextInPlaylist,
  addTracksNextInPlaylist,
  removeTrackFromPlaylist,
} from './playerPlaylist'
import {
  cachePlaylistMetadata,
  loadPlaylistCovers,
  recordCoverUpdate as recordCoverUpdateToMap,
  takeCoverUpdates as takeCoverUpdatesFromMap,
} from './playerMediaCache'
import type { Track, AudioInfo, LyricLine, RepeatMode, ResumeResult } from '@/types'

interface PlayerState {
  currentTrack: Track | null
  playlist: Track[]
  isPlaying: boolean
  currentTime: number
  duration: number
  volume: number
  isMuted: boolean
  previousVolume: number
  repeatMode: RepeatMode
  isShuffle: boolean
  lyrics: LyricLine[] | null
  currentLyricIndex: number
  lyricsOffset: number
  audioInfo: AudioInfo
  _isLoading: boolean
  lastTrackIndex: number
  /** 缓存管理器 (文件存在性 + 元数据 LRU 缓存),markRaw 避免深度代理 class 实例 */
  _cacheManager: PlayerCacheManager | null
  _isDestroyed: boolean
  _isInitializing: boolean
  _initPromise: Promise<void> | null
  _trackEndedUnlisten: UnlistenFn | null
  _positionUnlisten: UnlistenFn | null
  _taskbarPreviousUnlisten: UnlistenFn | null
  _taskbarPlayPauseUnlisten: UnlistenFn | null
  _taskbarNextUnlisten: UnlistenFn | null
  _deviceRemovedUnlisten: UnlistenFn | null
  _deviceSwitchRequiredUnlisten: UnlistenFn | null
  _noDeviceAvailableUnlisten: UnlistenFn | null
  _deviceDefaultChangedUnlisten: UnlistenFn | null
  _playRequestId: number
  _activePlayRequestId: number
  _lyricsRequestId: number
  _isSwitchingDevice: boolean
  _lastDeviceSwitchTarget: string | null
  /** 用于取消正在进行的 _cachePlaylistMetadata 任务 */
  _cacheAbortController: AbortController | null
  /** 用于清理 nextTrack 定时器 */
  _nextTrackTimeoutId: ReturnType<typeof setTimeout> | null
  /** shuffle 模式下,Knuth 洗牌后的播放索引序列 (空数组表示未生成) */
  _shuffleOrder: number[]
  /** 当前在 _shuffleOrder 中的位置 (-1 表示未定位) */
  _shufflePosition: number
  /** 历史栈:记录已播放过的索引,用于 previousTrack 真正回到上一首 */
  _shuffleHistory: number[]
  /** 封面加载完成版本号:_loadPlaylistCovers 每处理完一批递增一次,
   *  供 PlaylistView 以 O(变更数) 而非 O(N²) 感知封面更新 */
  playlistCoverVersion: number
}

// 待通知的封面更新队列与批量加载实现见 playerMediaCache.ts

export const usePlayerStore = defineStore('player', {
  state: (): PlayerState => ({
    // 当前播放状态
    currentTrack: null,
    playlist: [],
    isPlaying: false,
    currentTime: 0,
    duration: 0,
    volume: 1,
    isMuted: false,
    previousVolume: 1,

    // 重复模式设置
    repeatMode: 'none',
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
      format: null,
    },

    // 加载状态
    _isLoading: false,
    lastTrackIndex: -1,

    // 缓存管理器
    _cacheManager: null,

    // 销毁标志
    _isDestroyed: false,
    _isInitializing: false,
    _initPromise: null,

    // 事件监听器
    _trackEndedUnlisten: null,
    _positionUnlisten: null,
    _taskbarPreviousUnlisten: null,
    _taskbarPlayPauseUnlisten: null,
    _taskbarNextUnlisten: null,
    _deviceRemovedUnlisten: null,
    _deviceSwitchRequiredUnlisten: null,
    _noDeviceAvailableUnlisten: null,
    _deviceDefaultChangedUnlisten: null,

    // 并发保护
    _playRequestId: 0,
    _activePlayRequestId: 0,
    _lyricsRequestId: 0,
    _isSwitchingDevice: false,
    _lastDeviceSwitchTarget: null,
    _cacheAbortController: null,
    _nextTrackTimeoutId: null,
    // shuffle 状态: 空数组表示未生成洗牌顺序
    _shuffleOrder: [],
    _shufflePosition: -1,
    _shuffleHistory: [],

    // 封面加载版本号
    playlistCoverVersion: 0,
  }),

  getters: {
    currentTrackIndex: (state): number => {
      if (!state.currentTrack || state.playlist.length === 0) return -1
      return state.playlist.findIndex((track) => track.path === state.currentTrack!.path)
    },
    hasNextTrack: (state): boolean => {
      // 手动切换应总是允许,与循环模式无关;自动结束行为由 _onEnded 处理
      if (!state.currentTrack || state.playlist.length <= 1) return false
      return true
    },
    hasPreviousTrack: (state): boolean => {
      // 手动切换应总是允许,与循环模式无关
      if (!state.currentTrack || state.playlist.length <= 1) return false
      return true
    },
    currentLyric: (state): LyricLine | null => {
      if (
        !state.lyrics ||
        state.currentLyricIndex < 0 ||
        state.currentLyricIndex >= state.lyrics.length
      ) {
        return null
      }
      return state.lyrics[state.currentLyricIndex]
    },
  },

  actions: {
    // --- 缓存管理 ---

    _getCacheManager(): PlayerCacheManager {
      if (!this._cacheManager) {
        this._cacheManager = markRaw(new PlayerCacheManager())
      }
      return this._cacheManager as PlayerCacheManager
    },

    _getFileExistsCache() {
      return this._getCacheManager().getFileExistsCache()
    },

    _getMetadataCache() {
      return this._getCacheManager().getMetadataCache()
    },

    _startCleanupTask(): void {
      this._getCacheManager().startCleanupTask()
    },

    _stopCleanupTask(): void {
      this._cacheManager?.stopCleanupTask()
    },

    // --- 初始化 ---
    async initAudio(): Promise<void> {
      if (this._isInitializing && this._initPromise) {
        await this._initPromise
        return
      }

      if (
        this._trackEndedUnlisten ||
        this._positionUnlisten ||
        this._taskbarPreviousUnlisten ||
        this._taskbarPlayPauseUnlisten ||
        this._taskbarNextUnlisten ||
        this._deviceRemovedUnlisten ||
        this._deviceSwitchRequiredUnlisten ||
        this._noDeviceAvailableUnlisten ||
        this._deviceDefaultChangedUnlisten
      ) {
        logger.warn('Player store already initialized, skipping duplicate initAudio call')
        return
      }

      this._isDestroyed = false
      this._isInitializing = true

      this._initPromise = (async () => {
        try {
          try {
            const configStore = useConfigStore()
            const savedVolume = configStore.audio.volume
            if (typeof savedVolume === 'number' && savedVolume >= 0 && savedVolume <= 1) {
              this.volume = savedVolume
              await invoke('set_volume', { volume: savedVolume })
            }
          } catch (err) {
            logger.error('Failed to load volume from config:', err)
          }

          await this._setupListeners()
          this._startCleanupTask()

          logger.info('Player store initialized.')
        } finally {
          this._isInitializing = false
          this._initPromise = null
        }
      })()

      await this._initPromise
    },

    /**
     * 统一设置所有事件监听器 (track-ended / playback-position / taskbar / device / global shortcuts)
     * 监听器实现抽离到 playerListeners.ts
     */
    async _setupListeners(): Promise<void> {
      this._trackEndedUnlisten = await setupTrackEndedListener(this)
      this._positionUnlisten = await setupPositionListener(this)

      const taskbarListeners = await setupTaskbarListeners(this)
      this._taskbarPreviousUnlisten = taskbarListeners.previous
      this._taskbarPlayPauseUnlisten = taskbarListeners.playPause
      this._taskbarNextUnlisten = taskbarListeners.next

      const deviceListeners = await setupDeviceListeners(this)
      this._deviceRemovedUnlisten = deviceListeners.removed
      this._deviceSwitchRequiredUnlisten = deviceListeners.switchRequired
      this._noDeviceAvailableUnlisten = deviceListeners.noDevice
      this._deviceDefaultChangedUnlisten = deviceListeners.defaultChanged

      await setupGlobalShortcuts(this)
    },

    /**
     * 立即保存 last_session (无节流,用于 pause/切曲/关闭等关键节点)
     * 实现在 playerSession.ts
     */
    async _saveLastSessionNow(): Promise<void> {
      await saveLastSessionNow(this)
    },

    /**
     * 启动时调用 - 尝试恢复上次播放会话
     * 实现在 playerSession.ts
     */
    async resumeLastSession(): Promise<ResumeResult | null> {
      return resumeLastSession(this)
    },

    async _switchAudioDevice(
      deviceName: string,
      successAction: 'switch-fallback-success' | 'switch-default-success',
      successMessage: string,
      errorAction: 'switch-fallback' | 'switch-default',
      errorSeverity: ErrorSeverity,
    ): Promise<void> {
      if (this._isDestroyed || !deviceName) return
      if (this._isSwitchingDevice) {
        if (this._lastDeviceSwitchTarget === deviceName) {
          logger.debug(`Skip duplicated device switch event: ${deviceName}`)
        } else {
          logger.warn(`Device switch ignored while another switch is running: ${deviceName}`)
        }
        return
      }

      this._isSwitchingDevice = true
      this._lastDeviceSwitchTarget = deviceName

      const wasPlaying = this.isPlaying
      const currentTime = this.currentTime

      try {
        await invoke('set_audio_device', {
          deviceName,
          currentTime,
        })

        if (!wasPlaying && this.currentTrack) {
          await invoke('pause_track')
          this.isPlaying = false
        }

        logger.info(`Successfully switched audio device: ${deviceName}`)
        errorHandler.handle(new Error(`Device switched to ${deviceName}`), {
          type: ErrorType.AUDIO_DEVICE_ERROR,
          severity: ErrorSeverity.LOW,
          context: { deviceName, action: successAction },
          showToUser: true,
          userMessage: successMessage,
        })
      } catch (err) {
        errorHandler.handle(err instanceof Error ? err : new Error(String(err)), {
          type: ErrorType.AUDIO_DEVICE_ERROR,
          severity: errorSeverity,
          context: { deviceName, action: errorAction },
          showToUser: true,
          userMessage:
            errorAction === 'switch-fallback'
              ? i18n.global.t('errors.switchDeviceFallbackFailed', { device: deviceName })
              : i18n.global.t('errors.switchDeviceFailed', { device: deviceName }),
        })
      } finally {
        this._isSwitchingDevice = false
        this._lastDeviceSwitchTarget = null
      }
    },

    _updateTaskbarState(): void {
      // 更新 Windows 任务栏按钮状态
      invoke('update_taskbar_state', { isPlaying: this.isPlaying }).catch(() => {
        // 忽略非 Windows 平台的错误
      })
    },

    // --- 核心行为 ---

    play(): void {
      if (this.currentTrack) {
        this.playTrack(this.currentTrack)
      } else if (this.playlist.length > 0) {
        this.playTrack(this.playlist[0])
      }
    },

    async playTrack(track: Track): Promise<void> {
      if (this._isDestroyed || !track || this._isLoading) return

      // 切曲前保存上一曲的最后位置 (无节流,确保切换瞬间记录最新)
      // 只在确实在切曲 (currentTrack 存在且不是同一首) 时才保存
      if (this.currentTrack && this.currentTrack.path !== track.path) {
        void this._saveLastSessionNow()
      }

      const requestId = ++this._playRequestId
      this._activePlayRequestId = requestId

      let resolvedPath = track.path
      let trackExists = await this._checkFileExists(resolvedPath)
      if (!trackExists && resolvedPath) {
        const altPath = resolvedPath.includes('/')
          ? resolvedPath.replace(/\//g, '\\')
          : resolvedPath.replace(/\\/g, '/')
        if (altPath !== resolvedPath) {
          resolvedPath = altPath
          trackExists = await this._checkFileExists(resolvedPath)
        }
      }

      if (this._activePlayRequestId !== requestId || this._isDestroyed) {
        return
      }

      if (!trackExists) {
        logger.warn('Track file not found:', resolvedPath)
        const currentTrackIndex = this.playlist.findIndex(
          (t) => t.path === track.path || t.path === resolvedPath,
        )
        if (
          this.playlist.length > 1 &&
          currentTrackIndex >= 0 &&
          currentTrackIndex < this.playlist.length - 1
        ) {
          return this.nextTrack()
        }

        await this.resetPlayerState()
        return
      }

      this._isLoading = true

      const metadataCache = this._getMetadataCache()
      let metadata = metadataCache.get(resolvedPath) ?? metadataCache.get(track.path)
      if (!metadata) {
        metadata = {
          title: track.title || FileUtils.getFileNameWithoutExtension(resolvedPath),
          artist: track.artist || '',
          album: track.album || '',
          duration: track.duration || 0,
          bitrate: track.bitrate || null,
          sampleRate: track.sampleRate || null,
          channels: track.channels || null,
          bitDepth: track.bitDepth || null,
          format: track.format || null,
        }
      }

      const resolvedTrack: Track = {
        ...track,
        path: resolvedPath,
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        duration: metadata.duration,
        coverPath: track.coverPath, // 保留原始的 coverPath
      }

      this.lastTrackIndex = this.currentTrackIndex
      this.currentTrack = resolvedTrack

      // shuffle 模式下,用户手动切曲时同步 _shufflePosition 到新曲目在 _shuffleOrder 中的位置
      // 如果新曲目不在 _shuffleOrder 中 (顺序失效/外部触发),则作废顺序,下次 nextTrack 时重新生成
      if (this.isShuffle && this._shuffleOrder.length > 0) {
        const newIdx = this.currentTrackIndex
        const pos = this._shuffleOrder.indexOf(newIdx)
        if (pos >= 0) {
          this._shufflePosition = pos
        } else {
          // 顺序已失效,作废等待下次懒生成
          this._shuffleOrder = []
          this._shufflePosition = -1
        }
      }

      // 按需加载封面路径（如果还没有）
      if (!resolvedTrack.coverPath) {
        logger.debug('Loading cover for track:', resolvedPath)
        invoke<string | null>('get_track_cover_path', { path: resolvedPath })
          .then((coverPath) => {
            logger.debug('Cover path result:', coverPath)
            if (this.currentTrack?.path === resolvedPath && coverPath) {
              this.currentTrack.coverPath = coverPath
              // 同时更新元数据缓存中的封面路径
              const cachedMetadata = metadataCache.get(resolvedPath)
              if (cachedMetadata) {
                cachedMetadata.coverPath = coverPath
                metadataCache.set(resolvedPath, cachedMetadata)
              }
            }
          })
          .catch((err) => logger.error('Failed to load cover path:', err))
      } else {
        logger.debug('Track already has coverPath:', resolvedTrack.coverPath)
      }
      this.duration = metadata.duration || 0
      this.currentTime = 0
      this.lyrics = null
      this.currentLyricIndex = -1
      this.audioInfo = {
        bitrate: metadata.bitrate || null,
        sampleRate: metadata.sampleRate || null,
        channels: metadata.channels || null,
        bitDepth: metadata.bitDepth || null,
        format: metadata.format || null,
      }

      invoke('pause_track').catch((err) => logger.warn('pause before play:', err))

      try {
        logger.info('Playing track:', resolvedPath)

        let timeoutId: ReturnType<typeof setTimeout> | null = null
        const playPromise = invoke('play_track', { path: resolvedPath })
        const timeoutPromise = new Promise((_, reject) => {
          timeoutId = setTimeout(
            () => reject(new Error(i18n.global.t('errors.playTimeout'))),
            PLAY_TRACK_TIMEOUT_MS,
          )
        })

        try {
          await Promise.race([playPromise, timeoutPromise])
        } finally {
          if (timeoutId) {
            clearTimeout(timeoutId)
            timeoutId = null
          }
        }

        if (this._activePlayRequestId !== requestId || this._isDestroyed) {
          return
        }

        this.isPlaying = true
        this._updateTaskbarState()

        this.loadLyrics(resolvedPath, requestId).catch((err) => {
          logger.debug('Lyrics load error:', err)
        })
      } catch (err) {
        if (this._activePlayRequestId !== requestId || this._isDestroyed) {
          return
        }

        const type = classifyAudioInvokeError(err)
        const handled = errorHandler.handle(err instanceof Error ? err : new Error(String(err)), {
          type,
          severity: ErrorSeverity.HIGH,
          context: { trackPath: resolvedPath, trackName: track.name },
          showToUser: true,
        })

        logger.error('Failed to play track:', handled)
        this.isPlaying = false

        const currentIdx = this.playlist.findIndex(
          (t) => t.path === track.path || t.path === resolvedPath,
        )
        if (this.playlist.length > 1 && currentIdx >= 0 && currentIdx < this.playlist.length - 1) {
          const nextTrackTimeoutId = setTimeout(() => {
            if (!this._isDestroyed && this._activePlayRequestId === requestId) {
              this.nextTrack()
            }
          }, AUTO_NEXT_TRACK_DELAY_MS)
          // 保存定时器ID以便在cleanup时清理
          this._nextTrackTimeoutId = nextTrackTimeoutId
        }
      } finally {
        if (this._activePlayRequestId === requestId) {
          this._isLoading = false
        }
      }
    },

    pause(): void {
      if (!this.isPlaying) return
      invoke('pause_track')
        .then(() => {
          this.isPlaying = false
          this._updateTaskbarState()
          // 暂停时立即保存 last_session (无节流)
          void this._saveLastSessionNow()
        })
        .catch((err) => logger.error('Failed to pause:', err))
    },

    resume(): void {
      if (this.isPlaying || !this.currentTrack) return
      invoke('resume_track')
        .then(() => {
          this.isPlaying = true
          this._updateTaskbarState()
        })
        .catch((err) => logger.error('Failed to resume:', err))
    },

    togglePlay(): void {
      if (this.isPlaying) {
        this.pause()
      } else if (this.currentTrack) {
        const isAtEnd = this.duration > 0 && this.currentTime >= this.duration - 0.5
        if (isAtEnd) {
          this.play()
        } else {
          this.resume()
        }
      }
    },

    // --- 进度控制 ---

    // --- 播放结束 ---

    async _onEnded(): Promise<void> {
      if (this._isDestroyed || !this.currentTrack) return

      // 空播放列表守卫:停止播放,避免后续 list 模式分支 % 0 得到 NaN
      if (this.playlist.length === 0) {
        this.isPlaying = false
        invoke('pause_track').catch((err) =>
          logger.warn('pause on ended with empty playlist:', err),
        )
        return
      }

      const endedTrackPath = this.currentTrack.path
      await invoke('pause_track').catch((err) => logger.warn('pause on ended:', err))

      // 在 await 期间，用户可能已经手动切换了曲目，需要检测过时事件
      if (this._isDestroyed || !this.currentTrack || this.currentTrack.path !== endedTrackPath) {
        logger.debug('Ignore stale ended event because current track already changed')
        return
      }

      if (this.repeatMode === 'track') {
        await this.playTrack(this.currentTrack)
      } else if (this.isShuffle) {
        // shuffle 模式: 顺序播放且一轮已播完时停止,否则沿 _shuffleOrder 前进
        const isShuffleOrderValid = this._isShuffleOrderValid()
        const isAtEnd =
          isShuffleOrderValid && this._shufflePosition >= this._shuffleOrder.length - 1
        if (this.repeatMode === 'none' && isAtEnd) {
          this.isPlaying = false
          this.currentTime = this.duration
          invoke('pause_track').catch((err) => logger.warn('pause after shuffle ended:', err))
        } else {
          await this.nextTrack()
        }
      } else if (this.repeatMode === 'list') {
        const nextIndex = (this.currentTrackIndex + 1) % this.playlist.length
        await this.playTrack(this.playlist[nextIndex])
      } else if (this.currentTrackIndex < this.playlist.length - 1) {
        const nextIndex = this.currentTrackIndex + 1
        await this.playTrack(this.playlist[nextIndex])
      } else {
        this.isPlaying = false
        this.currentTime = this.duration
        invoke('pause_track').catch((err) => logger.warn('pause after playlist ended:', err))
      }
    },

    // --- 文件检查 ---

    async _checkFileExists(filePath: string): Promise<boolean> {
      if (!filePath) return false

      const cache = this._getFileExistsCache()
      const cached = cache.get(filePath)
      if (cached !== null) {
        return cached
      }

      try {
        const exists = await FileUtils.fileExists(filePath)
        cache.set(filePath, exists)
        return exists
      } catch (error) {
        logger.error('Error checking file:', error)
        cache.set(filePath, false)
        return false
      }
    },

    async resetPlayerState(clearPlaylist = true): Promise<void> {
      logger.info('Resetting player state')

      this.isPlaying = false

      this.currentTrack = null
      if (clearPlaylist) {
        this.playlist = []
      }
      this.currentTime = 0
      this.duration = 0
      this.lyrics = null
      this.currentLyricIndex = -1

      this._cacheManager?.destroy()
      this._cacheManager = null
      if (this._cacheAbortController) {
        this._cacheAbortController.abort()
        this._cacheAbortController = null
      }

      try {
        await invoke('pause_track')
      } catch (error) {
        logger.error('Error stopping backend playback:', error)
      }
    },

    async nextTrack(): Promise<void> {
      if (!this.currentTrack || this._isLoading) return
      // 空播放列表守卫:避免 (currentTrackIndex + 1) % 0 得到 NaN
      if (this.playlist.length === 0) return

      // 单曲循环由 _onEnded 处理,手动 next 走下一首
      let nextIndex: number
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          nextIndex = 0
        } else {
          const result = getNextShuffleIndex(
            this._shuffleOrder,
            this._shufflePosition,
            this.playlist.length,
            this.currentTrackIndex,
          )
          this._shuffleOrder = result.order
          this._shufflePosition = result.position
          nextIndex = result.index
          // 记入历史栈,用于 previousTrack 回溯
          this._shuffleHistory.push(this.currentTrackIndex)
        }
      } else {
        nextIndex = (this.currentTrackIndex + 1) % this.playlist.length
      }

      await this.playTrack(this.playlist[nextIndex])
    },

    async previousTrack(): Promise<void> {
      if (!this.currentTrack || this._isLoading) return
      // 空播放列表守卫:避免 playlist.length - 1 得到 -1,从而访问 playlist[-1] = undefined
      if (this.playlist.length === 0) return

      let prevIndex: number
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          prevIndex = 0
        } else {
          const result = getPreviousShuffleIndex(
            this._shuffleOrder,
            this._shufflePosition,
            this._shuffleHistory,
            this.playlist.length,
            this.currentTrackIndex,
          )
          this._shuffleOrder = result.order
          this._shufflePosition = result.position
          this._shuffleHistory = result.history
          prevIndex = result.index
        }
      } else {
        prevIndex = this.currentTrackIndex - 1
        if (prevIndex < 0) {
          prevIndex = this.playlist.length - 1
        }
      }

      await this.playTrack(this.playlist[prevIndex])
    },

    // --- 播放控制 ---

    seek(time: number): void {
      if (!this.currentTrack) return

      const wasPlaying = this.isPlaying
      const newTime = Math.max(0, Math.min(time, this.duration))

      invoke('seek_track', { time: newTime })
        .then(() => {
          this.currentTime = newTime
          if (!wasPlaying) {
            // 后端 seek 总是 play，如果之前是暂停状态需要重新暂停
            invoke('pause_track').catch((err) => logger.error('Failed to pause after seek:', err))
          }
        })
        .catch((err) => logger.error('Failed to seek:', err))
    },

    setVolume(volume: number): void {
      const newVolume = Math.max(0, Math.min(1, volume))
      this.volume = newVolume

      // 如果设置音量大于0，取消静音状态
      if (newVolume > 0 && this.isMuted) {
        this.isMuted = false
      }

      // 如果音量大于0，更新 previousVolume
      if (newVolume > 0) {
        this.previousVolume = newVolume
      }

      invoke('set_volume', { volume: this.isMuted ? 0 : newVolume })
        .then(() => {
          const configStore = useConfigStore()
          configStore.audio.volume = newVolume
          // 拖动音量条时每次 mousemove 都会走到这里：saveConfigNow 会对整个
          // config（含 lastSession 的播放队列快照）做深比较 + 深拷贝 + 写盘，
          // 大队列下足以卡住主线程、让滑块看起来掉帧。
          // 改用防抖保存（2s），停止拖动后仍会落盘，关应用前还有 flushPendingSave 兜底。
          configStore.saveConfig()
        })
        .catch((err) => logger.error('Failed to set volume:', err))
    },

    toggleMute(): void {
      if (this.isMuted) {
        // 取消静音，恢复之前的音量
        this.isMuted = false
        const volumeToRestore = this.previousVolume > 0 ? this.previousVolume : 0.5
        this.volume = volumeToRestore
        invoke('set_volume', { volume: volumeToRestore })
          .then(() => {
            const configStore = useConfigStore()
            configStore.audio.volume = volumeToRestore
            configStore.saveConfigNow()
          })
          .catch((err) => logger.error('Failed to unmute:', err))
      } else {
        // 静音，保存当前音量
        this.previousVolume = this.volume > 0 ? this.volume : this.previousVolume
        this.isMuted = true
        invoke('set_volume', { volume: 0 }).catch((err) => logger.error('Failed to mute:', err))
      }
    },

    toggleRepeat(): void {
      if (this.repeatMode === 'none') {
        this.repeatMode = 'list'
        this.isShuffle = false
      } else if (this.repeatMode === 'list') {
        this.repeatMode = 'track'
      } else {
        this.repeatMode = 'none'
      }
    },

    toggleShuffle(): void {
      this.isShuffle = !this.isShuffle
      if (this.isShuffle) {
        this.repeatMode = 'none'
        // 立即生成洗牌顺序,以当前曲目为起点
        const result = generateShuffleOrder(this.playlist.length, this.currentTrackIndex)
        this._shuffleOrder = result.order
        this._shufflePosition = result.position
        this._shuffleHistory = []
      } else {
        // 关闭 shuffle 时作废洗牌顺序,但保留历史栈以便恢复
        this._shuffleOrder = []
        this._shufflePosition = -1
      }
    },

    _regenerateShuffleOrder(): void {
      const result = generateShuffleOrder(this.playlist.length, this.currentTrackIndex)
      this._shuffleOrder = result.order
      this._shufflePosition = result.position
    },

    _isShuffleOrderValid(): boolean {
      return isShuffleOrderValid(this._shuffleOrder, this._shufflePosition, this.playlist.length)
    },

    // --- 歌词偏移 ---

    setLyricsOffset(offset: number): void {
      this.lyricsOffset = offset
    },

    adjustLyricsOffset(delta: number): void {
      this.lyricsOffset = Math.round((this.lyricsOffset + delta) * 10) / 10
    },

    resetLyricsOffset(): void {
      this.lyricsOffset = 0
    },

    // --- 播放列表管理 ---

    removeTrack(path: string): void {
      removeTrackFromPlaylist(this, path)
    },

    /**
     * 将曲目添加到当前播放列表的下一首位置
     */
    addTrackNext(track: Track): void {
      addTrackNextInPlaylist(this, track)
    },

    /**
     * 将多个曲目添加到当前播放列表的下一首位置
     */
    addTracksNext(tracks: Track[]): void {
      addTracksNextInPlaylist(this, tracks)
    },

    // --- 数据加载 ---

    loadPlaylist(playlist: Track[]): void {
      // 取消上一次正在进行的缓存任务
      if (this._cacheAbortController) {
        this._cacheAbortController.abort()
      }

      this.playlist = playlist
      // playlist 变化,作废 shuffle 顺序与历史栈
      this._shuffleOrder = []
      this._shufflePosition = -1
      this._shuffleHistory = []
      if (playlist && playlist.length > 0) {
        const firstTrack = playlist[0]
        this.currentTrack = firstTrack
        this.duration = firstTrack.duration || 0
        this.audioInfo = {
          bitrate: firstTrack.bitrate || null,
          sampleRate: firstTrack.sampleRate || null,
          channels: firstTrack.channels || null,
          bitDepth: firstTrack.bitDepth || null,
          format: firstTrack.format || null,
        }
      } else {
        this.currentTrack = null
        this.isPlaying = false
        this.currentTime = 0
        this.duration = 0
      }

      this._cachePlaylistMetadata(playlist)
      this._loadPlaylistCovers(playlist)
    },

    /** 批量缓存播放列表元数据 (实现见 playerMediaCache.ts) */
    async _cachePlaylistMetadata(playlist: Track[]): Promise<void> {
      await cachePlaylistMetadata(this, playlist)
    },

    /** 记录一条封面更新并通知列表组件（供 PlaylistView 之外的封面加载路径复用） */
    recordCoverUpdate(path: string, coverPath: string): void {
      recordCoverUpdateToMap(path, coverPath)
      this.playlistCoverVersion++
    },

    /** 取走并清空自上次调用以来累计的封面更新,供 PlaylistView 增量应用 */
    takeCoverUpdates(): Map<string, string> {
      return takeCoverUpdatesFromMap()
    },

    /** 批量加载播放列表封面 (实现见 playerMediaCache.ts) */
    async _loadPlaylistCovers(playlist: Track[]): Promise<void> {
      await loadPlaylistCovers(this, playlist)
    },

    async loadLyrics(trackPath: string, requestId?: number): Promise<void> {
      const lyricsRequestId = requestId ?? ++this._lyricsRequestId
      if (requestId == null) {
        this._lyricsRequestId = lyricsRequestId
      }

      try {
        const lyricsPath = await FileUtils.findLyricsFile(trackPath)
        if (
          this._isDestroyed ||
          this._activePlayRequestId !== lyricsRequestId ||
          this.currentTrack?.path !== trackPath
        ) {
          return
        }

        if (lyricsPath) {
          const lyricsContent = await FileUtils.readFile(lyricsPath)
          const format = FileUtils.getFileExtension(lyricsPath) as 'lrc' | 'ass' | 'srt'
          const parsedLyrics = await LyricsParser.parseAsync(lyricsContent, format)

          if (
            this._isDestroyed ||
            this._activePlayRequestId !== lyricsRequestId ||
            this.currentTrack?.path !== trackPath
          ) {
            return
          }

          // markRaw: 歌词数据只整体替换、不修改内部字段,
          // 无需深度响应式代理 (大歌词可达数百行,深度代理开销显著)
          this.lyrics = markRaw(parsedLyrics)
        } else if (this.currentTrack?.path === trackPath) {
          this.lyrics = null
        }
      } catch (error) {
        logger.debug('No lyrics found or failed to load:', error)
        if (
          this.currentTrack?.path === trackPath &&
          this._activePlayRequestId === lyricsRequestId
        ) {
          this.lyrics = null
        }
      }
    },

    // --- 清理 ---
    async cleanup(): Promise<void> {
      // 关闭前同步保存 last_session (await 确保 IPC 调用完成)
      if (this.currentTrack) {
        try {
          await this._saveLastSessionNow()
        } catch (err) {
          logger.debug('Failed to save last session on cleanup:', err)
        }
      }
      this._isDestroyed = true
      this._isInitializing = false
      this._initPromise = null
      this._isLoading = false
      this._activePlayRequestId = ++this._playRequestId
      this._lyricsRequestId = this._activePlayRequestId
      this._isSwitchingDevice = false
      this._lastDeviceSwitchTarget = null

      this._stopCleanupTask()
      this._cacheManager?.destroy()
      this._cacheManager = null

      if (this._trackEndedUnlisten) {
        this._trackEndedUnlisten()
        this._trackEndedUnlisten = null
      }

      if (this._positionUnlisten) {
        this._positionUnlisten()
        this._positionUnlisten = null
      }

      // 清理任务栏事件监听
      if (this._taskbarPreviousUnlisten) {
        this._taskbarPreviousUnlisten()
        this._taskbarPreviousUnlisten = null
      }
      if (this._taskbarPlayPauseUnlisten) {
        this._taskbarPlayPauseUnlisten()
        this._taskbarPlayPauseUnlisten = null
      }
      if (this._taskbarNextUnlisten) {
        this._taskbarNextUnlisten()
        this._taskbarNextUnlisten = null
      }

      // 清理设备事件监听
      if (this._deviceRemovedUnlisten) {
        this._deviceRemovedUnlisten()
        this._deviceRemovedUnlisten = null
      }
      if (this._deviceSwitchRequiredUnlisten) {
        this._deviceSwitchRequiredUnlisten()
        this._deviceSwitchRequiredUnlisten = null
      }
      if (this._noDeviceAvailableUnlisten) {
        this._noDeviceAvailableUnlisten()
        this._noDeviceAvailableUnlisten = null
      }
      if (this._deviceDefaultChangedUnlisten) {
        this._deviceDefaultChangedUnlisten()
        this._deviceDefaultChangedUnlisten = null
      }

      // 注销全局媒体键快捷方式
      await unregisterGlobalShortcuts()

      try {
        invoke('pause_track').catch((err) => logger.warn('pause during cleanup:', err))
        // 设置任务栏为停止状态
        invoke('set_taskbar_stopped').catch((e) =>
          errorHandler.handle(e, { severity: ErrorSeverity.LOW, showToUser: false }),
        )
      } catch {
        // 忽略错误
      }

      if (this._cacheAbortController) {
        this._cacheAbortController.abort()
        this._cacheAbortController = null
      }

      if (this._nextTrackTimeoutId) {
        clearTimeout(this._nextTrackTimeoutId)
        this._nextTrackTimeoutId = null
      }

      logger.info('Player store cleaned up')
    },
  },
})
