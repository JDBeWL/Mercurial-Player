import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { type UnlistenFn } from '@tauri-apps/api/event'
import FileUtils from '../utils/fileUtils'
import LyricsParser from '../utils/lyricsParser'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity } from '../utils/errorHandler'
import { classifyAudioInvokeError } from '../utils/audioErrorClassifier'
import { LRUCache } from '@/utils/lruCache'
import { useConfigStore } from './config'
import { useMusicLibraryStore } from './musicLibrary'
import {
  setupTrackEndedListener,
  setupPositionListener,
  setupTaskbarListeners,
  setupGlobalShortcuts,
  setupDeviceListeners,
  unregisterGlobalShortcuts,
} from './playerListeners'
import type { Track, AudioInfo, LyricLine, RepeatMode, ResumeResult, TrackSnapshot } from '@/types'

interface TrackMetadata {
  title: string
  artist: string
  album: string
  duration: number
  coverPath?: string
  bitrate: number | null
  sampleRate: number | null
  channels: number | null
  bitDepth: number | null
  format: string | null
}

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
  _statusPollId: ReturnType<typeof setTimeout> | null
  lastTrackIndex: number
  _fileExistsCache: LRUCache<boolean> | null
  _metadataCache: LRUCache<TrackMetadata> | null
  _cleanupTimerId: ReturnType<typeof setInterval> | null
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
}

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
    _statusPollId: null,
    lastTrackIndex: -1,

    // 缓存 - 使用 LRU 缓存，限制大小
    _fileExistsCache: null,
    _metadataCache: null,

    // 清理定时器
    _cleanupTimerId: null,

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

    _getFileExistsCache(): LRUCache<boolean> {
      if (!this._fileExistsCache) {
        this._fileExistsCache = new LRUCache<boolean>(200, 30000) as any
      }
      return this._fileExistsCache as LRUCache<boolean>
    },

    _getMetadataCache(): LRUCache<TrackMetadata> {
      if (!this._metadataCache) {
        this._metadataCache = new LRUCache<TrackMetadata>(500, 300000) as any
      }
      return this._metadataCache as LRUCache<TrackMetadata>
    },

    _startCleanupTask(): void {
      if (this._cleanupTimerId) return

      this._cleanupTimerId = setInterval(() => {
        this._cleanupCaches()
      }, 300000)
    },

    _stopCleanupTask(): void {
      if (this._cleanupTimerId) {
        clearInterval(this._cleanupTimerId)
        this._cleanupTimerId = null
      }
    },

    async _cleanupCaches(): Promise<void> {
      const CHUNK_SIZE = 50

      if (this._fileExistsCache) {
        const keys = Array.from(this._fileExistsCache.keys())
        for (let i = 0; i < keys.length; i++) {
          this._fileExistsCache.get(keys[i])
          if (i > 0 && i % CHUNK_SIZE === 0) {
            await new Promise((resolve) => setTimeout(resolve, 0))
          }
        }
      }
      if (this._metadataCache) {
        const keys = Array.from(this._metadataCache.keys())
        for (let i = 0; i < keys.length; i++) {
          this._metadataCache.get(keys[i])
          if (i > 0 && i % CHUNK_SIZE === 0) {
            await new Promise((resolve) => setTimeout(resolve, 0))
          }
        }
      }
      logger.debug('Cache cleanup completed')
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
     *
     * 不在 playback-position 事件中写入,避免播放期间频繁写盘。
     * 仅在以下场景触发:
     * - pause():暂停时立即保存
     * - playTrack():切曲前保存上一曲的最后位置
     * - cleanup():关闭窗口前 await 保存
     * 风险:程序崩溃/断电时丢失自上次关键节点以来的进度。
     */
    async _saveLastSessionNow(): Promise<void> {
      if (!this.currentTrack || this._isDestroyed) return
      const track = this.currentTrack
      // 从 musicLibraryStore 获取当前播放列表名 (用于启动恢复)
      let playlistName: string | null = null
      let trackIndexInPlaylist: number | null = null
      try {
        const musicLibraryStore = useMusicLibraryStore()
        if (musicLibraryStore.currentPlaylist) {
          playlistName = musicLibraryStore.currentPlaylist.name
          const idx = musicLibraryStore.currentPlaylist.files.findIndex(
            (f) => f.path === track.path,
          )
          if (idx >= 0) trackIndexInPlaylist = idx
        }
      } catch (err) {
        logger.debug('Failed to get current playlist name:', err)
      }
      // 提取 player.playlist 的元数据快照 (不依赖 musicLibrary 缓存)
      // 这样启动恢复时即使 musicLibrary 还没加载,也能直接重建 player.playlist
      const playlistTracks: TrackSnapshot[] = this.playlist.map((t) => ({
        path: t.path,
        title: t.title ?? null,
        artist: t.artist ?? null,
        album: t.album ?? null,
        duration: t.duration ?? null,
        bitrate: t.bitrate ?? null,
        sampleRate: t.sampleRate ?? null,
        channels: t.channels ?? null,
        bitDepth: t.bitDepth ?? null,
        format: t.format ?? null,
      }))
      try {
        await invoke('save_last_session', {
          trackPath: track.path,
          trackTitle: track.title || track.displayTitle || track.name || '',
          trackArtist: track.artist || track.displayArtist || '',
          durationSecs: this.duration || 0,
          positionSecs: this.currentTime,
          playlistName,
          trackIndexInPlaylist,
          playlistTracks,
        })
      } catch (err) {
        logger.debug('Failed to save last session:', err)
      }
    },

    /**
     * 启动时调用 - 尝试恢复上次播放会话
     *
     * 行为:
     * - resumed=true: 后端已加载文件并暂停在 position,前端设置 UI 状态
     *   (currentTrack/duration/currentTime/audioInfo/playlist),isPlaying=false 保持暂停
     * - resumed=false 且 status='not_found': 文件不存在,从播放列表移除该路径
     * - 其他 false 状态: 静默忽略,无需 UI 反馈
     */
    async resumeLastSession(): Promise<ResumeResult | null> {
      try {
        const result = await invoke<ResumeResult>('resume_last_session')
        if (result.resumed && result.trackPath) {
          const trackPath = result.trackPath

          // 1. 用返回的 playlistTracks 直接构造 player.playlist
          //    不依赖 musicLibrary 缓存是否加载,保证恢复后播放列表完整
          const playlistTracks = result.playlistTracks ?? []
          const playlist: Track[] = playlistTracks.map((s) => ({
            path: s.path,
            title: s.title ?? undefined,
            artist: s.artist ?? undefined,
            album: s.album ?? undefined,
            displayTitle: s.title ?? undefined,
            displayArtist: s.artist ?? undefined,
            duration: s.duration ?? undefined,
            bitrate: s.bitrate ?? null,
            sampleRate: s.sampleRate ?? null,
            channels: s.channels ?? null,
            bitDepth: s.bitDepth ?? null,
            format: s.format ?? null,
          }))
          this.playlist = playlist

          // 2. 在 playlist 中查找当前曲目 (含完整元数据: bitrate/sampleRate 等)
          let matchedTrack: Track | null = playlist.find((t) => t.path === trackPath) ?? null

          // 3. 如果 playlistTracks 为空或没找到,用 lastSession 快照构造一个最小 Track
          if (!matchedTrack) {
            matchedTrack = {
              path: trackPath,
              title: result.trackTitle || undefined,
              artist: result.trackArtist || undefined,
              displayTitle: result.trackTitle || undefined,
              displayArtist: result.trackArtist || undefined,
              duration: result.durationSecs ?? undefined,
            }
            // 当前曲目不在 playlistTracks 中,把它加到 playlist 末尾
            this.playlist.push(matchedTrack)
          }

          // 4. 尝试同步 musicLibrary 的 currentPlaylist (让 UI 高亮,但不依赖它)
          if (result.playlistName) {
            try {
              const musicLibraryStore = useMusicLibraryStore()
              if (musicLibraryStore.playlists.length === 0) {
                await musicLibraryStore.loadPlaylistsFromCache()
              }
              const mlPlaylist = musicLibraryStore.playlists.find(
                (p) => p.name === result.playlistName,
              )
              if (mlPlaylist) {
                musicLibraryStore.selectPlaylist(mlPlaylist)
                // 如果 musicLibrary 中的曲目有更完整的元数据 (比如 coverPath 已加载),用它
                const mlTrack = mlPlaylist.files.find((t) => t.path === trackPath)
                if (mlTrack && mlTrack.bitrate && !matchedTrack.bitrate) {
                  matchedTrack = mlTrack
                }
              }
            } catch (err) {
              logger.warn('Failed to sync musicLibrary playlist for resume:', err)
            }
          }

          // 5. 触发元数据缓存和封面预加载 (与 loadPlaylist 一致)
          if (this._cacheAbortController) {
            this._cacheAbortController.abort()
          }
          this._cachePlaylistMetadata(this.playlist)
          this._loadPlaylistCovers(this.playlist)

          // 6. 设置播放状态 (currentTrack/audioInfo/duration/currentTime)
          this.currentTrack = matchedTrack
          this.duration = matchedTrack.duration ?? result.durationSecs ?? 0
          this.currentTime = result.positionSecs ?? 0
          // 从 matchedTrack 提取完整音频元数据 (bitrate/sampleRate/channels/bitDepth/format)
          this.audioInfo = {
            bitrate: matchedTrack.bitrate || null,
            sampleRate: matchedTrack.sampleRate || null,
            channels: matchedTrack.channels || null,
            bitDepth: matchedTrack.bitDepth || null,
            format: matchedTrack.format || null,
          }
          // 保持暂停状态 - 用户主动点播放才会开始
          this.isPlaying = false
          this._updateTaskbarState()
          logger.info(
            `Resumed last session (paused): ${trackPath} @ ${result.positionSecs}s (${result.status}), playlist=${playlist.length} tracks`,
          )

          // 7. 加载当前曲目封面 (异步,不阻塞恢复)
          invoke<string | null>('get_track_cover_path', { path: trackPath })
            .then((coverPath) => {
              // 守卫:应用关闭后不再修改已销毁的 store state
              if (this._isDestroyed) return
              if (this.currentTrack && this.currentTrack.path === trackPath && coverPath) {
                this.currentTrack.coverPath = coverPath
              }
            })
            .catch((err) => logger.debug('Failed to load cover for resumed track:', err))
        } else if (result.status === 'not_found' && result.trackPath) {
          // 静默处理:从当前播放列表移除该文件 (用户选择)
          const idx = this.playlist.findIndex((t) => t.path === result.trackPath)
          if (idx >= 0) {
            this.playlist.splice(idx, 1)
            logger.info(`Removed missing track from playlist: ${result.trackPath}`)
          }
        }
        return result
      } catch (err) {
        logger.error('Failed to resume last session:', err)
        return null
      }
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
          this.stopStatusPolling()
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
              ? `音频设备已断开，切换到备用设备失败: ${deviceName}`
              : `切换到新设备失败: ${deviceName}`,
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
      this.stopStatusPolling()

      const metadataCache = this._getMetadataCache()
      let metadata = metadataCache.get(resolvedPath) ?? metadataCache.get(track.path)
      if (!metadata) {
        metadata = {
          title: track.title || FileUtils.getFileName(resolvedPath),
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
          timeoutId = setTimeout(() => reject(new Error('播放超时')), 5000)
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
        this.startStatusPolling()
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
          }, 100)
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
          this.stopStatusPolling()
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
          this.startStatusPolling()
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

    startStatusPolling(): void {
      // 不再需要轮询 - 使用 track-ended 事件代替
      // 保留方法以保持 API 兼容性
    },

    stopStatusPolling(): void {
      // 保留方法以保持 API 兼容性
      if (this._statusPollId) {
        clearTimeout(this._statusPollId)
        this._statusPollId = null
      }
    },

    // --- 播放结束 ---

    async _onEnded(): Promise<void> {
      if (this._isDestroyed || !this.currentTrack) return

      // 空播放列表守卫:停止播放,避免后续 list 模式分支 % 0 得到 NaN
      if (this.playlist.length === 0) {
        this.isPlaying = false
        this.stopStatusPolling()
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
          this.stopStatusPolling()
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
        this.stopStatusPolling()
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
      this.stopStatusPolling()

      this.currentTrack = null
      if (clearPlaylist) {
        this.playlist = []
      }
      this.currentTime = 0
      this.duration = 0
      this.lyrics = null
      this.currentLyricIndex = -1

      if (this._fileExistsCache) {
        this._fileExistsCache.clear()
      }
      if (this._metadataCache) {
        this._metadataCache.clear()
      }
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
          // 懒生成:第一次或顺序失效时重新洗牌
          if (!this._isShuffleOrderValid()) {
            this._regenerateShuffleOrder()
          }
          // 走到末尾:重新洗牌继续 (手动触发时不应停止,自动结束的停止逻辑由 _onEnded 处理)
          if (this._shufflePosition >= this._shuffleOrder.length - 1) {
            this._regenerateShuffleOrder()
            nextIndex = this._shuffleOrder[0]
            this._shufflePosition = 0
          } else {
            this._shufflePosition++
            nextIndex = this._shuffleOrder[this._shufflePosition]
          }
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
        } else if (this._shuffleHistory.length > 0) {
          // 优先从历史栈弹出,真正回到上一首
          prevIndex = this._shuffleHistory.pop()!
          // 同步调整 _shufflePosition
          if (this._shufflePosition > 0) this._shufflePosition--
        } else {
          // 历史栈空:走到洗牌序列上一首,如果已在起点则重新洗牌取最后一首
          if (!this._isShuffleOrderValid()) {
            this._regenerateShuffleOrder()
          }
          if (this._shufflePosition > 0) {
            this._shufflePosition--
            prevIndex = this._shuffleOrder[this._shufflePosition]
          } else {
            // 在起点之前:回绕到序列末尾
            this._shufflePosition = this._shuffleOrder.length - 1
            prevIndex = this._shuffleOrder[this._shufflePosition]
          }
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
          configStore.saveConfigNow()
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
        this._regenerateShuffleOrder()
        this._shuffleHistory = []
      } else {
        // 关闭 shuffle 时作废洗牌顺序,但保留历史栈以便恢复
        this._shuffleOrder = []
        this._shufflePosition = -1
      }
    },

    /**
     * 用 Knuth (Fisher-Yates-Knuth) 算法生成洗牌顺序
     * 算法: 从后往前遍历 [n-1..1], 每次从 [0..i] 中随机取一个与 i 交换
     * 时间复杂度 O(n), 空间 O(n), 保证 n! 种排列等概率出现
     *
     * 以当前曲目为起点: 把当前 index 放到序列第 0 位,只对剩余 n-1 首洗牌
     */
    _regenerateShuffleOrder(): void {
      const n = this.playlist.length
      if (n === 0) {
        this._shuffleOrder = []
        this._shufflePosition = -1
        return
      }

      // 1. 生成 [0, 1, ..., n-1]
      const order = Array.from({ length: n }, (_, i) => i)

      // 2. Knuth shuffle: for i = n-1 downto 1, swap(order[i], order[rand(0..i)])
      for (let i = n - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        const tmp = order[i]
        order[i] = order[j]
        order[j] = tmp
      }

      // 3. 以当前曲目为起点:把 currentTrackIndex 移到第 0 位
      const cur = this.currentTrackIndex
      if (cur >= 0 && cur < n) {
        const curPos = order.indexOf(cur)
        if (curPos > 0) {
          // 把当前位置与第 0 位交换
          const tmp = order[0]
          order[0] = order[curPos]
          order[curPos] = tmp
        }
      }

      this._shuffleOrder = order
      this._shufflePosition = 0
    },

    /**
     * 校验洗牌顺序是否仍然有效
     * 失效条件: 序列长度与当前 playlist 不一致 (playlist 变化/重置)
     */
    _isShuffleOrderValid(): boolean {
      return (
        this._shuffleOrder.length === this.playlist.length &&
        this._shuffleOrder.length > 0 &&
        this._shufflePosition >= 0
      )
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
      const index = this.playlist.findIndex((t) => t.path === path)
      if (index === -1) return

      // 先从播放列表中移除
      this.playlist.splice(index, 1)

      // 如果播放列表为空，重置状态
      if (this.playlist.length === 0) {
        this.resetPlayerState(false)
        return
      }

      // 如果删除的是当前播放的歌曲
      if (this.currentTrack?.path === path) {
        const nextIndex = index >= this.playlist.length ? 0 : index
        const wasPlaying = this.isPlaying

        // 暂停当前播放，避免音频状态不一致
        if (wasPlaying) {
          invoke('pause_track').catch((err) => logger.warn('pause before remove:', err))
        }

        this.playTrack(this.playlist[nextIndex]).then(() => {
          if (!wasPlaying) {
            this.pause()
          }
        })
      } else {
        // 如果删除的不是当前播放的歌曲，但删除了当前歌曲前面的歌曲，
        // 我们不需要更新 currentTrack，但需要处理 vue 响应式带来的潜在问题
        // 虽然在目前的设计中 currentTrackIndex 是一个 getter，所以它会自动更新

        // 同步更新 shuffle 顺序，避免 _shuffleOrder.length 与 playlist.length 不一致
        // 导致 _isShuffleOrderValid() 返回 false，进而造成 shuffle 模式下单曲列表无限重播
        if (this._shuffleOrder.length > 0) {
          // 被删 index 在 _shuffleOrder 中的位置 (用于后续校正 _shufflePosition)
          const removedPos = this._shuffleOrder.indexOf(index)
          // 移除被删 index,并将大于该 index 的值减 1 (保持索引一致性)
          this._shuffleOrder = this._shuffleOrder
            .filter((i) => i !== index)
            .map((i) => (i > index ? i - 1 : i))
          // 若被删条目位于当前播放位置之前,当前条目前移一位,_shufflePosition 需同步减 1
          if (removedPos !== -1 && removedPos < this._shufflePosition) {
            this._shufflePosition--
          }
        }
      }
    },

    /**
     * 将曲目添加到当前播放列表的下一首位置
     */
    addTrackNext(track: Track): void {
      if (!track) return

      const currentIndex = this.currentTrackIndex

      // 如果没有当前曲目或播放列表为空，直接添加到开头
      if (currentIndex === -1 || this.playlist.length === 0) {
        this.playlist.unshift(track)
        logger.info('Added track to beginning of playlist:', track.path)
        return
      }

      // 检查曲目是否已经在播放列表中
      const existingIndex = this.playlist.findIndex((t) => t.path === track.path)

      if (existingIndex !== -1) {
        // 如果曲目已存在，先移除它
        this.playlist.splice(existingIndex, 1)

        // 移除后当前曲目的实际索引可能已偏移，需要重新计算
        const adjustedCurrentIndex = existingIndex < currentIndex ? currentIndex - 1 : currentIndex
        // 插入到当前曲目之后
        const adjustedIndex = adjustedCurrentIndex + 1
        this.playlist.splice(adjustedIndex, 0, track)
        logger.info('Moved existing track to next position:', track.path)
      } else {
        // 如果曲目不存在，直接插入到当前曲目后面
        this.playlist.splice(currentIndex + 1, 0, track)
        logger.info('Added new track to next position:', track.path)
      }
    },

    /**
     * 将多个曲目添加到当前播放列表的下一首位置
     */
    addTracksNext(tracks: Track[]): void {
      if (!tracks || tracks.length === 0) return

      const currentIndex = this.currentTrackIndex

      // 如果没有当前曲目或播放列表为空，直接添加到开头
      if (currentIndex === -1 || this.playlist.length === 0) {
        this.playlist.unshift(...tracks)
        logger.info(`Added ${tracks.length} tracks to beginning of playlist`)
        return
      }

      // 过滤掉已存在的曲目并记录它们的位置
      const existingPaths = new Set(this.playlist.map((t) => t.path))
      const newTracks = tracks.filter((t) => !existingPaths.has(t.path))

      // 插入到当前曲目后面
      this.playlist.splice(currentIndex + 1, 0, ...newTracks)
      logger.info(`Added ${newTracks.length} new tracks to next position`)
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
        this.stopStatusPolling()
      }

      this._cachePlaylistMetadata(playlist)
      this._loadPlaylistCovers(playlist)
    },

    async _cachePlaylistMetadata(playlist: Track[]): Promise<void> {
      if (!playlist || playlist.length === 0) return

      // 创建新的 AbortController，使此次缓存任务可被后续调用取消
      const abortController = new AbortController()

      this._cacheAbortController = abortController as any

      const cache = this._getMetadataCache()
      const CHUNK_SIZE = 200
      let cached = 0

      for (let i = 0; i < playlist.length; i++) {
        // 检查是否已被取消
        if (abortController.signal.aborted) {
          logger.debug(`Metadata caching aborted after ${cached} tracks`)
          return
        }

        const track = playlist[i]
        if (!track.path || cache.has(track.path)) continue

        cache.set(track.path, {
          title:
            track.displayTitle || track.title || track.name || FileUtils.getFileName(track.path),
          artist: track.displayArtist || track.artist || '',
          album: track.album || '',
          duration: track.duration || 0,
          bitrate: track.bitrate || null,
          sampleRate: track.sampleRate || null,
          channels: track.channels || null,
          bitDepth: track.bitDepth || null,
          format: track.format || null,
        })
        cached++

        if (cached > 0 && cached % CHUNK_SIZE === 0) {
          await new Promise((resolve) => setTimeout(resolve, 0))
        }
      }

      logger.debug(`Cached metadata for ${cached} tracks`)
    },

    async _loadPlaylistCovers(playlist: Track[]): Promise<void> {
      if (!playlist || playlist.length === 0) return

      const metadataCache = this._getMetadataCache()

      // 注意:循环内逐个修改 track.coverPath 会触发 Pinia 响应式更新。
      // 保持该写法的原因:播放列表项已通过 v-memo 优化重渲染,实际重渲染开销很小;
      // 且封面加载本身是 IO 密集型,瓶颈不在响应式触发上。
      // 如需进一步优化,可在所有封面加载完成后统一刷新 (例如先收集到本地 Map 再批量赋值),
      // 但收益有限、改动风险较高,故暂不调整。

      // 批量加载封面路径，每次处理 10 首歌曲
      const BATCH_SIZE = 10
      for (let i = 0; i < playlist.length; i += BATCH_SIZE) {
        const batch = playlist.slice(i, i + BATCH_SIZE)

        // 并行加载这一批的封面
        await Promise.all(
          batch.map(async (track) => {
            if (!track.coverPath) {
              try {
                const coverPath = await invoke<string | null>('get_track_cover_path', {
                  path: track.path,
                })
                if (coverPath) {
                  track.coverPath = coverPath
                  // 同步 currentTrack: playTrack 会创建 resolvedTrack 浅拷贝,
                  // 导致 currentTrack 与 playlist 内对象脱钩,
                  // 主界面/MiniPlayer 封面基于 currentTrack.coverPath,
                  // 不同步会导致切歌时封面丢失
                  if (this.currentTrack?.path === track.path) {
                    this.currentTrack.coverPath = coverPath
                  }
                  // 同时更新元数据缓存中的封面路径
                  const cachedMetadata = metadataCache.get(track.path)
                  if (cachedMetadata) {
                    cachedMetadata.coverPath = coverPath
                    metadataCache.set(track.path, cachedMetadata)
                  }
                }
              } catch (err) {
                logger.debug(`Failed to load cover for ${track.path}:`, err)
              }
            }
          }),
        )

        // 让出主线程，避免阻塞 UI
        if (i + BATCH_SIZE < playlist.length) {
          await new Promise((resolve) => setTimeout(resolve, 0))
        }
      }

      logger.debug(`Loaded covers for playlist`)
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

          this.lyrics = parsedLyrics
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

      this.stopStatusPolling()
      this._stopCleanupTask()

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
        invoke('set_taskbar_stopped').catch(() => {})
      } catch {
        // 忽略错误
      }

      if (this._fileExistsCache) {
        this._fileExistsCache.clear()
        this._fileExistsCache = null
      }
      if (this._metadataCache) {
        this._metadataCache.clear()
        this._metadataCache = null
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
