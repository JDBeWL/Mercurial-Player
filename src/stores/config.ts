import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { load, type Store } from '@tauri-apps/plugin-store'
import { useThemeStore } from './theme'
import { useMusicLibraryStore } from './musicLibrary'
import logger from '../utils/logger'
import { ErrorType, ErrorSeverity, handlePromise } from '../utils/errorHandler'
import type {
  DirectoryScanConfig,
  TitleExtractionConfig,
  PlaylistConfig,
  GeneralConfig,
  LyricsConfig,
  DesktopLyricsConfig,
  UIConfig,
  AudioConfig,
  VisualizerConfig,
  AppConfig
} from '@/types'

// plugin-store 实例（懒加载单例）
let _storeInstance: Store | null = null
async function getStore(): Promise<Store> {
  if (!_storeInstance) {
    _storeInstance = await load('config.json', {
      defaults: {},
      autoSave: false,
    })
  }
  return _storeInstance
}

// 防抖函数（带取消功能）
interface DebouncedFunction<T extends (...args: unknown[]) => unknown> {
  (...args: Parameters<T>): void
  cancel: () => void
}

function debounce<T extends (...args: unknown[]) => unknown>(func: T, wait: number): DebouncedFunction<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const debounced = function(this: unknown, ...args: Parameters<T>) {
    clearTimeout(timeout)
    timeout = setTimeout(() => func.apply(this, args), wait)
  } as DebouncedFunction<T>
  debounced.cancel = () => clearTimeout(timeout)
  return debounced
}

// 深度比较两个对象是否相等
function deepEqual(obj1: unknown, obj2: unknown): boolean {
  if (obj1 === obj2) return true
  if (typeof obj1 !== 'object' || typeof obj2 !== 'object') return false
  if (obj1 === null || obj2 === null) return false
  
  const keys1 = Object.keys(obj1 as object)
  const keys2 = Object.keys(obj2 as object)
  
  if (keys1.length !== keys2.length) return false
  
  for (const key of keys1) {
    if (!keys2.includes(key)) return false
    if (!deepEqual((obj1 as Record<string, unknown>)[key], (obj2 as Record<string, unknown>)[key])) return false
  }
  
  return true
}

interface ConfigState {
  musicDirectories: string[]
  directoryScan: DirectoryScanConfig
  titleExtraction: TitleExtractionConfig
  playlist: PlaylistConfig
  general: GeneralConfig
  lyrics: LyricsConfig
  ui: UIConfig
  audio: AudioConfig
  visualizer: VisualizerConfig
  _isInitializing: boolean
  _isDirty: boolean
  _lastSavedConfig: Partial<AppConfig> | null
  _savePromise: Promise<unknown> | null
}

/**
 * 配置系统存储（包含UI设置）
 */
export const useConfigStore = defineStore('config', {
  state: (): ConfigState => ({
    // 音乐文件夹列表
    musicDirectories: [],

    // 子目录扫描配置
    directoryScan: {
      enableSubdirectoryScan: true,
      maxDepth: 3,
      ignoreHiddenFolders: true,
      folderBlacklist: ['.git', 'node_modules', 'temp', 'tmp']
    },

    // 标题提取配置
    titleExtraction: {
      preferMetadata: true,
      separator: '-',
      customSeparators: ['-', '_', '.', ' '],
      hideFileExtension: true,
      parseArtistTitle: true
    },

    // 播放列表配置
    playlist: {
      generateAllSongsPlaylist: true,
      folderBasedPlaylists: true,
      playlistNameFormat: '{folderName}',
      sortOrder: 'asc'
    },

    // 通用设置
    general: {
      language: 'zh',
      theme: 'auto',
      startupLoadLastConfig: true,
      autoSaveConfig: true,
      showAudioInfo: true,
      enableAutoUpdate: false,
      coverCacheSizeMb: 1024, // 1GB default
      coverCachePath: undefined, // 默认使用系统临时目录
    },

    // 歌词设置
    lyrics: {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease',
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Roboto',
      lyricsStyle: 'modern',
      desktopLyrics: {
        enabled: false,
        locked: true,
        fontSize: 28,
        colorPreset: 'dark' as const,
      },
    },

    // UI设置
    ui: {
      showSettings: false,
      showConfigPanel: false,
      miniMode: false
    },

    // 音频设置
    audio: {
      exclusiveMode: false,
      volume: 0.5
    },

    // 可视化设置
    visualizer: {
      targetFps: 60,
      enableVerticalSync: false
    },

    // 内部状态（不保存到文件）
    _isInitializing: false,
    _isDirty: false,
    _lastSavedConfig: null,
    _savePromise: null,
  }),

  getters: {
    availableSeparators: (state): string[] => {
      return [...new Set([state.titleExtraction.separator, ...state.titleExtraction.customSeparators])]
    },
    validSeparators(): string[] {
      return this.availableSeparators.filter((sep: string) => sep && sep.trim() !== '')
    },
    hasUnsavedChanges: (state): boolean => state._isDirty
  },

  actions: {
    // 获取可保存的配置（排除内部状态）
    _getSaveableConfig(): Partial<AppConfig> {
      const { _isInitializing, _isDirty, _lastSavedConfig, _savePromise, ...config } = this.$state

      // 创建一个副本以避免修改当前状态，因为 UI 临时状态不应持久化
      const saveableConfig = { ...config }
      if (saveableConfig.ui) {
        saveableConfig.ui = { 
          ...saveableConfig.ui,
          showSettings: false,
          showConfigPanel: false
        }
      }

      return saveableConfig
    },

    // 标记配置已更改
    _markDirty(): void {
      if (!this._isInitializing) {
        this._isDirty = true
      }
    },

    // 检查配置是否真的有变化
    _hasRealChanges(): boolean {
      if (!this._lastSavedConfig) return true
      const currentConfig = this._getSaveableConfig()
      return !deepEqual(currentConfig, this._lastSavedConfig)
    },

    async loadConfig(resetUI = true): Promise<void> {
      this._isInitializing = true

      // 优先从 plugin-store 加载配置
      let configData: Partial<AppConfig> | null = null
      try {
        const store = await getStore()
        const storeConfig = await store.get<Partial<AppConfig>>('appConfig')
        if (storeConfig && typeof storeConfig === 'object' && Object.keys(storeConfig).length > 0) {
          configData = storeConfig
          logger.info('Configuration loaded from plugin-store')
        }
      } catch (err) {
        logger.warn('Failed to load from plugin-store, falling back to backend:', err)
      }

      // 回退到后端 ConfigManager（首次迁移或 store 为空时）
      if (!configData) {
        const configResult = await handlePromise(
          invoke<Partial<AppConfig>>('load_config'),
          {
            type: ErrorType.CONFIG_LOAD_ERROR,
            severity: ErrorSeverity.MEDIUM,
            context: { action: 'loadConfig' },
            showToUser: false,
            throw: false
          }
        )
        if (configResult.success && configResult.data) {
          configData = configResult.data
          logger.info('Configuration loaded from backend ConfigManager')
          // lastSession 仅由后端管理 (避免 plugin-store 副本过期),
          // 迁移到 plugin-store 时剥离该字段
          if ('lastSession' in configData) {
            delete configData.lastSession
          }
          // 首次迁移：写入 plugin-store 以便后续直接使用
          try {
            const store = await getStore()
            await store.set('appConfig', configData)
            await store.save()
            logger.info('Migrated config to plugin-store')
          } catch (err) {
            logger.warn('Failed to migrate config to plugin-store:', err)
          }
        }
      } else {
        // 从 plugin-store 加载时也剥离 lastSession (历史遗留数据),
        // 保证该字段只由后端 save_last_session 命令管理
        if ('lastSession' in configData) {
          delete configData.lastSession
        }
      }

      if (configData) {
        // 迁移旧的歌词设置从 general 到 lyrics
        if (configData.general) {
          const general = configData.general as any
          if (general.lyricsAlignment || general.lyricsFontFamily || general.lyricsStyle) {
            if (!configData.lyrics) {
              configData.lyrics = {
                enableOnlineFetch: false,
                autoSaveOnlineLyrics: true,
                preferTranslation: true,
                onlineSource: 'netease',
                lyricsAlignment: 'center',
                lyricsFontFamily: 'Roboto',
                lyricsStyle: 'modern'
              }
            }
            if (general.lyricsAlignment) {
              configData.lyrics.lyricsAlignment = general.lyricsAlignment
              delete general.lyricsAlignment
            }
            if (general.lyricsFontFamily) {
              configData.lyrics.lyricsFontFamily = general.lyricsFontFamily
              delete general.lyricsFontFamily
            }
            if (general.lyricsStyle) {
              configData.lyrics.lyricsStyle = general.lyricsStyle
              delete general.lyricsStyle
            }
            if (!configData.lyrics.lyricsAlignment) configData.lyrics.lyricsAlignment = 'center'
            if (!configData.lyrics.lyricsFontFamily) configData.lyrics.lyricsFontFamily = 'Roboto'
            if (!configData.lyrics.lyricsStyle) configData.lyrics.lyricsStyle = 'modern'

            logger.info('Migrated lyrics settings from general to lyrics config')
            this._markDirty()
          }
        }

        this.$patch(configData)

        // 仅在明确要求时重置 UI 临时面板状态，避免在用户正在使用设置页时意外关闭
        if (resetUI) {
          this.ui.showSettings = false
          this.ui.showConfigPanel = false
        }

        this._lastSavedConfig = JSON.parse(JSON.stringify(this._getSaveableConfig()))

        const themeStore = useThemeStore()
        if (configData.general && configData.general.theme !== themeStore.themePreference) {
          themeStore.setThemePreference(configData.general.theme)
        }

        logger.info('Configuration loaded successfully')
      }

      const directoriesResult = await handlePromise(
        invoke<string[]>('get_music_directories'),
        {
          type: ErrorType.CONFIG_LOAD_ERROR,
          severity: ErrorSeverity.LOW,
          context: { action: 'loadMusicDirectories' },
          showToUser: false,
          throw: false
        }
      )

      if (directoriesResult.success && directoriesResult.data) {
        this.musicDirectories = directoriesResult.data
        const musicLibraryStore = useMusicLibraryStore()
        musicLibraryStore.musicFolders = directoriesResult.data
        logger.info('Music directories loaded successfully')
      } else {
        this.musicDirectories = []
      }

      setTimeout(() => {
        this.markInitializationComplete()
      }, 1000)
    },

    async saveConfigNow(): Promise<void> {
      // 检查是否真的有变化
      if (!this._hasRealChanges()) {
        logger.debug('No config changes to save')
        return
      }

      // 如果已经有保存操作在进行，等待它完成
      if (this._savePromise) {
        await this._savePromise
      }

      const configToSave = JSON.parse(JSON.stringify(this._getSaveableConfig()))
      const themeStore = useThemeStore()
      configToSave.general.theme = themeStore.themePreference

      // 确保 lyrics 配置包含所有必需字段
      if (!configToSave.lyrics) {
        configToSave.lyrics = {
          enableOnlineFetch: false,
          autoSaveOnlineLyrics: true,
          preferTranslation: true,
          onlineSource: 'netease',
          lyricsAlignment: 'center',
          lyricsFontFamily: 'Roboto',
          lyricsStyle: 'modern',
          desktopLyrics: {
            enabled: false,
            locked: true,
            fontSize: 28,
          },
        }
      } else {
        if (!configToSave.lyrics.lyricsAlignment) configToSave.lyrics.lyricsAlignment = 'center'
        if (!configToSave.lyrics.lyricsFontFamily) configToSave.lyrics.lyricsFontFamily = 'Roboto'
        if (!configToSave.lyrics.lyricsStyle) configToSave.lyrics.lyricsStyle = 'modern'
        if (configToSave.lyrics.onlineSource === undefined) configToSave.lyrics.onlineSource = 'netease'
        if (!configToSave.lyrics.desktopLyrics) {
          configToSave.lyrics.desktopLyrics = { enabled: false, locked: true, fontSize: 28, colorPreset: 'dark' as const }
        }
      }

      // 主存储：plugin-store
      this._savePromise = (async () => {
        try {
          const store = await getStore()
          await store.set('appConfig', configToSave)
          await store.save()
          logger.debug('Configuration saved to plugin-store')
        } catch (err) {
          logger.warn('Failed to save to plugin-store:', err)
        }

        // 同步到后端 ConfigManager（确保 Rust 侧的 exclusive_mode 等配置保持一致）
        await handlePromise(
          invoke('save_config', { config: configToSave }),
          {
            type: ErrorType.CONFIG_SAVE_ERROR,
            severity: ErrorSeverity.LOW,
            context: { action: 'saveConfigBackend' },
            showToUser: false,
            throw: false
          }
        )
      })()

      await this._savePromise
      this._savePromise = null

      this._lastSavedConfig = configToSave
      this._isDirty = false
      logger.debug('Configuration saved successfully')
    },

    // 防抖保存（2秒延迟）
    saveConfig: debounce(function(this: any) {
      return this.saveConfigNow()
    }, 2000) as DebouncedFunction<() => void>,

    // 强制立即保存（取消防抖，用于应用关闭前）
    async flushPendingSave(): Promise<void> {
      // 取消待执行的防抖保存
      if ((this.saveConfig as DebouncedFunction<() => void>).cancel) {
        (this.saveConfig as DebouncedFunction<() => void>).cancel()
      }
      // 立即保存
      await this.saveConfigNow()
    },

    async exportConfig(filePath: string): Promise<void> {
      try {
        const configToExport = JSON.parse(JSON.stringify(this._getSaveableConfig()))
        await invoke('export_config', { config: configToExport, filePath })
        logger.info('Configuration exported successfully')
      } catch (error) {
        logger.error('Failed to export config:', error)
        throw new Error('Failed to export configuration')
      }
    },

    async importConfig(filePath: string): Promise<void> {
      try {
        const config = await invoke<Partial<AppConfig>>('import_config', { filePath })
        if (config) {
          this.$patch(config)
          this._markDirty()
          logger.info('Configuration imported successfully')
        }
      } catch (error) {
        logger.error('Failed to import config:', error)
        throw new Error('Failed to import configuration')
      }
    },

    resetToDefaults(): void {
      this.$reset()
      this._markDirty()
    },

    setDirectoryScanConfig(config: Partial<DirectoryScanConfig>): void {
      this.directoryScan = { ...this.directoryScan, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setTitleExtractionConfig(config: Partial<TitleExtractionConfig>): void {
      this.titleExtraction = { ...this.titleExtraction, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setPlaylistConfig(config: Partial<PlaylistConfig>): void {
      this.playlist = { ...this.playlist, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    toggleSortOrder(): void {
      this.playlist.sortOrder = this.playlist.sortOrder === 'asc' ? 'desc' : 'asc'
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setGeneralConfig(config: Partial<GeneralConfig>): void {
      this.general = { ...this.general, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setAudioConfig(config: Partial<AudioConfig>): void {
      this.audio = { ...this.audio, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setLyricsConfig(config: Partial<LyricsConfig>): void {
      this.lyrics = { ...this.lyrics, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setDesktopLyricsConfig(config: Partial<DesktopLyricsConfig>): void {
      if (!this.lyrics.desktopLyrics) {
        this.lyrics.desktopLyrics = { enabled: false, locked: true, fontSize: 28, colorPreset: 'dark' as const }
      }
      this.lyrics.desktopLyrics = { ...this.lyrics.desktopLyrics, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    addCustomSeparator(separator: string): void {
      if (separator && !this.titleExtraction.customSeparators.includes(separator)) {
        this.titleExtraction.customSeparators.push(separator)
        this._markDirty()
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    removeCustomSeparator(separator: string): void {
      const index = this.titleExtraction.customSeparators.indexOf(separator)
      if (index > -1) {
        this.titleExtraction.customSeparators.splice(index, 1)
        this._markDirty()
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    markInitializationComplete(): void {
      this._isInitializing = false
      this._isDirty = false
    },

    // UI 相关
    openSettings(): void { this.ui.showSettings = true },
    closeSettings(): void { this.ui.showSettings = false },
    toggleSettings(): void { this.ui.showSettings = !this.ui.showSettings },
    openConfigPanel(): void { this.ui.showConfigPanel = true },
    closeConfigPanel(): void { this.ui.showConfigPanel = false },
    toggleConfigPanel(): void { this.ui.showConfigPanel = !this.ui.showConfigPanel },

    async toggleMiniMode(): Promise<void> {
      try {
        const newMode = !this.ui.miniMode
        await invoke('set_mini_mode', { enable: newMode })
        this.ui.miniMode = newMode
      } catch (error) {
        logger.error('Failed to toggle mini mode:', error)
        // invoke 失败时不修改状态，因为 try 块中尚未修改
      }
    }
  }
})
