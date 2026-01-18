/**
 * 插件 API
 * 为插件提供安全的接口访问应用功能
 */

import { readonly } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { writeFile } from '@tauri-apps/plugin-fs'
import {
  PluginPermission,
  type PluginAPI,
  type PluginPermissionType,
  type PlayerState,
  type LyricLine,
  type Playlist,
  type Track,
  type ThemeInfo,
  type SettingsPanel,
  type MenuItem,
  type PlayerDecorator,
  type ActionButton,
  type LyricsProvider,
  type Visualizer,
  type Command,
  type Shortcut,
  type SaveAsOptions,
  type EventCallback,
} from './pluginManager'
import type { PluginManager } from './pluginManager'
import logger from '../utils/logger'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import { useMusicLibraryStore } from '../stores/musicLibrary'
import { useThemeStore } from '../stores/theme'
import { useErrorNotification } from '../composables/useErrorNotification'
import FileUtils from '../utils/fileUtils'
import { LyricsParser } from '../utils/lyricsParser'

/**
 * 创建插件 API
 */
export function createPluginAPI(
  pluginId: string,
  permissions: PluginPermissionType[],
  manager: PluginManager
): PluginAPI {
  const hasPermission = (permission: PluginPermissionType): boolean =>
    permissions.includes(permission)

  const requirePermission = (permission: PluginPermissionType, action: string): void => {
    if (!hasPermission(permission)) {
      throw new Error(`插件 ${pluginId} 没有 ${permission} 权限，无法执行 ${action}`)
    }
  }

  // 延迟初始化 stores
  let playerStore: ReturnType<typeof usePlayerStore> | null = null
  let configStore: ReturnType<typeof useConfigStore> | null = null
  let musicLibraryStore: ReturnType<typeof useMusicLibraryStore> | null = null
  let themeStore: ReturnType<typeof useThemeStore> | null = null

  const getPlayerStore = () => {
    if (!playerStore) playerStore = usePlayerStore()
    return playerStore
  }

  const getConfigStore = () => {
    if (!configStore) configStore = useConfigStore()
    return configStore
  }

  const getMusicLibraryStore = () => {
    if (!musicLibraryStore) musicLibraryStore = useMusicLibraryStore()
    return musicLibraryStore
  }

  const getThemeStore = () => {
    if (!themeStore) themeStore = useThemeStore()
    return themeStore
  }

  return {
    pluginId,
    permissions: readonly(permissions) as readonly string[],

    // ========== 日志 ==========
    log: {
      info: (...args: unknown[]) => logger.info(`[Plugin:${pluginId}]`, ...args),
      warn: (...args: unknown[]) => logger.warn(`[Plugin:${pluginId}]`, ...args),
      error: (...args: unknown[]) => logger.error(`[Plugin:${pluginId}]`, ...args),
      debug: (...args: unknown[]) => logger.debug(`[Plugin:${pluginId}]`, ...args),
    },

    // ========== 播放器 API ==========
    player: {
      getState(): PlayerState {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getState')
        const store = getPlayerStore()
        return {
          currentTrack: store.currentTrack ? JSON.parse(JSON.stringify(store.currentTrack)) : null,
          isPlaying: store.isPlaying,
          currentTime: store.currentTime,
          duration: store.duration,
          volume: store.volume,
          repeatMode: store.repeatMode,
          isShuffle: store.isShuffle,
        }
      },

      async getLyrics(): Promise<LyricLine[] | null> {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getLyrics')
        const store = getPlayerStore()

        logger.debug(`[Plugin:${pluginId}] getLyrics 开始 - 当前歌曲:`, store.currentTrack?.path)
        logger.debug(`[Plugin:${pluginId}] getLyrics - store.lyrics 长度:`, store.lyrics?.length || 0)

        // 如果 store 中已有歌词，直接返回
        if (store.lyrics && store.lyrics.length > 0) {
          logger.debug(`[Plugin:${pluginId}] 使用 store 中的歌词`)
          return store.lyrics.map(line => {
            // 处理两种格式的歌词数据：
            // 1. 异步解析: { time, texts: string[] }
            // 2. 同步解析: { time, text: string }
            let textArray: { text: string }[] = []

            if (line.texts && Array.isArray(line.texts) && line.texts.length > 0) {
              // 异步解析的格式，texts 是字符串数组
              textArray = line.texts.map(t => ({ text: typeof t === 'string' ? t : String(t) }))
            } else if (line.text) {
              // 同步解析的格式，text 是单个字符串
              textArray = [{ text: line.text }]
            }

            return {
              time: line.time,
              texts: textArray,
              // 保留原始属性供插件使用
              text: line.text,
              karaoke: line.karaoke,
              words: line.words,
            }
          }) as LyricLine[]
        }

        // 如果没有歌词但有当前歌曲，尝试加载歌词
        if (store.currentTrack?.path) {
          logger.debug(`[Plugin:${pluginId}] 尝试加载歌词:`, store.currentTrack.path)

          // 先检查歌词文件是否存在
          try {
            const lyricsPath = await FileUtils.findLyricsFile(store.currentTrack.path)
            logger.debug(`[Plugin:${pluginId}] 歌词文件路径:`, lyricsPath)

            if (!lyricsPath) {
              logger.debug(`[Plugin:${pluginId}] 没有找到歌词文件`)
              return null
            }
          } catch (e) {
            logger.error(`[Plugin:${pluginId}] 检查歌词文件失败:`, e)
            return null
          }

          try {
            // 先尝试触发 store 的歌词加载
            await store.loadLyrics(store.currentTrack.path)
            logger.debug(`[Plugin:${pluginId}] loadLyrics 调用完成`)

            // 重试机制：最多等待 1 秒，每 100ms 检查一次
            for (let i = 0; i < 10; i++) {
              logger.debug(`[Plugin:${pluginId}] 检查歌词 (${i + 1}/10) - 长度:`, store.lyrics?.length || 0)
              if (store.lyrics && store.lyrics.length > 0) {
                logger.debug(`[Plugin:${pluginId}] 歌词加载成功`)
                return store.lyrics.map(line => {
                  let textArray: { text: string }[] = []

                  if (line.texts && Array.isArray(line.texts) && line.texts.length > 0) {
                    textArray = line.texts.map(t => ({ text: typeof t === 'string' ? t : String(t) }))
                  } else if (line.text) {
                    textArray = [{ text: line.text }]
                  }

                  return {
                    time: line.time,
                    texts: textArray,
                    text: line.text,
                    karaoke: line.karaoke,
                    words: line.words,
                  }
                }) as LyricLine[]
              }
              await new Promise(resolve => setTimeout(resolve, 100))
            }

            logger.debug(`[Plugin:${pluginId}] 歌词加载超时或无歌词文件`)
          } catch (e) {
            logger.error(`[Plugin:${pluginId}] 加载歌词失败:`, e)
          }
        } else {
          logger.debug(`[Plugin:${pluginId}] 没有当前歌曲`)
        }

        return null
      },

      getCurrentLyricIndex(): number {
        requirePermission(PluginPermission.PLAYER_READ, 'player.getCurrentLyricIndex')
        const store = getPlayerStore()

        // 如果没有歌词，返回 -1
        if (!store.lyrics || store.lyrics.length === 0) {
          return -1
        }

        // 应用歌词偏移
        const offset = store.lyricsOffset || 0
        const adjustedTime = store.currentTime + 0.05 - offset

        // 二分查找当前歌词索引（与 useLyrics.ts 中的逻辑一致）
        let l = 0, r = store.lyrics.length - 1, idx = -1
        while (l <= r) {
          const mid = (l + r) >> 1
          if (store.lyrics[mid].time <= adjustedTime) {
            idx = mid
            l = mid + 1
          } else {
            r = mid - 1
          }
        }

        return idx
      },

      play(): void {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.play')
        getPlayerStore().play()
      },

      pause(): void {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.pause')
        getPlayerStore().pause()
      },

      togglePlay(): void {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.togglePlay')
        getPlayerStore().togglePlay()
      },

      async next(): Promise<void> {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.next')
        await getPlayerStore().nextTrack()
      },

      async previous(): Promise<void> {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.previous')
        await getPlayerStore().previousTrack()
      },

      seek(time: number): void {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.seek')
        getPlayerStore().seek(time)
      },

      setVolume(volume: number): void {
        requirePermission(PluginPermission.PLAYER_CONTROL, 'player.setVolume')
        getPlayerStore().setVolume(volume)
      },

      setLyrics(lyrics: LyricLine[]): void {
        requirePermission(PluginPermission.LYRICS_PROVIDER, 'player.setLyrics')
        // 转换为 store 的格式
        const store = getPlayerStore()
        const storeLyrics = lyrics.map(line => ({
          time: line.time,
          texts: line.texts?.map(t => t.text) || [],
        }))
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        store.lyrics = storeLyrics as any
      },
    },

    // ========== 音乐库 API ==========
    library: {
      getPlaylists(): Playlist[] {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getPlaylists')
        const store = getMusicLibraryStore()
        if (!store.playlists) return []
        return store.playlists.map(p => ({
          id: p.name, // 使用 name 作为 id
          name: p.name,
          tracks: p.files?.map(f => ({ ...f })) || [],
        }))
      },

      getCurrentPlaylist(): Playlist | null {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getCurrentPlaylist')
        const store = getMusicLibraryStore()
        if (!store.currentPlaylist) return null
        return {
          id: store.currentPlaylist.name,
          name: store.currentPlaylist.name,
          tracks: store.currentPlaylist.files?.map(f => ({ ...f })) || [],
        }
      },

      getTracks(): Track[] {
        requirePermission(PluginPermission.LIBRARY_READ, 'library.getTracks')
        const store = getPlayerStore()
        return store.playlist ? [...store.playlist] : []
      },
    },

    // ========== 主题 API ==========
    theme: {
      getCurrent(): ThemeInfo {
        const store = getThemeStore()
        return {
          preference: store.themePreference,
          isDark: store.isDark,
          primaryColor: store.primaryColor,
        }
      },

      async setColors(colors: Record<string, string>): Promise<void> {
        requirePermission(PluginPermission.THEME, 'theme.setColors')
        const root = document.documentElement
        for (const [key, value] of Object.entries(colors)) {
          root.style.setProperty(`--plugin-${pluginId}-${key}`, value)
        }
      },

      getCSSVariable(name: string): string {
        const root = document.documentElement
        const varName = name.startsWith('--') ? name : `--${name}`
        return getComputedStyle(root).getPropertyValue(varName).trim()
      },

      getAllColors(): Record<string, string> {
        const root = document.documentElement
        const style = getComputedStyle(root)

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

        const colors: Record<string, string> = {}
        for (const varName of colorVars) {
          const value = style.getPropertyValue(`--${varName}`).trim()
          if (value) {
            const key = varName.replace(/-([a-z])/g, (_, c) => c.toUpperCase())
            colors[key] = value
          }
        }
        return colors
      },
    },

    // ========== UI 扩展 API ==========
    ui: {
      registerSettingsPanel(panel: SettingsPanel): void {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerSettingsPanel')
        manager.registerExtension('settingsPanels', pluginId, panel)
      },

      registerMenuItem(item: MenuItem): void {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerMenuItem')
        manager.registerExtension('menuItems', pluginId, item)
      },

      registerPlayerDecorator(decorator: PlayerDecorator): void {
        requirePermission(PluginPermission.UI_EXTEND, 'ui.registerPlayerDecorator')
        manager.registerExtension('playerDecorators', pluginId, decorator)
      },

      registerActionButton(button: ActionButton): void {
        if (!button.id || !button.name || !button.icon || !button.action) {
          throw new Error('按钮必须包含 id, name, icon 和 action')
        }
        manager.registerExtension('actionButtons', pluginId, {
          ...button,
          location: button.location || 'lyrics',
        })
        logger.info(`操作按钮已注册: ${button.name}`)
      },

      unregisterActionButton(buttonId: string): void {
        const buttons = manager.extensions.actionButtons
        const index = buttons.findIndex((b: ActionButton & { pluginId: string }) => b.id === buttonId && b.pluginId === pluginId)
        if (index > -1) {
          buttons.splice(index, 1)
          logger.info(`操作按钮已取消: ${buttonId}`)
        }
      },

      showNotification(message: string, type: 'error' | 'warning' | 'info' = 'info'): void {
        const { showError } = useErrorNotification()
        showError(message, type)
      },
    },

    // ========== 歌词源 API ==========
    lyrics: {
      registerProvider(provider: LyricsProvider): void {
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
      register(visualizer: Visualizer): void {
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
      register(command: Command): void {
        if (!command.id || !command.name || !command.execute) {
          throw new Error('命令必须包含 id, name 和 execute 方法')
        }
        manager.registerExtension('commands', pluginId, command)
      },

      async execute(commandId: string): Promise<void> {
        const commands = manager.getExtensions('commands')
        const command = commands.find((c: Command & { pluginId: string }) => c.id === commandId)
        if (command) {
          await command.execute()
        }
      },
    },

    // ========== 快捷键 API ==========
    shortcuts: {
      register(shortcut: Shortcut): void {
        if (!shortcut.id || !shortcut.name || !shortcut.key || !shortcut.action) {
          throw new Error('快捷键必须包含 id, name, key 和 action')
        }

        const normalizedKey = shortcut.key
          .toLowerCase()
          .split('+')
          .map(k => k.trim())
          .sort((a, b) => {
            const order: Record<string, number> = { ctrl: 0, alt: 1, shift: 2, meta: 3 }
            return (order[a] ?? 4) - (order[b] ?? 4)
          })
          .join('+')

        manager.registerExtension('shortcuts', pluginId, {
          ...shortcut,
          key: normalizedKey,
        })
        logger.info(`快捷键已注册: ${shortcut.name} (${shortcut.key})`)
      },

      unregister(shortcutId: string): void {
        const shortcuts = manager.extensions.shortcuts
        const index = shortcuts.findIndex((s: Shortcut & { pluginId: string }) => s.id === shortcutId && s.pluginId === pluginId)
        if (index > -1) {
          shortcuts.splice(index, 1)
          logger.info(`快捷键已取消: ${shortcutId}`)
        }
      },
    },

    // ========== 存储 API ==========
    storage: {
      get<T>(key: string, defaultValue: T | null = null): T {
        requirePermission(PluginPermission.STORAGE, 'storage.get')
        const storage = manager.getStorage(pluginId)
        return (storage[key] as T) ?? (defaultValue as T)
      },

      set<T>(key: string, value: T): void {
        requirePermission(PluginPermission.STORAGE, 'storage.set')
        const storage = manager.getStorage(pluginId)
        storage[key] = value
      },

      remove(key: string): void {
        requirePermission(PluginPermission.STORAGE, 'storage.remove')
        const storage = manager.getStorage(pluginId)
        delete storage[key]
      },

      getAll(): Record<string, unknown> {
        requirePermission(PluginPermission.STORAGE, 'storage.getAll')
        return { ...manager.getStorage(pluginId) }
      },
    },

    // ========== 事件 API ==========
    events: {
      on(event: string, callback: EventCallback): void {
        manager.on(event, pluginId, callback)
      },

      off(event: string, callback: EventCallback): void {
        manager.off(event, pluginId, callback)
      },

      emit(event: string, data?: unknown): void {
        manager.emit(`plugin:${pluginId}:${event}`, data)
      },
    },

    // ========== 网络 API ==========
    network: {
      async fetch(url: string, options: RequestInit = {}): Promise<Response> {
        requirePermission(PluginPermission.NETWORK, 'network.fetch')

        if (!url.startsWith('https://')) {
          throw new Error('只允许 HTTPS 请求')
        }

        try {
          const response = await fetch(url, {
            ...options,
            headers: {
              ...options.headers,
              'X-Plugin-Request': 'true', // 不暴露具体插件ID
            },
            redirect: 'manual', // 防止重定向绕过HTTPS检查
          })

          // 检查最终URL是否仍为HTTPS
          if (response.url && !response.url.startsWith('https://')) {
            throw new Error('请求被重定向到非HTTPS地址')
          }

          return response
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 网络请求失败:`, error)
          throw error
        }
      },
    },

    // ========== 工具 API ==========
    utils: {
      createCanvas(width: number, height: number): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D | null } {
        const canvas = document.createElement('canvas')
        canvas.width = width
        canvas.height = height
        const ctx = canvas.getContext('2d')
        return { canvas, ctx }
      },

      canvasToBlob(canvas: HTMLCanvasElement, type = 'image/png', quality = 0.92): Promise<Blob> {
        return new Promise((resolve, reject) => {
          canvas.toBlob(
            (blob) => {
              if (blob) resolve(blob)
              else reject(new Error('Canvas 转换 Blob 失败'))
            },
            type,
            quality
          )
        })
      },

      canvasToDataURL(canvas: HTMLCanvasElement, type = 'image/png', quality = 0.92): string {
        return canvas.toDataURL(type, quality)
      },

      loadImage(src: string): Promise<HTMLImageElement> {
        return new Promise((resolve, reject) => {
          const img = new Image()
          img.crossOrigin = 'anonymous'
          img.onload = () => resolve(img)
          img.onerror = (e) => reject(new Error(`图片加载失败: ${(e as ErrorEvent).message || src}`))
          img.src = src
        })
      },

      blobToArrayBuffer(blob: Blob): Promise<ArrayBuffer> {
        return blob.arrayBuffer()
      },

      dataURLToBlob(dataURL: string): Blob {
        const arr = dataURL.split(',')
        const mimeMatch = arr[0].match(/:(.*?);/)
        const mime = mimeMatch ? mimeMatch[1] : 'application/octet-stream'
        const bstr = atob(arr[1])
        let n = bstr.length
        const u8arr = new Uint8Array(n)
        while (n--) {
          u8arr[n] = bstr.charCodeAt(n)
        }
        return new Blob([u8arr], { type: mime })
      },

      formatTime(seconds: number): string {
        if (isNaN(seconds) || !isFinite(seconds)) return '0:00'
        const hours = Math.floor(seconds / 3600)
        const minutes = Math.floor((seconds % 3600) / 60)
        const secs = Math.floor(seconds % 60)
        if (hours > 0) {
          return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
        }
        return `${minutes}:${secs.toString().padStart(2, '0')}`
      },

      generateId(): string {
        return `${pluginId}-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
      },
    },

    // ========== 文件 API ==========
    file: {
      async saveAs(data: Blob | Uint8Array | string, options: SaveAsOptions = {}): Promise<string | null> {
        requirePermission(PluginPermission.STORAGE, 'file.saveAs')

        try {
          const filePath = await save({
            defaultPath: options.defaultName,
            filters: options.filters || [{ name: '所有文件', extensions: ['*'] }],
            title: options.title || '保存文件',
          })

          if (!filePath) return null

          let fileData: Uint8Array
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

      async saveImage(
        image: HTMLCanvasElement | Blob | string,
        defaultName = 'image.png',
        format = 'png'
      ): Promise<string | null> {
        requirePermission(PluginPermission.STORAGE, 'file.saveImage')

        const mimeType = `image/${format === 'jpg' ? 'jpeg' : format}`
        let blob: Blob

        if (image instanceof HTMLCanvasElement) {
          blob = await new Promise<Blob>((resolve, reject) => {
            image.toBlob((b) => {
              if (b) resolve(b)
              else reject(new Error('Canvas 转换失败'))
            }, mimeType, 0.92)
          })
        } else if (image instanceof Blob) {
          blob = image
        } else if (typeof image === 'string' && image.startsWith('data:')) {
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

        const arrayBuffer = await blob.arrayBuffer()
        const data = Array.from(new Uint8Array(arrayBuffer))

        const filePath = await invoke<string>('save_screenshot', { filename: defaultName, data })
        logger.info(`[Plugin:${pluginId}] 图片已保存: ${filePath}`)
        return filePath
      },

      async openScreenshotsDirectory(): Promise<void> {
        await invoke('open_screenshots_directory')
      },
    },

    // ========== 剪贴板 API ==========
    clipboard: {
      async writeImage(image: HTMLCanvasElement | Blob | string): Promise<void> {
        requirePermission(PluginPermission.STORAGE, 'clipboard.writeImage')

        try {
          let blob: Blob

          if (image instanceof HTMLCanvasElement) {
            blob = await new Promise<Blob>((resolve, reject) => {
              image.toBlob((b) => {
                if (b) resolve(b)
                else reject(new Error('Canvas 转换失败'))
              }, 'image/png')
            })
          } else if (image instanceof Blob) {
            blob = image
          } else if (typeof image === 'string' && image.startsWith('data:')) {
            const arr = image.split(',')
            const mimeMatch = arr[0].match(/:(.*?);/)
            const mime = mimeMatch ? mimeMatch[1] : 'image/png'
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

          await navigator.clipboard.write([
            new ClipboardItem({ [blob.type]: blob })
          ])
          logger.info(`[Plugin:${pluginId}] 图片已复制到剪贴板`)
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 复制图片失败:`, error)
          throw error
        }
      },

      async writeText(text: string): Promise<void> {
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
