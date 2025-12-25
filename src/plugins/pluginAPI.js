/**
 * 插件 API
 * 为插件提供安全的接口访问应用功能
 */

import { computed, readonly } from 'vue'
import { PluginPermission } from './pluginManager'
import logger from '../utils/logger'

/**
 * 创建插件 API
 * @param {string} pluginId 插件 ID
 * @param {string[]} permissions 插件权限
 * @param {PluginManager} manager 插件管理器
 */
export function createPluginAPI(pluginId, permissions, manager) {
  // 权限检查
  const hasPermission = (permission) => permissions.includes(permission)
  
  const requirePermission = (permission, action) => {
    if (!hasPermission(permission)) {
      throw new Error(`插件 ${pluginId} 没有 ${permission} 权限，无法执行 ${action}`)
    }
  }

  // 延迟加载 stores（避免循环依赖）
  let playerStore = null
  let configStore = null
  let musicLibraryStore = null
  let themeStore = null

  const getPlayerStore = async () => {
    if (!playerStore) {
      const { usePlayerStore } = await import('../stores/player')
      playerStore = usePlayerStore()
    }
    return playerStore
  }

  const getConfigStore = async () => {
    if (!configStore) {
      const { useConfigStore } = await import('../stores/config')
      configStore = useConfigStore()
    }
    return configStore
  }

  const getMusicLibraryStore = async () => {
    if (!musicLibraryStore) {
      const { useMusicLibraryStore } = await import('../stores/musicLibrary')
      musicLibraryStore = useMusicLibraryStore()
    }
    return musicLibraryStore
  }

  const getThemeStore = async () => {
    if (!themeStore) {
      const { useThemeStore } = await import('../stores/theme')
      themeStore = useThemeStore()
    }
    return themeStore
  }

  return {
    // ========== 插件信息 ==========
    pluginId,
    permissions: readonly(permissions),

    // ========== 日志 ==========
    log: {
      info: (...args) => logger.info(`[Plugin:${pluginId}]`, ...args),
      warn: (...args) => logger.warn(`[Plugin:${pluginId}]`, ...args),
      error: (...args) => logger.error(`[Plugin:${pluginId}]`, ...args),
      debug: (...args) => logger.debug(`[Plugin:${pluginId}]`, ...args),
    },

    // ========== 播放器 API ==========
    player: {
      // 获取当前播放状态（只读）
      async getState() {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getState')
        const store = await getPlayerStore()
        return {
          currentTrack: store.currentTrack ? { ...store.currentTrack } : null,
          isPlaying: store.isPlaying,
          currentTime: store.currentTime,
          duration: store.duration,
          volume: store.volume,
          repeatMode: store.repeatMode,
          isShuffle: store.isShuffle,
        }
      },

      // 获取当前歌词
      async getLyrics() {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getLyrics')
        const store = await getPlayerStore()
        return store.lyrics ? [...store.lyrics] : null
      },

      // 获取当前歌词索引
      async getCurrentLyricIndex() {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getCurrentLyricIndex')
        const store = await getPlayerStore()
        return store.currentLyricIndex
      },

      // 播放控制
      async play() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.play')
        const store = await getPlayerStore()
        store.play()
      },

      async pause() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.pause')
        const store = await getPlayerStore()
        store.pause()
      },

      async togglePlay() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.togglePlay')
        const store = await getPlayerStore()
        store.togglePlay()
      },

      async next() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.next')
        const store = await getPlayerStore()
        await store.nextTrack()
      },

      async previous() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.previous')
        const store = await getPlayerStore()
        await store.previousTrack()
      },

      async seek(time) {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.seek')
        const store = await getPlayerStore()
        store.seek(time)
      },

      async setVolume(volume) {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.setVolume')
        const store = await getPlayerStore()
        store.setVolume(volume)
      },

      // 设置歌词（供歌词源插件使用）
      async setLyrics(lyrics) {
        requirePermission(PluginPermission.LYRICS_PROVIDER, 'player.setLyrics')
        const store = await getPlayerStore()
        store.lyrics = lyrics
      },
    },

    // ========== 音乐库 API ==========
    library: {
      async getPlaylists() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getPlaylists')
        const store = await getMusicLibraryStore()
        return store.playlists ? [...store.playlists] : []
      },

      async getCurrentPlaylist() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getCurrentPlaylist')
        const store = await getMusicLibraryStore()
        return store.currentPlaylist ? { ...store.currentPlaylist } : null
      },

      async getTracks() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getTracks')
        const store = await getPlayerStore()
        return store.playlist ? [...store.playlist] : []
      },
    },

    // ========== 主题 API ==========
    theme: {
      async getCurrent() {
        const store = await getThemeStore()
        return {
          preference: store.themePreference,
          isDark: store.isDark,
          primaryColor: store.primaryColor,
        }
      },

      async setColors(colors) {
        requirePermission(PluginPermission.THEME, 'theme.setColors')
        // 设置 CSS 变量
        const root = document.documentElement
        for (const [key, value] of Object.entries(colors)) {
          root.style.setProperty(`--plugin-${pluginId}-${key}`, value)
        }
      },
    },

    // ========== UI 扩展 API ==========
    ui: {
      // 注册设置面板
      registerSettingsPanel(panel) {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerSettingsPanel')
        manager.registerExtension('settingsPanels', pluginId, panel)
      },

      // 注册菜单项
      registerMenuItem(item) {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerMenuItem')
        manager.registerExtension('menuItems', pluginId, item)
      },

      // 注册播放器装饰器（在播放器周围添加内容）
      registerPlayerDecorator(decorator) {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerPlayerDecorator')
        manager.registerExtension('playerDecorators', pluginId, decorator)
      },

      // 显示通知
      async showNotification(message, type = 'info') {
        const { useErrorNotification } = await import('../composables/useErrorNotification')
        const { showError } = useErrorNotification()
        showError(message, type)
      },
    },

    // ========== 歌词源 API ==========
    lyrics: {
      // 注册歌词源
      registerProvider(provider) {
        requirePermission(PluginPermission.LYRICS_PROVIDER, 'lyrics.registerProvider')
        if (!provider.id || !provider.name || !provider.search) {
          throw new Error('歌词源必须包含 id, name 和 search 方法')
        }
        manager.registerExtension('lyricsProviders', pluginId, provider)
        logger.info(`歌词源已注册: ${provider.name}`)
      },
    },

    // ========== 可视化 API ==========
    visualizer: {
      // 注册可视化效果
      register(visualizer) {
        requirePermission(PluginPermission.VISUALIZER, 'visualizer.register')
        if (!visualizer.id || !visualizer.name || !visualizer.render) {
          throw new Error('可视化效果必须包含 id, name 和 render 方法')
        }
        manager.registerExtension('visualizers', pluginId, visualizer)
        logger.info(`可视化效果已注册: ${visualizer.name}`)
      },
    },

    // ========== 命令 API ==========
    commands: {
      // 注册命令
      register(command) {
        if (!command.id || !command.name || !command.execute) {
          throw new Error('命令必须包含 id, name 和 execute 方法')
        }
        manager.registerExtension('commands', pluginId, command)
      },

      // 执行命令
      async execute(commandId) {
        const commands = manager.getExtensions('commands')
        const command = commands.find(c => c.id === commandId)
        if (command) {
          await command.execute()
        }
      },
    },

    // ========== 存储 API ==========
    storage: {
      get(key, defaultValue = null) {
        requirePermission(PluginPermission.STORAGE, 'storage.get')
        const storage = manager.getStorage(pluginId)
        return storage[key] ?? defaultValue
      },

      set(key, value) {
        requirePermission(PluginPermission.STORAGE, 'storage.set')
        const storage = manager.getStorage(pluginId)
        storage[key] = value
      },

      remove(key) {
        requirePermission(PluginPermission.STORAGE, 'storage.remove')
        const storage = manager.getStorage(pluginId)
        delete storage[key]
      },

      getAll() {
        requirePermission(PluginPermission.STORAGE, 'storage.getAll')
        return { ...manager.getStorage(pluginId) }
      },
    },

    // ========== 事件 API ==========
    events: {
      // 监听事件
      on(event, callback) {
        manager.on(event, pluginId, callback)
      },

      // 取消监听
      off(event, callback) {
        manager.off(event, pluginId, callback)
      },

      // 触发事件（仅限插件间通信）
      emit(event, data) {
        manager.emit(`plugin:${pluginId}:${event}`, data)
      },
    },

    // ========== 网络 API ==========
    network: {
      async fetch(url, options = {}) {
        requirePermission(PluginPermission.NETWORK, 'network.fetch')
        
        // 安全检查：只允许 HTTPS
        if (!url.startsWith('https://')) {
          throw new Error('只允许 HTTPS 请求')
        }

        try {
          const response = await fetch(url, {
            ...options,
            headers: {
              ...options.headers,
              'X-Plugin-Id': pluginId,
            },
          })
          return response
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 网络请求失败:`, error)
          throw error
        }
      },
    },
  }
}
