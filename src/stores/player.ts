import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import FileUtils from '../utils/fileUtils'
import LyricsParser from '../utils/lyricsParser'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity } from '../utils/errorHandler'
import { classifyAudioInvokeError } from '../utils/audioErrorClassifier'
import { useConfigStore } from './config'
import type { Track, AudioInfo, LyricLine, RepeatMode, CacheItem } from '@/types'

/**
 * 简单的LRU缓存实现
 */
class LRUCache<T> {
  private maxSize: number
  private ttl: number
  private cache: Map<string, CacheItem<T>>

  constructor(maxSize: number = 100, ttl: number = 60000) {
    this.maxSize = maxSize
    this.ttl = ttl
    this.cache = new Map()
  }

  get(key: string): T | null {
    const item = this.cache.get(key)
    if (!item) return null

    // 检查是否过期
    if (Date.now() - item.timestamp > this.ttl) {
      this.cache.delete(key)
      return null
    }

    // 移到末尾（最近使用）
    this.cache.delete(key)
    this.cache.set(key, item)
    return item.value
  }

  set(key: string, value: T): void {
    // 如果已存在，先删除
    if (this.cache.has(key)) {
      this.cache.delete(key)
    }

    // 如果超过最大大小，删除最旧的
    while (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value
      if (firstKey) this.cache.delete(firstKey)
    }

    this.cache.set(key, {
      value,
      timestamp: Date.now()
    })
  }

  has(key: string): boolean {
    return this.get(key) !== null
  }

  delete(key: string): void {
    this.cache.delete(key)
  }

  clear(): void {
    this.cache.clear()
  }

  get size(): number {
    return this.cache.size
  }

  keys(): IterableIterator<string> {
    return this.cache.keys()
  }
}

interface TrackMetadata {
  title: string
  artist: string
  album: string
  duration: number
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
  _trackEndedUnlisten: UnlistenFn | null
  _positionUnlisten: UnlistenFn | null
  _taskbarPreviousUnlisten: UnlistenFn | null
  _taskbarPlayPauseUnlisten: UnlistenFn | null
  _taskbarNextUnlisten: UnlistenFn | null
  _deviceRemovedUnlisten: UnlistenFn | null
  _deviceSwitchRequiredUnlisten: UnlistenFn | null
  _noDeviceAvailableUnlisten: UnlistenFn | null
  _deviceDefaultChangedUnlisten: UnlistenFn | null
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
      format: null
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
  }),

  getters: {
    currentTrackIndex: (state): number => {
      if (!state.currentTrack || state.playlist.length === 0) return -1
      return state.playlist.findIndex(track => track.path === state.currentTrack!.path)
    },
    hasNextTrack: (state): boolean => {
      if (!state.currentTrack) return false
      return state.playlist.length > 0
    },
    hasPreviousTrack: (state): boolean => {
      if (!state.currentTrack) return false
      return state.playlist.length > 0
    },
    currentLyric: (state): LyricLine | null => {
      if (!state.lyrics || state.currentLyricIndex < 0 || state.currentLyricIndex >= state.lyrics.length) {
        return null
      }
      return state.lyrics[state.currentLyricIndex]
    }
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
            await new Promise(resolve => setTimeout(resolve, 0))
          }
        }
      }
      if (this._metadataCache) {
        const keys = Array.from(this._metadataCache.keys())
        for (let i = 0; i < keys.length; i++) {
          this._metadataCache.get(keys[i])
          if (i > 0 && i % CHUNK_SIZE === 0) {
            await new Promise(resolve => setTimeout(resolve, 0))
          }
        }
      }
      logger.debug('Cache cleanup completed')
    },

    // --- 初始化 ---
    async initAudio(): Promise<void> {
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

      this._setupTrackEndedListener()
      this._setupPositionListener()
      this._setupTaskbarListeners()
      this._setupDeviceListeners()
      this._startCleanupTask()

      logger.info('Player store initialized.')
    },

    async _setupTrackEndedListener(): Promise<void> {
      try {
        this._trackEndedUnlisten = await listen('track-ended', () => {
          if (this._isDestroyed) return
          logger.debug('Received track-ended event')
          this._onEnded()
        })
      } catch (err) {
        logger.error('Failed to setup track-ended listener:', err)
      }
    },

    async _setupPositionListener(): Promise<void> {
      try {
        this._positionUnlisten = await listen<{ position: number }>('playback-position', (event) => {
          if (this._isDestroyed || !this.isPlaying) return
          const position = event.payload?.position
          if (typeof position === 'number' && position >= 0) {
            this.currentTime = position
          }
        })
      } catch (err) {
        logger.error('Failed to setup position listener:', err)
      }
    },

    async _setupTaskbarListeners(): Promise<void> {
      try {
        // 监听任务栏上一首按钮
        this._taskbarPreviousUnlisten = await listen('taskbar-previous', () => {
          if (this._isDestroyed) return
          logger.debug('Taskbar: Previous button clicked')
          this.previousTrack()
        })

        // 监听任务栏播放/暂停按钮
        this._taskbarPlayPauseUnlisten = await listen('taskbar-play-pause', () => {
          if (this._isDestroyed) return
          logger.debug('Taskbar: Play/Pause button clicked')
          this.togglePlay()
        })

        // 监听任务栏下一首按钮
        this._taskbarNextUnlisten = await listen('taskbar-next', () => {
          if (this._isDestroyed) return
          logger.debug('Taskbar: Next button clicked')
          this.nextTrack()
        })

        logger.info('Taskbar listeners setup complete')
      } catch (err) {
        logger.error('Failed to setup taskbar listeners:', err)
      }
    },

    async _setupDeviceListeners(): Promise<void> {
      try {
        // 监听设备移除事件
        this._deviceRemovedUnlisten = await listen<{ eventType: string; deviceName: string | null }>('device-removed', (event) => {
          if (this._isDestroyed) return
          const deviceName = event.payload?.deviceName
          logger.warn(`Audio device removed: ${deviceName}`)
        })

        // 监听设备切换请求事件
        this._deviceSwitchRequiredUnlisten = await listen<{ eventType: string; deviceName: string | null }>('device-switch-required', async (event) => {
          if (this._isDestroyed) return
          const deviceName = event.payload?.deviceName
          if (!deviceName) return

          logger.info(`Switching to fallback device: ${deviceName}`)
          
          try {
            const currentTime = this.currentTime
            await invoke('set_audio_device', { 
              deviceName, 
              currentTime: this.isPlaying ? currentTime : null 
            })
            
            logger.info(`Successfully switched to fallback device: ${deviceName}`)
            
            // 显示成功通知
            errorHandler.handle(
              new Error(`Device switched to ${deviceName}`),
              {
                type: ErrorType.AUDIO_DEVICE_ERROR,
                severity: ErrorSeverity.LOW,
                context: { deviceName, action: 'switch-fallback-success' },
                showToUser: true,
                userMessage: `音频设备已断开，已自动切换到: ${deviceName}`
              }
            )
          } catch (err) {
            errorHandler.handle(
              err instanceof Error ? err : new Error(String(err)),
              {
                type: ErrorType.AUDIO_DEVICE_ERROR,
                severity: ErrorSeverity.HIGH,
                context: { deviceName, action: 'switch-fallback' },
                showToUser: true,
                userMessage: `音频设备已断开，切换到备用设备失败: ${deviceName}`
              }
            )
          }
        })

        // 监听无可用设备事件
        this._noDeviceAvailableUnlisten = await listen('no-device-available', () => {
          if (this._isDestroyed) return
          logger.error('No audio device available')
          
          // 暂停播放
          this.pause()
          
          errorHandler.handle(
            new Error('No audio device available'),
            {
              type: ErrorType.AUDIO_DEVICE_ERROR,
              severity: ErrorSeverity.CRITICAL,
              context: { action: 'no-device' },
              showToUser: true,
              userMessage: '没有可用的音频设备，请连接音频设备后重试'
            }
          )
        })

        // 监听默认设备变更事件（新设备添加且为系统默认）
        this._deviceDefaultChangedUnlisten = await listen<{ eventType: string; deviceName: string | null }>('device-default-changed', async (event) => {
          if (this._isDestroyed) return
          const deviceName = event.payload?.deviceName
          if (!deviceName) return

          logger.info(`System default device changed to: ${deviceName}`)
          
          try {
            const currentTime = this.currentTime
            await invoke('set_audio_device', { 
              deviceName, 
              currentTime: this.isPlaying ? currentTime : null 
            })
            
            logger.info(`Successfully switched to new default device: ${deviceName}`)
            
            // 显示成功通知
            errorHandler.handle(
              new Error(`Device switched to ${deviceName}`),
              {
                type: ErrorType.AUDIO_DEVICE_ERROR,
                severity: ErrorSeverity.LOW,
                context: { deviceName, action: 'switch-default-success' },
                showToUser: true,
                userMessage: `已自动切换到新设备: ${deviceName}`
              }
            )
          } catch (err) {
            errorHandler.handle(
              err instanceof Error ? err : new Error(String(err)),
              {
                type: ErrorType.AUDIO_DEVICE_ERROR,
                severity: ErrorSeverity.MEDIUM,
                context: { deviceName, action: 'switch-default' },
                showToUser: true,
                userMessage: `切换到新设备失败: ${deviceName}`
              }
            )
          }
        })

        logger.info('Device listeners setup complete')
      } catch (err) {
        logger.error('Failed to setup device listeners:', err)
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
      if (this._isDestroyed) return
      if (!track || this._isLoading) return

      let trackExists = await this._checkFileExists(track.path)
      if (!trackExists && track.path) {
        const altPath = track.path.includes('/') ? track.path.replace(/\//g, '\\') : track.path.replace(/\\/g, '/')
        if (altPath !== track.path) {
          track.path = altPath
          trackExists = await this._checkFileExists(altPath)
        }
      }

      if (!trackExists) {
        logger.warn('Track file not found:', track.path)
        const currentTrackIndex = this.playlist.findIndex(t => t.path === track.path)
        if (this.playlist.length > 1 && currentTrackIndex < this.playlist.length - 1) {
          return this.nextTrack()
        } else {
          await this.resetPlayerState()
          return
        }
      }

      this._isLoading = true
      this.stopStatusPolling()

      // 获取元数据
      const metadataCache = this._getMetadataCache()
      let metadata = metadataCache.get(track.path)
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
        }
      }

      this.lastTrackIndex = this.currentTrackIndex
      this.currentTrack = {
        ...track,
        title: metadata.title,
        artist: metadata.artist,
        album: metadata.album,
        duration: metadata.duration
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

      invoke('pause_track').catch(err => logger.debug("pause before play:", err))

      try {
        logger.info('Playing track:', track.path)

        const playPromise = invoke('play_track', { path: track.path })
        const timeoutPromise = new Promise((_, reject) => {
          setTimeout(() => reject(new Error('播放超时')), 5000)
        })

        await Promise.race([playPromise, timeoutPromise])

        this.isPlaying = true
        this.startStatusPolling()
        this._updateTaskbarState()

        this.loadLyrics(track.path).catch(err => {
          logger.debug('Lyrics load error:', err)
        })
      } catch (err) {
        const type = classifyAudioInvokeError(err)
        const handled = errorHandler.handle(
          err instanceof Error ? err : new Error(String(err)),
          {
            type,
            severity: ErrorSeverity.HIGH,
            context: { trackPath: track.path, trackName: track.name },
            showToUser: true,
          }
        )

        logger.error('Failed to play track:', handled)
        this.isPlaying = false

        const currentIdx = this.playlist.findIndex(t => t.path === track.path)
        if (this.playlist.length > 1 && currentIdx < this.playlist.length - 1) {
          setTimeout(() => this.nextTrack(), 100)
        }
      } finally {
        this._isLoading = false
      }
    },

    pause(): void {
      if (!this.isPlaying) return
      invoke('pause_track')
        .then(() => {
          this.isPlaying = false
          this.stopStatusPolling()
          this._updateTaskbarState()
        })
        .catch(err => logger.error("Failed to pause:", err))
    },

    resume(): void {
      if (this.isPlaying || !this.currentTrack) return
      invoke('resume_track')
        .then(() => {
          this.isPlaying = true
          this.startStatusPolling()
          this._updateTaskbarState()
        })
        .catch(err => logger.error("Failed to resume:", err))
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
      if (this._isDestroyed) return

      invoke('pause_track').catch(err => logger.debug("pause on ended:", err))

      if (this.repeatMode === 'track') {
        await this.playTrack(this.currentTrack!)
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
        invoke('pause_track').catch(err => logger.debug("pause after playlist ended:", err))
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

    async resetPlayerState(): Promise<void> {
      logger.info('Resetting player state')

      this.isPlaying = false
      this.stopStatusPolling()

      this.currentTrack = null
      this.playlist = []
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

      try {
        await invoke('pause_track')
      } catch (error) {
        logger.error('Error stopping backend playback:', error)
      }
    },

    async nextTrack(): Promise<void> {
      if (!this.currentTrack || this._isLoading) return

      let nextIndex: number
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          nextIndex = 0
        } else {
          do {
            nextIndex = Math.floor(Math.random() * this.playlist.length)
          } while (nextIndex === this.currentTrackIndex)
        }
      } else {
        nextIndex = (this.currentTrackIndex + 1) % this.playlist.length
      }

      await this.playTrack(this.playlist[nextIndex])
    },

    async previousTrack(): Promise<void> {
      if (!this.currentTrack || this._isLoading) return

      let prevIndex: number
      if (this.isShuffle) {
        if (this.playlist.length <= 1) {
          prevIndex = 0
        } else {
          do {
            prevIndex = Math.floor(Math.random() * this.playlist.length)
          } while (prevIndex === this.currentTrackIndex)
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

      const newTime = Math.max(0, Math.min(time, this.duration))

      invoke('seek_track', { time: newTime })
        .then(() => {
          this.currentTime = newTime
          if (!this.isPlaying) {
            this.resume()
          }
        })
        .catch(err => logger.error("Failed to seek:", err))
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
        .catch(err => logger.error("Failed to set volume:", err))
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
          .catch(err => logger.error("Failed to unmute:", err))
      } else {
        // 静音，保存当前音量
        this.previousVolume = this.volume > 0 ? this.volume : this.previousVolume
        this.isMuted = true
        invoke('set_volume', { volume: 0 })
          .catch(err => logger.error("Failed to mute:", err))
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
      }
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

    // --- 数据加载 ---

    loadPlaylist(playlist: Track[]): void {
      this.playlist = playlist
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
    },

    async _cachePlaylistMetadata(playlist: Track[]): Promise<void> {
      if (!playlist || playlist.length === 0) return

      const cache = this._getMetadataCache()
      const CHUNK_SIZE = 50
      let cached = 0

      for (let i = 0; i < playlist.length; i++) {
        const track = playlist[i]
        if (!track.path || cache.has(track.path)) continue

        cache.set(track.path, {
          title: (track as any).displayTitle || track.title || track.name || FileUtils.getFileName(track.path),
          artist: (track as any).displayArtist || track.artist || '',
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
          await new Promise(resolve => setTimeout(resolve, 0))
        }
      }

      logger.debug(`Cached metadata for ${cached} tracks`)
    },

    async loadLyrics(trackPath: string): Promise<void> {
      try {
        const lyricsPath = await FileUtils.findLyricsFile(trackPath)
        if (lyricsPath) {
          const lyricsContent = await FileUtils.readFile(lyricsPath)
          const format = FileUtils.getFileExtension(lyricsPath) as 'lrc' | 'ass' | 'srt'
          // 使用异步解析方法以支持翻译和卡拉OK效果
          this.lyrics = await LyricsParser.parseAsync(lyricsContent, format)
        } else {
          this.lyrics = null
        }
      } catch (error) {
        logger.debug('No lyrics found or failed to load:', error)
        this.lyrics = null
      }
    },

    // --- 清理 ---
    cleanup(): void {
      this._isDestroyed = true

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

      try {
        invoke('pause_track').catch(() => { })
        // 设置任务栏为停止状态
        invoke('set_taskbar_stopped').catch(() => { })
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

      logger.info('Player store cleaned up')
    }
  }
})
