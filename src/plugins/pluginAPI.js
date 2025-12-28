/**
 * 插件 API
 * 为插件提供安全的接口访问应用功能
 */

import { computed, readonly } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { writeFile } from '@tauri-apps/plugin-fs'
import { PluginPermission } from './pluginManager'
import logger from '../utils/logger'
// 静态导入 stores 和工具类
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import { useMusicLibraryStore } from '../stores/musicLibrary'
import { useThemeStore } from '../stores/theme'
import { useErrorNotification } from '../composables/useErrorNotification'
import FileUtils from '../utils/fileUtils'
import { LyricsParser } from '../utils/lyricsParser'

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

  // 延迟初始化 stores（避免在 Pinia 未初始化时调用）
  let playerStore = null
  let configStore = null
  let musicLibraryStore = null
  let themeStore = null

  const getPlayerStore = () => {
    if (!playerStore) {
      playerStore = usePlayerStore()
    }
    return playerStore
  }

  const getConfigStore = () => {
    if (!configStore) {
      configStore = useConfigStore()
    }
    return configStore
  }

  const getMusicLibraryStore = () => {
    if (!musicLibraryStore) {
      musicLibraryStore = useMusicLibraryStore()
    }
    return musicLibraryStore
  }

  const getThemeStore = () => {
    if (!themeStore) {
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
      getState() {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getState')
        const store = getPlayerStore()
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
        const store = getPlayerStore()
        
        // 如果 store 里有歌词且格式正确（有 texts 数组），直接返回
        if (store.lyrics && store.lyrics.length > 0 && store.lyrics[0].texts) {
          return [...store.lyrics]
        }
        
        // 否则使用 LyricsParser 重新解析
        if (store.currentTrack?.path) {
          try {
            const lyricsPath = await FileUtils.findLyricsFile(store.currentTrack.path)
            if (lyricsPath) {
              const content = await FileUtils.readFile(lyricsPath)
              const ext = FileUtils.getFileExtension(lyricsPath)
              return await LyricsParser.parseAsync(content, ext)
            }
          } catch (e) {
            logger.debug(`[Plugin:${pluginId}] 加载歌词失败:`, e)
          }
        }
        
        return null
      },

      // 获取当前歌词索引
      getCurrentLyricIndex() {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getCurrentLyricIndex')
        const store = getPlayerStore()
        
        // 如果 store 里有索引，直接返回
        if (store.currentLyricIndex >= 0) {
          return store.currentLyricIndex
        }
        
        return -1
      },

      // 播放控制
      play() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.play')
        const store = getPlayerStore()
        store.play()
      },

      pause() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.pause')
        const store = getPlayerStore()
        store.pause()
      },

      togglePlay() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.togglePlay')
        const store = getPlayerStore()
        store.togglePlay()
      },

      async next() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.next')
        const store = getPlayerStore()
        await store.nextTrack()
      },

      async previous() {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.previous')
        const store = getPlayerStore()
        await store.previousTrack()
      },

      seek(time) {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.seek')
        const store = getPlayerStore()
        store.seek(time)
      },

      setVolume(volume) {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.setVolume')
        const store = getPlayerStore()
        store.setVolume(volume)
      },

      // 设置歌词（供歌词源插件使用）
      setLyrics(lyrics) {
        requirePermission(PluginPermission.LYRICS_PROVIDER, 'player.setLyrics')
        const store = getPlayerStore()
        store.lyrics = lyrics
      },
    },

    // ========== 音乐库 API ==========
    library: {
      getPlaylists() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getPlaylists')
        const store = getMusicLibraryStore()
        return store.playlists ? [...store.playlists] : []
      },

      getCurrentPlaylist() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getCurrentPlaylist')
        const store = getMusicLibraryStore()
        return store.currentPlaylist ? { ...store.currentPlaylist } : null
      },

      getTracks() {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getTracks')
        const store = getPlayerStore()
        return store.playlist ? [...store.playlist] : []
      },
    },

    // ========== 主题 API ==========
    theme: {
      getCurrent() {
        const store = getThemeStore()
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

      /**
       * 获取 CSS 变量值
       * @param {string} name - CSS 变量名（不含 --）
       * @returns {string} CSS 变量值
       */
      getCSSVariable(name) {
        const root = document.documentElement
        const varName = name.startsWith('--') ? name : `--${name}`
        return getComputedStyle(root).getPropertyValue(varName).trim()
      },

      /**
       * 获取所有主题颜色
       * @returns {Object} 主题颜色对象
       */
      getAllColors() {
        const root = document.documentElement
        const style = getComputedStyle(root)
        
        // Material Design 3 常用颜色变量
        const colorVars = [
          'md-sys-color-primary',
          'md-sys-color-on-primary',
          'md-sys-color-primary-container',
          'md-sys-color-on-primary-container',
          'md-sys-color-secondary',
          'md-sys-color-on-secondary',
          'md-sys-color-secondary-container',
          'md-sys-color-on-secondary-container',
          'md-sys-color-tertiary',
          'md-sys-color-on-tertiary',
          'md-sys-color-tertiary-container',
          'md-sys-color-on-tertiary-container',
          'md-sys-color-error',
          'md-sys-color-on-error',
          'md-sys-color-error-container',
          'md-sys-color-on-error-container',
          'md-sys-color-background',
          'md-sys-color-on-background',
          'md-sys-color-surface',
          'md-sys-color-on-surface',
          'md-sys-color-surface-variant',
          'md-sys-color-on-surface-variant',
          'md-sys-color-outline',
          'md-sys-color-outline-variant',
          'md-sys-color-shadow',
          'md-sys-color-scrim',
          'md-sys-color-inverse-surface',
          'md-sys-color-inverse-on-surface',
          'md-sys-color-inverse-primary',
          'md-sys-color-surface-container',
          'md-sys-color-surface-container-high',
          'md-sys-color-surface-container-highest',
          'md-sys-color-surface-container-low',
          'md-sys-color-surface-container-lowest',
        ]

        const colors = {}
        for (const varName of colorVars) {
          const value = style.getPropertyValue(`--${varName}`).trim()
          if (value) {
            // 转换为驼峰命名
            const key = varName.replace(/-([a-z])/g, (_, c) => c.toUpperCase())
            colors[key] = value
          }
        }
        return colors
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

      /**
       * 注册操作按钮（显示在歌词区域）
       * @param {Object} button - 按钮配置
       * @param {string} button.id - 唯一标识
       * @param {string} button.name - 显示名称（tooltip）
       * @param {string} button.icon - Material Symbols 图标名称
       * @param {Function} button.action - 点击时执行的函数
       * @param {string} [button.location] - 显示位置，默认 'lyrics'
       */
      registerActionButton(button) {
        if (!button.id || !button.name || !button.icon || !button.action) {
          throw new Error('按钮必须包含 id, name, icon 和 action')
        }
        manager.registerExtension('actionButtons', pluginId, {
          ...button,
          location: button.location || 'lyrics',
        })
        logger.info(`操作按钮已注册: ${button.name}`)
      },

      /**
       * 取消注册操作按钮
       * @param {string} buttonId - 按钮 ID
       */
      unregisterActionButton(buttonId) {
        const buttons = manager.extensions.actionButtons
        const index = buttons.findIndex(b => b.id === buttonId && b.pluginId === pluginId)
        if (index > -1) {
          buttons.splice(index, 1)
          logger.info(`操作按钮已取消: ${buttonId}`)
        }
      },

      // 显示通知
      showNotification(message, type = 'info') {
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

    // ========== 快捷键 API ==========
    shortcuts: {
      /**
       * 注册快捷键
       * @param {Object} shortcut - 快捷键配置
       * @param {string} shortcut.id - 唯一标识
       * @param {string} shortcut.name - 显示名称
       * @param {string} shortcut.key - 快捷键组合，如 'Ctrl+Shift+S', 'Alt+C'
       * @param {Function} shortcut.action - 触发时执行的函数
       * @param {string} [shortcut.description] - 描述
       */
      register(shortcut) {
        if (!shortcut.id || !shortcut.name || !shortcut.key || !shortcut.action) {
          throw new Error('快捷键必须包含 id, name, key 和 action')
        }
        
        // 标准化快捷键格式
        const normalizedKey = shortcut.key
          .toLowerCase()
          .split('+')
          .map(k => k.trim())
          .sort((a, b) => {
            // 修饰键排在前面：ctrl < alt < shift < meta < 其他键
            const order = { ctrl: 0, alt: 1, shift: 2, meta: 3 }
            return (order[a] ?? 4) - (order[b] ?? 4)
          })
          .join('+')
        
        manager.registerExtension('shortcuts', pluginId, {
          ...shortcut,
          key: normalizedKey,
          action: shortcut.action,
        })
        logger.info(`快捷键已注册: ${shortcut.name} (${shortcut.key})`)
      },

      /**
       * 取消注册快捷键
       * @param {string} shortcutId - 快捷键 ID
       */
      unregister(shortcutId) {
        const shortcuts = manager.extensions.shortcuts
        const index = shortcuts.findIndex(s => s.id === shortcutId && s.pluginId === pluginId)
        if (index > -1) {
          shortcuts.splice(index, 1)
          logger.info(`快捷键已取消: ${shortcutId}`)
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

    // ========== 工具 API ==========
    utils: {
      /**
       * 创建 Canvas
       * @param {number} width - 宽度
       * @param {number} height - 高度
       * @returns {{ canvas: HTMLCanvasElement, ctx: CanvasRenderingContext2D }}
       */
      createCanvas(width, height) {
        const canvas = document.createElement('canvas')
        canvas.width = width
        canvas.height = height
        const ctx = canvas.getContext('2d')
        return { canvas, ctx }
      },

      /**
       * Canvas 转 Blob
       * @param {HTMLCanvasElement} canvas - Canvas 元素
       * @param {string} type - MIME 类型，默认 'image/png'
       * @param {number} quality - 质量 0-1，仅对 jpeg 有效
       * @returns {Promise<Blob>}
       */
      canvasToBlob(canvas, type = 'image/png', quality = 0.92) {
        return new Promise((resolve, reject) => {
          canvas.toBlob(
            (blob) => {
              if (blob) {
                resolve(blob)
              } else {
                reject(new Error('Canvas 转换 Blob 失败'))
              }
            },
            type,
            quality
          )
        })
      },

      /**
       * Canvas 转 DataURL (base64)
       * @param {HTMLCanvasElement} canvas - Canvas 元素
       * @param {string} type - MIME 类型，默认 'image/png'
       * @param {number} quality - 质量 0-1，仅对 jpeg 有效
       * @returns {string}
       */
      canvasToDataURL(canvas, type = 'image/png', quality = 0.92) {
        return canvas.toDataURL(type, quality)
      },

      /**
       * 加载图片
       * @param {string} src - 图片 URL 或 base64
       * @returns {Promise<HTMLImageElement>}
       */
      loadImage(src) {
        return new Promise((resolve, reject) => {
          const img = new Image()
          img.crossOrigin = 'anonymous'
          img.onload = () => resolve(img)
          img.onerror = (e) => reject(new Error(`图片加载失败: ${e.message || src}`))
          img.src = src
        })
      },

      /**
       * Blob 转 ArrayBuffer
       * @param {Blob} blob
       * @returns {Promise<ArrayBuffer>}
       */
      blobToArrayBuffer(blob) {
        return blob.arrayBuffer()
      },

      /**
       * DataURL 转 Blob
       * @param {string} dataURL
       * @returns {Blob}
       */
      dataURLToBlob(dataURL) {
        const arr = dataURL.split(',')
        const mime = arr[0].match(/:(.*?);/)[1]
        const bstr = atob(arr[1])
        let n = bstr.length
        const u8arr = new Uint8Array(n)
        while (n--) {
          u8arr[n] = bstr.charCodeAt(n)
        }
        return new Blob([u8arr], { type: mime })
      },

      /**
       * 格式化时间
       * @param {number} seconds - 秒数
       * @returns {string} 格式化后的时间 (mm:ss 或 hh:mm:ss)
       */
      formatTime(seconds) {
        if (isNaN(seconds) || !isFinite(seconds)) return '0:00'
        const hours = Math.floor(seconds / 3600)
        const minutes = Math.floor((seconds % 3600) / 60)
        const secs = Math.floor(seconds % 60)
        if (hours > 0) {
          return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
        }
        return `${minutes}:${secs.toString().padStart(2, '0')}`
      },

      /**
       * 生成唯一 ID
       * @returns {string}
       */
      generateId() {
        return `${pluginId}-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
      },
    },

    // ========== 文件 API ==========
    file: {
      /**
       * 保存文件（弹出保存对话框）
       * @param {Blob|Uint8Array} data - 文件数据
       * @param {Object} options - 选项
       * @param {string} options.defaultName - 默认文件名
       * @param {Array<{name: string, extensions: string[]}>} options.filters - 文件类型过滤
       * @returns {Promise<string|null>} 保存的文件路径，取消返回 null
       */
      async saveAs(data, options = {}) {
        requirePermission(PluginPermission.STORAGE, 'file.saveAs')
        
        try {
          const filePath = await save({
            defaultPath: options.defaultName,
            filters: options.filters || [
              { name: '所有文件', extensions: ['*'] }
            ],
            title: options.title || '保存文件',
          })

          if (!filePath) {
            return null // 用户取消
          }

          // 转换数据格式
          let fileData
          if (data instanceof Blob) {
            const buffer = await data.arrayBuffer()
            fileData = new Uint8Array(buffer)
          } else if (data instanceof Uint8Array) {
            fileData = data
          } else if (typeof data === 'string') {
            fileData = new TextEncoder().encode(data)
          } else {
            throw new Error('不支持的数据类型')
          }

          await writeFile(filePath, fileData)
          logger.info(`[Plugin:${pluginId}] 文件已保存: ${filePath}`)
          return filePath
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 保存文件失败:`, error)
          throw error
        }
      },

      /**
       * 保存图片文件到程序 screenshots 目录
       * @param {HTMLCanvasElement|Blob|string} image - Canvas、Blob 或 DataURL
       * @param {string} defaultName - 默认文件名
       * @param {string} format - 图片格式 'png' | 'jpeg' | 'webp'
       * @returns {Promise<string|null>} 保存的文件路径
       */
      async saveImage(image, defaultName = 'image.png', format = 'png') {
        requirePermission(PluginPermission.STORAGE, 'file.saveImage')

        const mimeType = `image/${format === 'jpg' ? 'jpeg' : format}`
        let blob

        if (image instanceof HTMLCanvasElement) {
          blob = await new Promise((resolve) => {
            image.toBlob(resolve, mimeType, 0.92)
          })
        } else if (image instanceof Blob) {
          blob = image
        } else if (typeof image === 'string' && image.startsWith('data:')) {
          // DataURL
          const arr = image.split(',')
          const bstr = atob(arr[1])
          let n = bstr.length
          const u8arr = new Uint8Array(n)
          while (n--) {
            u8arr[n] = bstr.charCodeAt(n)
          }
          blob = new Blob([u8arr], { type: mimeType })
        } else {
          throw new Error('不支持的图片格式')
        }

        // 转换为 Uint8Array
        const arrayBuffer = await blob.arrayBuffer()
        const data = Array.from(new Uint8Array(arrayBuffer))
        
        // 调用后端保存
        const filePath = await invoke('save_screenshot', { filename: defaultName, data })
        logger.info(`[Plugin:${pluginId}] 图片已保存: ${filePath}`)
        return filePath
      },

      /**
       * 打开截图目录
       */
      async openScreenshotsDirectory() {
        await invoke('open_screenshots_directory')
      },
    },

    // ========== 剪贴板 API ==========
    clipboard: {
      /**
       * 复制图片到剪贴板
       * @param {HTMLCanvasElement|Blob|string} image - Canvas、Blob 或 DataURL
       * @returns {Promise<void>}
       */
      async writeImage(image) {
        requirePermission(PluginPermission.STORAGE, 'clipboard.writeImage')

        try {
          let blob

          if (image instanceof HTMLCanvasElement) {
            // Canvas 转 Blob
            blob = await new Promise((resolve) => {
              image.toBlob(resolve, 'image/png')
            })
          } else if (image instanceof Blob) {
            blob = image
          } else if (typeof image === 'string' && image.startsWith('data:')) {
            // DataURL 转 Blob
            const arr = image.split(',')
            const mime = arr[0].match(/:(.*?);/)[1]
            const bstr = atob(arr[1])
            let n = bstr.length
            const u8arr = new Uint8Array(n)
            while (n--) {
              u8arr[n] = bstr.charCodeAt(n)
            }
            blob = new Blob([u8arr], { type: mime })
          } else {
            throw new Error('不支持的图片格式')
          }

          // 使用浏览器原生 Clipboard API
          await navigator.clipboard.write([
            new ClipboardItem({ [blob.type]: blob })
          ])
          logger.info(`[Plugin:${pluginId}] 图片已复制到剪贴板`)
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 复制图片失败:`, error)
          throw error
        }
      },

      /**
       * 复制文本到剪贴板
       * @param {string} text - 文本内容
       * @returns {Promise<void>}
       */
      async writeText(text) {
        requirePermission(PluginPermission.STORAGE, 'clipboard.writeText')

        try {
          await navigator.clipboard.writeText(text)
          logger.info(`[Plugin:${pluginId}] 文本已复制到剪贴板`)
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 复制文本失败:`, error)
          throw error
        }
      },
    },
  }
}
