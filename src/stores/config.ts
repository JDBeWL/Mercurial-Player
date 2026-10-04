import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useThemeStore } from './theme'
import { useMusicLibraryStore } from './musicLibrary'
import logger from '../utils/logger'
import { debounce, type DebouncedFunction } from '../utils/function'
import { deepClone, deepEqual } from '../utils/object'
import {
  createDefaultLyricsConfig,
  ensureLyricsConfigDefaults,
  migrateLyricsFieldsFromGeneral,
} from '../utils/configDefaults'
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
  AppConfig,
} from '@/types'

/** 初始化完成的延迟句柄(重新 loadConfig 时需取消旧的) */
let initCompleteTimer: ReturnType<typeof setTimeout> | null = null

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

/** 配置系统存储（包含 UI 设置） */
export const useConfigStore = defineStore('config', {
  state: (): ConfigState => ({
    musicDirectories: [],

    directoryScan: {
      enableSubdirectoryScan: true,
      maxDepth: 3,
      ignoreHiddenFolders: true,
      folderBlacklist: ['.git', 'node_modules', 'temp', 'tmp'],
    },

    titleExtraction: {
      preferMetadata: true,
      separator: '-',
      customSeparators: ['-', '_', '.'],
      hideFileExtension: true,
      parseArtistTitle: true,
    },

    playlist: {
      generateAllSongsPlaylist: true,
      folderBasedPlaylists: true,
      playlistNameFormat: '{folderName}',
      sortOrder: 'asc',
    },

    general: {
      language: 'zh',
      theme: 'auto',
      startupLoadLastConfig: true,
      autoSaveConfig: true,
      showAudioInfo: true,
      showQueueInfo: true,
      immersiveColorScheme: 'album' as const,
      immersiveAutoTheme: true,
      enableAutoUpdate: false,
      coverCacheSizeMb: 1024, // 1GB default
      coverCachePath: undefined, // 默认使用系统临时目录
    },

    // 歌词设置(默认值统一由 utils/configDefaults 维护)
    lyrics: createDefaultLyricsConfig(),

    ui: {
      showSettings: false,
      showConfigPanel: false,
      miniMode: false,
      /* 界面字号倍率，见 UIConfig.fontScale 的注释 */
      fontScale: 1,
    },

    audio: {
      exclusiveMode: false,
      volume: 0.5,
      fadeEnabled: true,
      usbDacExclusive: false,
    },

    visualizer: {
      targetFps: 60,
      enableVerticalSync: false,
    },

    // 内部状态（不保存到文件）
    _isInitializing: false,
    _isDirty: false,
    _lastSavedConfig: null,
    _savePromise: null,
  }),

  getters: {
    availableSeparators: (state): string[] => {
      return [
        ...new Set([state.titleExtraction.separator, ...state.titleExtraction.customSeparators]),
      ]
    },
    validSeparators(): string[] {
      return this.availableSeparators.filter((sep: string) => sep && sep.trim() !== '')
    },
    hasUnsavedChanges: (state): boolean => state._isDirty,
  },

  actions: {
    // 返回完整 AppConfig 而非 Partial:Partial 会让调用方的字段访问失去类型保护
    _getSaveableConfig(): AppConfig {
      const { _isInitializing, _isDirty, _lastSavedConfig, _savePromise, ...config } = this.$state

      // 浅拷贝后置空面板开关:UI 临时状态不应持久化
      const saveableConfig = { ...config }
      if (saveableConfig.ui) {
        saveableConfig.ui = {
          ...saveableConfig.ui,
          showSettings: false,
          showConfigPanel: false,
        }
      }

      return saveableConfig
    },

    // 初始化期间置位同样有效:此时只跳过自动保存(见 _patchSection),落盘由 markInitializationComplete 补一次
    _markDirty(): void {
      this._isDirty = true
    },

    _hasRealChanges(): boolean {
      if (!this._lastSavedConfig) return true
      const currentConfig = this._getSaveableConfig()
      return !deepEqual(currentConfig, this._lastSavedConfig)
    },

    async loadConfig(resetUI = true): Promise<void> {
      this._isInitializing = true

      // 配置由后端 ConfigManager 读写 data/config.json(裸 AppConfig 格式),旧版布局由后端做一次性迁移
      const configResult = await handlePromise(invoke<Partial<AppConfig>>('load_config'), {
        type: ErrorType.CONFIG_LOAD_ERROR,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'loadConfig' },
        showToUser: false,
        throw: false,
      })
      let configData: Partial<AppConfig> | null = null
      if (configResult.success && configResult.data) {
        configData = configResult.data
        logger.info('Configuration loaded from backend ConfigManager')
        // lastSession 仅由后端管理,加载后剥离,避免前端副本过期
        if ('lastSession' in configData) {
          delete configData.lastSession
        }
      }

      if (configData) {
        if (migrateLyricsFieldsFromGeneral(configData)) {
          logger.info('Migrated lyrics settings from general to lyrics config')
          this._markDirty()
        }

        this.$patch(configData)

        // 仅在明确要求时重置 UI 临时面板状态，避免在用户正在使用设置页时意外关闭
        if (resetUI) {
          this.ui.showSettings = false
          this.ui.showConfigPanel = false
        }

        this._lastSavedConfig = deepClone(this._getSaveableConfig())

        const themeStore = useThemeStore()
        if (configData.general && configData.general.theme !== themeStore.themePreference) {
          themeStore.setThemePreference(configData.general.theme)
        }

        // 上次退出时处于迷你模式则恢复窗口状态(需配置加载完成后窗口已就绪)
        if (this.ui.miniMode) {
          invoke('set_mini_mode', { enable: true }).catch((error) => {
            logger.warn('Failed to restore mini mode on startup:', error)
          })
        }

        logger.info('Configuration loaded successfully')
      }

      const directoriesResult = await handlePromise(invoke<string[]>('get_music_directories'), {
        type: ErrorType.CONFIG_LOAD_ERROR,
        severity: ErrorSeverity.LOW,
        context: { action: 'loadMusicDirectories' },
        showToUser: false,
        throw: false,
      })

      if (directoriesResult.success && directoriesResult.data) {
        this.musicDirectories = directoriesResult.data
        const musicLibraryStore = useMusicLibraryStore()
        musicLibraryStore.musicFolders = directoriesResult.data
        logger.info('Music directories loaded successfully')
      } else {
        this.musicDirectories = []
      }

      if (initCompleteTimer !== null) {
        clearTimeout(initCompleteTimer)
      }
      initCompleteTimer = setTimeout(() => {
        initCompleteTimer = null
        this.markInitializationComplete()
      }, 1000)
    },

    async saveConfigNow(): Promise<void> {
      if (!this._hasRealChanges()) {
        // 无实际变化(如仅迁移了配置字段):顺手清掉脏标记,保持与状态一致
        this._isDirty = false
        logger.debug('No config changes to save')
        return
      }

      if (this._savePromise) {
        await this._savePromise
      }

      const configToSave = deepClone(this._getSaveableConfig())
      const themeStore = useThemeStore()
      configToSave.general.theme = themeStore.themePreference

      // 兼容旧版配置:补齐 lyrics 缺失字段
      if (!configToSave.lyrics) {
        configToSave.lyrics = createDefaultLyricsConfig()
      } else {
        configToSave.lyrics = ensureLyricsConfigDefaults(configToSave.lyrics)
      }

      // 主存储：后端 ConfigManager 写 data/config.json(原子写:先 .tmp 再 rename)
      const savePromise = handlePromise(invoke('save_config', { config: configToSave }), {
        type: ErrorType.CONFIG_SAVE_ERROR,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'saveConfig' },
        showToUser: false,
        throw: false,
      })
      this._savePromise = savePromise

      const saveResult = await savePromise
      this._savePromise = null

      if (saveResult.success) {
        this._lastSavedConfig = configToSave
        this._isDirty = false
        logger.debug('Configuration saved successfully')
      } else {
        // 保存失败 (磁盘满/权限错误等):保留脏标记与旧快照,
        // 下一次防抖保存会因 _hasRealChanges() 为 true 而自动重试
        logger.error('Configuration save failed; dirty flag retained for retry', saveResult.error)
      }
    },

    // 防抖 2s 合并落盘:每次保存都要对整个 config 深比较 + 深拷贝 + 写盘,
    // 音量/主色这类高频改动逐次写入会卡住主线程;关闭前由 flushPendingSave 兜底
    saveConfig: debounce(function (this: { saveConfigNow: () => Promise<void> }) {
      return this.saveConfigNow()
    }, 2000) as DebouncedFunction<() => void>,

    // 取消防抖并立即落盘(应用关闭前调用)
    async flushPendingSave(): Promise<void> {
      if ((this.saveConfig as DebouncedFunction<() => void>).cancel) {
        ;(this.saveConfig as DebouncedFunction<() => void>).cancel()
      }
      await this.saveConfigNow()
    },

    async exportConfig(filePath: string): Promise<void> {
      try {
        const configToExport = deepClone(this._getSaveableConfig())
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

    /** 统一的"合并分区 -> 标脏 -> 自动保存"入口,各 setXxxConfig 只指明分区与补丁 */
    _patchSection<K extends keyof AppConfig>(section: K, patch: Partial<AppConfig[K]>): void {
      const state = this.$state as unknown as AppConfig
      state[section] = { ...state[section], ...patch }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setDirectoryScanConfig(config: Partial<DirectoryScanConfig>): void {
      this._patchSection('directoryScan', config)
    },

    setTitleExtractionConfig(config: Partial<TitleExtractionConfig>): void {
      this._patchSection('titleExtraction', config)
    },

    setPlaylistConfig(config: Partial<PlaylistConfig>): void {
      this._patchSection('playlist', config)
    },

    toggleSortOrder(): void {
      this.playlist.sortOrder = this.playlist.sortOrder === 'asc' ? 'desc' : 'asc'
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setGeneralConfig(config: Partial<GeneralConfig>): void {
      this._patchSection('general', config)
    },

    setAudioConfig(config: Partial<AudioConfig>): void {
      this._patchSection('audio', config)
    },

    setLyricsConfig(config: Partial<LyricsConfig>): void {
      this._patchSection('lyrics', config)
    },

    /** 设置应用内界面字号倍率。这里只落配置，下发给原生侧（Android 的 WebView textZoom）
     *  由 `applyAppFontScale()` 负责，启动时由应用生命周期在配置加载完成后调用一次。 */
    setUIFontScale(scale: number): void {
      this._patchSection('ui', { fontScale: scale })
    },

    setDesktopLyricsConfig(config: Partial<DesktopLyricsConfig>): void {
      const desktopLyrics = this.lyrics.desktopLyrics ?? {
        enabled: false,
        locked: true,
        fontSize: 28,
        colorPreset: 'auto' as const,
      }
      this._patchSection('lyrics', { desktopLyrics: { ...desktopLyrics, ...config } })
    },

    markInitializationComplete(): void {
      this._isInitializing = false
      // 补存初始化窗口内被跳过的自动保存(原因见 _markDirty 注释)
      if (this._isDirty && this.general.autoSaveConfig) {
        this.saveConfig()
      }
    },

    // UI 相关
    openSettings(): void {
      this.ui.showSettings = true
    },
    closeSettings(): void {
      this.ui.showSettings = false
    },
    toggleSettings(): void {
      this.ui.showSettings = !this.ui.showSettings
    },
    openConfigPanel(): void {
      this.ui.showConfigPanel = true
    },
    closeConfigPanel(): void {
      this.ui.showConfigPanel = false
    },
    toggleConfigPanel(): void {
      this.ui.showConfigPanel = !this.ui.showConfigPanel
    },

    async toggleMiniMode(): Promise<void> {
      const newMode = !this.ui.miniMode
      try {
        await invoke('set_mini_mode', { enable: newMode })
        // 经 _patchSection 走统一"标脏 + 自动保存"链路,确保 mini 模式重启后恢复
        this._patchSection('ui', { miniMode: newMode })
      } catch (error) {
        logger.error('Failed to toggle mini mode:', error)
        // invoke 失败时不修改状态，因为 try 块中尚未修改
      }
    },
  },
})
