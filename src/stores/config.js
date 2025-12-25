import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useThemeStore } from './theme'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity, handlePromise } from '../utils/errorHandler'

// 防抖函数
function debounce(func, wait) {
  let timeout
  return function (...args) {
    const context = this
    clearTimeout(timeout)
    timeout = setTimeout(() => func.apply(context, args), wait)
  }
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
      enableSubdirectoryScan: true,     // 是否启用子目录扫描
      maxDepth: 3,                     // 最大扫描深度
      ignoreHiddenFolders: true,       // 忽略隐藏文件夹
      folderBlacklist: ['.git', 'node_modules', 'temp', 'tmp'] // 忽略的文件夹
    },

    // 标题提取配置
    titleExtraction: {
      preferMetadata: true,            // 优先使用元数据
      separator: '-',                   // 文件名分隔符（默认）
      customSeparators: ['-', '_', '.', ' '], // 自定义分隔符列表
      hideFileExtension: true,         // 隐藏文件扩展名
      parseArtistTitle: true           // 从文件名解析艺术家和标题
    },

    // 播放列表配置
    playlist: {
      generateAllSongsPlaylist: true,   // 生成"全部歌曲"播放列表
      folderBasedPlaylists: true,      // 基于文件夹生成播放列表
      playlistNameFormat: '{folderName}', // 播放列表名称格式
      sortOrder: 'asc'                // 排序顺序: 'asc' (A-Z) 或 'desc' (Z-A)
    },

    // 通用设置
    general: {
      language: 'zh',                   // 语言
      theme: 'auto',                    // 主题
      startupLoadLastConfig: true,      // 启动时加载上次配置
      autoSaveConfig: true,               // 自动保存配置
      showAudioInfo: true,              // 是否显示音频信息
      lyricsAlignment: 'center',        // 歌词对齐方式
      lyricsFontFamily: 'Roboto',       // 歌词字体
      lyricsStyle: 'modern',           // 歌词样式
    },

    // 歌词设置
    lyrics: {
      enableOnlineFetch: false,         // 是否启用网络歌词加载
      autoSaveOnlineLyrics: true,       // 自动保存下载的歌词到本地
      preferTranslation: true,          // 优先显示翻译歌词
      onlineSource: 'netease',          // 在线歌词源: 'netease'
    },

    // UI设置（原ui.js的功能）
    ui: {
      showSettings: false,              // 是否显示设置面板
      showConfigPanel: false,           // 是否显示配置面板
      miniMode: false                   // 是否处于Mini模式
    },

    // 音频设置
    audio: {
      exclusiveMode: false,             // 独占模式
      volume: 0.5                       // 音量 (0.0 - 1.0)
    }
  }),

  getters: {
    // 获取配置的分隔符列表
    availableSeparators: (state) => {
      return [...new Set([state.titleExtraction.separator, ...state.titleExtraction.customSeparators])]
    },

    // 获取有效的分隔符（过滤掉空字符串）
    validSeparators: (state) => {
      return state.availableSeparators.filter(sep => sep && sep.trim() !== '')
    }
  },

  actions: {
    /**
     * 从后端加载配置
     */
    async loadConfig() {
      // 标记为初始化状态，防止自动保存
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

        // 只在主题不同时才更新主题，避免不必要的重置
        const themeStore = useThemeStore()
        if (configResult.data.general && configResult.data.general.theme !== themeStore.themePreference) {
          themeStore.setThemePreference(configResult.data.general.theme)
        }

        logger.info('Configuration loaded successfully')
      }

      // 单独加载音乐文件夹列表
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
        // 同时更新音乐库存储中的音乐文件夹列表
        const { useMusicLibraryStore } = await import('./musicLibrary')
        const musicLibraryStore = useMusicLibraryStore()
        musicLibraryStore.musicFolders = directoriesResult.data
        logger.info('Music directories loaded successfully')
      } else {
        this.musicDirectories = []
      }

      // 延迟标记初始化完成，确保所有配置都已应用
      setTimeout(() => {
        this.markInitializationComplete()
      }, 1000)
    },

    /**
     * 立即保存配置到后端（用于手动保存）
     */
    async saveConfigNow() {
      // 深拷贝配置，以便在保存前更新主题
      const configToSave = JSON.parse(JSON.stringify(this.$state))
      // 更新主题
      const themeStore = useThemeStore()
      configToSave.general.theme = themeStore.themePreference

      const result = await handlePromise(
        invoke('save_config', { config: configToSave }),
        {
          type: ErrorType.CONFIG_SAVE_ERROR,
          severity: ErrorSeverity.MEDIUM,
          context: { action: 'saveConfig' },
          showToUser: true,
          throw: true
        }
      );

      if (result.success) {
        logger.debug('Configuration saved successfully')
      }
    },

    /**
     * 保存配置到后端（使用防抖）
     */
    saveConfig: debounce(function () {
      return this.saveConfigNow()
    }, 1000),

    /**
     * 导出配置到文件
     */
    async exportConfig(filePath) {
      try {
        const configToExport = JSON.parse(JSON.stringify(this.$state))
        await invoke('export_config', {
          config: configToExport,
          filePath
        })
        logger.info('Configuration exported successfully')
      } catch (error) {
        logger.error('Failed to export config:', error)
        throw new Error('Failed to export configuration')
      }
    },

    /**
     * 从文件导入配置
     */
    async importConfig(filePath) {
      try {
        const config = await invoke('import_config', { filePath })
        if (config) {
          this.$patch(config)
          logger.info('Configuration imported successfully')
        }
      } catch (error) {
        logger.error('Failed to import config:', error)
        throw new Error('Failed to import configuration')
      }
    },

    /**
     * 重置配置为默认值
     */
    resetToDefaults() {
      this.$reset()
    },

    /**
     * 设置子目录扫描配置
     */
    setDirectoryScanConfig(config) {
      this.directoryScan = { ...this.directoryScan, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 设置标题提取配置
     */
    setTitleExtractionConfig(config) {
      this.titleExtraction = { ...this.titleExtraction, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 设置播放列表配置
     */
    setPlaylistConfig(config) {
      this.playlist = { ...this.playlist, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 切换排序顺序
     */
    toggleSortOrder() {
      this.playlist.sortOrder = this.playlist.sortOrder === 'asc' ? 'desc' : 'asc'
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 设置通用配置
     */
    setGeneralConfig(config) {
      this.general = { ...this.general, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 设置音频配置
     */
    setAudioConfig(config) {
      this.audio = { ...this.audio, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 设置歌词配置
     */
    setLyricsConfig(config) {
      this.lyrics = { ...this.lyrics, ...config }
      // 只在自动保存配置启用且不是程序启动时才保存
      if (this.general.autoSaveConfig && !this._isInitializing) {
        this.saveConfig()
      }
    },

    /**
     * 添加自定义分隔符
     */
    addCustomSeparator(separator) {
      if (separator && !this.titleExtraction.customSeparators.includes(separator)) {
        this.titleExtraction.customSeparators.push(separator)
        // 只在自动保存配置启用且不是程序启动时才保存
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    /**
     * 移除自定义分隔符
     */
    removeCustomSeparator(separator) {
      const index = this.titleExtraction.customSeparators.indexOf(separator)
      if (index > -1) {
        this.titleExtraction.customSeparators.splice(index, 1)
        // 只在自动保存配置启用且不是程序启动时才保存
        if (this.general.autoSaveConfig && !this._isInitializing) {
          this.saveConfig()
        }
      }
    },

    /**
     * 标记初始化状态完成
     */
    markInitializationComplete() {
      this._isInitializing = false
    },

    // ========== UI设置相关Actions ==========

    /**
     * 打开设置面板
     */
    openSettings() {
      this.ui.showSettings = true
    },

    /**
     * 关闭设置面板
     */
    closeSettings() {
      this.ui.showSettings = false
    },

    /**
     * 切换设置面板显示状态
     */
    toggleSettings() {
      this.ui.showSettings = !this.ui.showSettings
    },

    /**
     * 打开配置面板
     */
    openConfigPanel() {
      this.ui.showConfigPanel = true
    },

    /**
     * 关闭配置面板
     */
    closeConfigPanel() {
      this.ui.showConfigPanel = false
    },

    /**
     * 切换配置面板显示状态
     */
    toggleConfigPanel() {
      this.ui.showConfigPanel = !this.ui.showConfigPanel
    },

    /**
     * 切换Mini模式
     */
    async toggleMiniMode() {
      try {
        const newMode = !this.ui.miniMode
        // 调用后端命令调整窗口
        await invoke('set_mini_mode', { enable: newMode })
        this.ui.miniMode = newMode
      } catch (error) {
        logger.error('Failed to toggle mini mode:', error)
        // 如果后端失败，回滚状态
        this.ui.miniMode = !this.ui.miniMode
      }
    }
  }
})