import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useThemeStore } from './theme'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity, handlePromise } from '../utils/errorHandler'

// 防抖函数（带取消功能）
function debounce(func, wait) {
  let timeout
  const debounced = function (...args) {
    const context = this
    clearTimeout(timeout)
    timeout = setTimeout(() => func.apply(context, args), wait)
  }
  debounced.cancel = () => clearTimeout(timeout)
  return debounced
}

// 深度比较两个对象是否相等
function deepEqual(obj1, obj2) {
  if (obj1 === obj2) return true
  if (typeof obj1 !== 'object' || typeof obj2 !== 'object') return false
  if (obj1 === null || obj2 === null) return false
  
  const keys1 = Object.keys(obj1)
  const keys2 = Object.keys(obj2)
  
  if (keys1.length !== keys2.length) return false
  
  for (const key of keys1) {
    if (!keys2.includes(key)) return false
    if (!deepEqual(obj1[key], obj2[key])) return false
  }
  
  return true
}

/**
 * 配置系统存储（包含UI设置）
 */
export const useConfigStore = defineStore('config', {
  state: () => ({
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
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Roboto',
      lyricsStyle: 'modern',
    },

    // 歌词设置
    lyrics: {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease',
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

    // 内部状态（不保存到文件）
    _isInitializing: false,
    _isDirty: false,
    _lastSavedConfig: null,
    _savePromise: null,
  }),

  getters: {
    availableSeparators: (state) => {
      return [...new Set([state.titleExtraction.separator, ...state.titleExtraction.customSeparators])]
    },
    validSeparators: (state) => {
      return state.availableSeparators.filter(sep => sep && sep.trim() !== '')
    },
    hasUnsavedChanges: (state) => state._isDirty
  },

  actions: {
    // 获取可保存的配置（排除内部状态）
    _getSaveableConfig() {
      const { _isInitializing, _isDirty, _lastSavedConfig, _savePromise, ...config } = this.$state
      return config
    },

    // 标记配置已更改
    _markDirty() {
      if (!this._isInitializing) {
        this._isDirty = true
      }
    },

    // 检查配置是否真的有变化
    _hasRealChanges() {
      if (!this._lastSavedConfig) return true
      const currentConfig = this._getSaveableConfig()
      return !deepEqual(currentConfig, this._lastSavedConfig)
    },

    async loadConfig() {
      this._isInitializing = true

      const configResult = await handlePromise(
        invoke('load_config'),
        {
          type: ErrorType.CONFIG_LOAD_ERROR,
          severity: ErrorSeverity.MEDIUM,
          context: { action: 'loadConfig' },
          showToUser: false,
          throw: false
        }
      );

      if (configResult.success && configResult.data) {
        this.$patch(configResult.data)
        this._lastSavedConfig = JSON.parse(JSON.stringify(this._getSaveableConfig()))

        const themeStore = useThemeStore()
        if (configResult.data.general && configResult.data.general.theme !== themeStore.themePreference) {
          themeStore.setThemePreference(configResult.data.general.theme)
        }

        logger.info('Configuration loaded successfully')
      }

      const directoriesResult = await handlePromise(
        invoke('get_music_directories'),
        {
          type: ErrorType.CONFIG_LOAD_ERROR,
          severity: ErrorSeverity.LOW,
          context: { action: 'loadMusicDirectories' },
          showToUser: false,
          throw: false
        }
      );

      if (directoriesResult.success && directoriesResult.data) {
        this.musicDirectories = directoriesResult.data
        const { useMusicLibraryStore } = await import('./musicLibrary')
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

    async saveConfigNow() {
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

      this._savePromise = handlePromise(
        invoke('save_config', { config: configToSave }),
        {
          type: ErrorType.CONFIG_SAVE_ERROR,
          severity: ErrorSeverity.MEDIUM,
          context: { action: 'saveConfig' },
          showToUser: false,
          throw: false
        }
      );

      const result = await this._savePromise
      this._savePromise = null

      if (result.success) {
        this._lastSavedConfig = configToSave
        this._isDirty = false
        logger.debug('Configuration saved successfully')
      }
    },

    // 防抖保存（2秒延迟）
    saveConfig: debounce(function () {
      return this.saveConfigNow()
    }, 2000),

    async exportConfig(filePath) {
      try {
        const configToExport = JSON.parse(JSON.stringify(this._getSaveableConfig()))
        await invoke('export_config', { config: configToExport, filePath })
        logger.info('Configuration exported successfully')
      } catch (error) {
        logger.error('Failed to export config:', error)
        throw new Error('Failed to export configuration')
      }
    },

    async importConfig(filePath) {
      try {
        const config = await invoke('import_config', { filePath })
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

    resetToDefaults() {
      this.$reset()
      this._markDirty()
    },

    setDirectoryScanConfig(config) {
      this.directoryScan = { ...this.directoryScan, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setTitleExtractionConfig(config) {
      this.titleExtraction = { ...this.titleExtraction, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setPlaylistConfig(config) {
      this.playlist = { ...this.playlist, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    toggleSortOrder() {
      this.playlist.sortOrder = this.playlist.sortOrder === 'asc' ? 'desc' : 'asc'
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setGeneralConfig(config) {
      this.general = { ...this.general, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setAudioConfig(config) {
      this.audio = { ...this.audio, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    setLyricsConfig(config) {
      this.lyrics = { ...this.lyrics, ...config }
      this._markDirty()
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    addCustomSeparator(separator) {
      if (separator && !this.titleExtraction.customSeparators.includes(separator)) {
        this.titleExtraction.customSeparators.push(separator)
        this._markDirty()
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    removeCustomSeparator(separator) {
      const index = this.titleExtraction.customSeparators.indexOf(separator)
      if (index > -1) {
        this.titleExtraction.customSeparators.splice(index, 1)
        this._markDirty()
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    markInitializationComplete() {
      this._isInitializing = false
      this._isDirty = false
    },

    // UI 相关
    openSettings() { this.ui.showSettings = true },
    closeSettings() { this.ui.showSettings = false },
    toggleSettings() { this.ui.showSettings = !this.ui.showSettings },
    openConfigPanel() { this.ui.showConfigPanel = true },
    closeConfigPanel() { this.ui.showConfigPanel = false },
    toggleConfigPanel() { this.ui.showConfigPanel = !this.ui.showConfigPanel },

    async toggleMiniMode() {
      try {
        const newMode = !this.ui.miniMode
        await invoke('set_mini_mode', { enable: newMode })
        this.ui.miniMode = newMode
      } catch (error) {
        logger.error('Failed to toggle mini mode:', error)
        this.ui.miniMode = !this.ui.miniMode
      }
    }
  }
})
