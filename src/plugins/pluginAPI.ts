/**
 * 插件 API
 * 为插件提供安全的接口访问应用功能
 */

import { readonly } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { writeFile, readFile } from '@tauri-apps/plugin-fs'
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import {
  assertPluginEventSubscriptionAllowed,
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
} from './pluginTypes'
import { permissionForAction } from './apiRegistry'
import type { PluginManager } from './pluginManager'
import logger from '../utils/logger'
import { usePlayerStore } from '../stores/player'
import { useMusicLibraryStore } from '../stores/musicLibrary'
import { useThemeStore } from '../stores/theme'
import { useErrorNotification } from '../composables/useErrorNotification'
import FileUtils from '../utils/fileUtils'
import { formatTime } from '../utils/format'
import { findLyricIndex } from '../utils/lyricsParser'

/**
 * 插件存储快照:只保留可结构化克隆的纯数据
 *
 * storage 底层是 Vue reactive 代理,`{ ...storage }` 的浅展开对嵌套对象
 * 仍然会拿到 reactive 代理;代理对象在 postMessage / structuredClone 下行为
 * 不稳定,且插件拿到后可直接改动宿主状态。这里统一转成纯值快照,
 * 顺带过滤掉函数等不可克隆的值(防御性:任何来源污染都不应让整份镜像报废)。
 */
function toCloneableSnapshot(source: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(source)) {
    if (typeof value === 'function' || typeof value === 'symbol') continue
    try {
      out[key] = JSON.parse(JSON.stringify(value ?? null))
    } catch {
      // 循环引用 / BigInt 等无法 JSON 化的值:直接跳过,不影响其余键
      logger.warn(`插件存储键 ${key} 无法序列化,快照中已跳过`)
    }
  }
  return out
}

/** Canvas 转 Blob 的 Promise 包装
 * @param errorMessage 转换失败时抛出的错误文案(保持各调用方原有文案)
 */
function canvasToBlob(
  canvas: HTMLCanvasElement,
  type = 'image/png',
  quality = 0.92,
  errorMessage = 'Canvas 转换 Blob 失败',
): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        if (blob) resolve(blob)
        else reject(new Error(errorMessage))
      },
      type,
      quality,
    )
  })
}

/**
 * 解析 data URL 并解码为 Blob
 * @param options.mimeType 显式指定 MIME 类型(跳过 dataURL 头部解析,如 saveImage 的格式参数)
 * @param options.fallbackMime dataURL 头部解析失败时使用的默认 MIME 类型
 */
function dataURLToBlob(
  dataURL: string,
  options: { mimeType?: string; fallbackMime?: string } = {},
): Blob {
  const arr = dataURL.split(',')
  let mime: string
  if (options.mimeType) {
    mime = options.mimeType
  } else {
    const mimeMatch = arr[0]!.match(/:(.*?);/)
    mime = mimeMatch ? mimeMatch[1]! : (options.fallbackMime ?? 'application/octet-stream')
  }
  const bstr = atob(arr[1]!)
  let n = bstr.length
  const u8arr = new Uint8Array(n)
  while (n--) {
    u8arr[n] = bstr.charCodeAt(n)
  }
  return new Blob([u8arr], { type: mime })
}

/**
 * 创建插件 API
 */
export function createPluginAPI(
  pluginId: string,
  permissions: PluginPermissionType[],
  manager: PluginManager,
): PluginAPI {
  const hasPermission = (permission: PluginPermissionType): boolean =>
    permissions.includes(permission)

  const requirePermission = (permission: PluginPermissionType, action: string): void => {
    if (!hasPermission(permission)) {
      throw new Error(`插件 ${pluginId} 没有 ${permission} 权限，无法执行 ${action}`)
    }
  }

  /** 按动作名查 apiRegistry 统一校验;null 表示该动作无需权限 */
  const requireAction = (action: string): void => {
    const permission = permissionForAction(action)
    if (permission !== null) {
      requirePermission(permission, action)
    }
  }

  // 歌词行格式转换辅助函数
  const convertLyricLine = (line: {
    time: number
    texts?: string[]
    text?: string
    karaoke?: unknown
    words?: unknown[]
  }): LyricLine => {
    let textArray: { text: string }[] = []
    if (line.texts && Array.isArray(line.texts) && line.texts.length > 0) {
      textArray = line.texts.map((t) => ({ text: typeof t === 'string' ? t : String(t) }))
    } else if (line.text) {
      textArray = [{ text: line.text }]
    }
    return {
      time: line.time,
      texts: textArray,
      text: line.text,
      karaoke: line.karaoke,
      words: line.words,
    } as LyricLine
  }

  // 延迟初始化 stores
  let playerStore: ReturnType<typeof usePlayerStore> | null = null
  let musicLibraryStore: ReturnType<typeof useMusicLibraryStore> | null = null
  let themeStore: ReturnType<typeof useThemeStore> | null = null

  const getPlayerStore = () => {
    if (!playerStore) playerStore = usePlayerStore()
    return playerStore
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
        requireAction('player.getState')
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
        requireAction('player.getLyrics')
        const store = getPlayerStore()

        // 如果 store 中已有歌词，直接返回
        if (store.lyrics && store.lyrics.length > 0) {
          return store.lyrics.map(convertLyricLine)
        }

        // 如果没有歌词但有当前歌曲，尝试加载歌词
        if (store.currentTrack?.path) {
          // 先检查歌词文件是否存在
          try {
            const lyricsPath = await FileUtils.findLyricsFile(store.currentTrack.path)

            if (!lyricsPath) {
              return null
            }
          } catch (e) {
            logger.error(`[Plugin:${pluginId}] 检查歌词文件失败:`, e)
            return null
          }

          try {
            // 先尝试触发 store 的歌词加载
            await store.loadLyrics(store.currentTrack.path)

            // 重试机制：最多等待 1 秒，每 100ms 检查一次
            for (let i = 0; i < 10; i++) {
              if (store.lyrics && store.lyrics.length > 0) {
                return store.lyrics.map(convertLyricLine)
              }
              await new Promise((resolve) => setTimeout(resolve, 100))
            }
          } catch (e) {
            logger.error(`[Plugin:${pluginId}] 加载歌词失败:`, e)
          }
        }

        return null
      },

      getCurrentLyricIndex(): number {
        requireAction('player.getCurrentLyricIndex')
        const store = getPlayerStore()

        // 如果没有歌词，返回 -1
        if (!store.lyrics || store.lyrics.length === 0) {
          return -1
        }

        // 应用歌词偏移(额外 +0.05s 提前量,保持既有插件行为)
        const offset = store.lyricsOffset || 0
        const adjustedTime = store.currentTime + 0.05 - offset

        return findLyricIndex(store.lyrics, adjustedTime)
      },

      play(): void {
        requireAction('player.play')
        getPlayerStore().play()
      },

      pause(): void {
        requireAction('player.pause')
        getPlayerStore().pause()
      },

      togglePlay(): void {
        requireAction('player.togglePlay')
        getPlayerStore().togglePlay()
      },

      async next(): Promise<void> {
        requireAction('player.next')
        await getPlayerStore().nextTrack()
      },

      async previous(): Promise<void> {
        requireAction('player.previous')
        await getPlayerStore().previousTrack()
      },

      seek(time: number): void {
        requireAction('player.seek')
        getPlayerStore().seek(time)
      },

      setVolume(volume: number): void {
        requireAction('player.setVolume')
        getPlayerStore().setVolume(volume)
      },

      setLyrics(lyrics: LyricLine[]): void {
        requireAction('player.setLyrics')
        // 转换为 store 的格式 (pluginAPI LyricLine.texts 是 {text,translation?}[],
        // store 期望 @/types LyricLine.texts 为 string[]
        const store = getPlayerStore()
        const storeLyrics = lyrics.map((line) => ({
          time: line.time,
          texts: line.texts?.map((t) => t.text) || [],
        }))

        store.lyrics = storeLyrics
      },

      async getCoverPath(): Promise<string | null> {
        requireAction('player.getCoverPath')
        const store = getPlayerStore()

        // 先检查 store 中是否已有 coverPath（可能已异步加载完成）
        if (store.currentTrack?.coverPath) {
          return store.currentTrack.coverPath
        }

        // store 中没有，直接调用后端获取（不依赖 store 的异步加载时序）
        const trackPath = store.currentTrack?.path
        if (!trackPath) return null

        try {
          return await invoke<string | null>('get_track_cover_path', { path: trackPath })
        } catch (e) {
          logger.error(`[Plugin:${pluginId}] 获取封面路径失败:`, e)
          return null
        }
      },
    },

    // ========== 音乐库 API ==========
    library: {
      getPlaylists(): Playlist[] {
        requireAction('library.getPlaylists')
        const store = getMusicLibraryStore()
        if (!store.playlists) return []
        return store.playlists.map((p) => ({
          id: p.name, // 使用 name 作为 id
          name: p.name,
          tracks: p.files?.map((f) => ({ ...f })) || [],
        }))
      },

      getCurrentPlaylist(): Playlist | null {
        requireAction('library.getCurrentPlaylist')
        const store = getMusicLibraryStore()
        if (!store.currentPlaylist) return null
        return {
          id: store.currentPlaylist.name,
          name: store.currentPlaylist.name,
          tracks: store.currentPlaylist.files?.map((f) => ({ ...f })) || [],
        }
      },

      getTracks(): Track[] {
        requireAction('library.getTracks')
        const store = getPlayerStore()
        return store.playlist ? [...store.playlist] : []
      },
    },

    // ========== 主题 API ==========
    theme: {
      getCurrent(): ThemeInfo {
        requireAction('theme.getCurrent')
        const store = getThemeStore()
        return {
          preference: store.themePreference,
          isDark: store.isDark,
          primaryColor: store.primaryColor,
        }
      },

      async setColors(colors: Record<string, string>): Promise<void> {
        requireAction('theme.setColors')
        const root = document.documentElement
        for (const [key, value] of Object.entries(colors)) {
          root.style.setProperty(`--plugin-${pluginId}-${key}`, value)
        }
      },

      getCSSVariable(name: string): string {
        // 读取仅开放 MD3 设计令牌,避免插件枚举任意 CSS 自定义属性(如 --plugin-* 注入面)
        requireAction('theme.getCSSVariable')
        const normalized = name.startsWith('--') ? name : `--${name}`
        if (!normalized.startsWith('--md-sys-')) {
          throw new Error(`theme.getCSSVariable 仅允许读取 --md-sys-* 变量,收到: ${name}`)
        }
        const root = document.documentElement
        const varName = name.startsWith('--') ? name : `--${name}`
        return getComputedStyle(root).getPropertyValue(varName).trim()
      },

      getAllColors(): Record<string, string> {
        requireAction('theme.getAllColors')
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
        requireAction('ui.registerSettingsPanel')
        manager.registerExtension('settingsPanels', pluginId, panel)
      },

      registerMenuItem(item: MenuItem): void {
        requireAction('ui.registerMenuItem')
        manager.registerExtension('menuItems', pluginId, item)
      },

      registerPlayerDecorator(decorator: PlayerDecorator): void {
        requireAction('ui.registerPlayerDecorator')
        manager.registerExtension('playerDecorators', pluginId, decorator)
      },

      registerActionButton(button: ActionButton): void {
        requireAction('ui.registerActionButton')
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
        requireAction('ui.unregisterActionButton')
        const buttons = manager.extensions.actionButtons
        const index = buttons.findIndex(
          (b: ActionButton & { pluginId: string }) => b.id === buttonId && b.pluginId === pluginId,
        )
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
        requireAction('lyrics.registerProvider')
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
        requireAction('visualizer.register')
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
        requireAction('commands.register')
        if (!command.id || !command.name || !command.execute) {
          throw new Error('命令必须包含 id, name 和 execute 方法')
        }
        manager.registerExtension('commands', pluginId, command)
      },

      async execute(commandId: string): Promise<void> {
        // 仅允许执行本插件注册的命令:命令在注册插件自身的权限上下文中运行,
        // 跨插件执行等价于借道其他插件的权限提权
        const commands = manager.getExtensions('commands')
        const command = commands.find(
          (c: Command & { pluginId: string }) => c.id === commandId && c.pluginId === pluginId,
        )
        if (command) {
          await command.execute()
        }
      },
    },

    // ========== 快捷键 API ==========
    shortcuts: {
      register(shortcut: Shortcut): void {
        requireAction('shortcuts.register')
        if (!shortcut.id || !shortcut.name || !shortcut.key || !shortcut.action) {
          throw new Error('快捷键必须包含 id, name, key 和 action')
        }

        const normalizedKey = shortcut.key
          .toLowerCase()
          .split('+')
          .map((k) => k.trim())
          .sort((a, b) => {
            const order: Record<string, number> = { ctrl: 0, alt: 1, shift: 2, meta: 3 }
            return (order[a] ?? 4) - (order[b] ?? 4)
          })
          .join('+')

        // 冲突检测:同一按键组合已被其他快捷键占用时拒绝注册,
        // 防止后注册者静默遮蔽/劫持既有快捷键 (shortcutManager 取首个匹配)
        const conflicting = manager.extensions.shortcuts.find(
          (s: Shortcut & { pluginId: string }) => s.key === normalizedKey && s.id !== shortcut.id,
        )
        if (conflicting) {
          throw new Error(
            `快捷键 ${normalizedKey} 已被 ${conflicting.name} (${conflicting.pluginId}) 注册，无法重复绑定`,
          )
        }
        // 同 id 重复注册 (重复激活):替换旧条目而非追加
        const selfIndex = manager.extensions.shortcuts.findIndex(
          (s: Shortcut & { pluginId: string }) => s.id === shortcut.id && s.pluginId === pluginId,
        )
        if (selfIndex > -1) {
          manager.extensions.shortcuts.splice(selfIndex, 1)
        }

        manager.registerExtension('shortcuts', pluginId, {
          ...shortcut,
          key: normalizedKey,
        })
        logger.info(`快捷键已注册: ${shortcut.name} (${shortcut.key})`)
      },

      unregister(shortcutId: string): void {
        requireAction('shortcuts.unregister')
        const shortcuts = manager.extensions.shortcuts
        const index = shortcuts.findIndex(
          (s: Shortcut & { pluginId: string }) => s.id === shortcutId && s.pluginId === pluginId,
        )
        if (index > -1) {
          shortcuts.splice(index, 1)
          logger.info(`快捷键已取消: ${shortcutId}`)
        }
      },
    },

    // ========== 存储 API ==========
    storage: {
      get<T>(key: string, defaultValue: T | null = null): T {
        requireAction('storage.get')
        const storage = manager.getStorage(pluginId)
        return (storage[key] as T) ?? (defaultValue as T)
      },

      set<T>(key: string, value: T): void {
        requireAction('storage.set')
        const storage = manager.getStorage(pluginId)
        storage[key] = value
      },

      remove(key: string): void {
        requireAction('storage.remove')
        const storage = manager.getStorage(pluginId)
        delete storage[key]
      },

      getAll(): Record<string, unknown> {
        requireAction('storage.getAll')
        return toCloneableSnapshot({ ...manager.getStorage(pluginId) })
      },
    },

    // ========== 事件 API ==========
    events: {
      on(event: string, callback: EventCallback): void {
        // 白名单 + 权限校验 (player:* 事件载荷含曲目路径等敏感数据,见 pluginTypes)
        assertPluginEventSubscriptionAllowed(event, hasPermission, pluginId)
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
        requireAction('network.fetch')

        if (!url.startsWith('https://')) {
          throw new Error('只允许 HTTPS 请求')
        }

        try {
          const response = await tauriFetch(url, {
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
      createCanvas(
        width: number,
        height: number,
      ): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D | null } {
        const canvas = document.createElement('canvas')
        canvas.width = width
        canvas.height = height
        const ctx = canvas.getContext('2d')
        return { canvas, ctx }
      },

      canvasToBlob,

      canvasToDataURL(canvas: HTMLCanvasElement, type = 'image/png', quality = 0.92): string {
        return canvas.toDataURL(type, quality)
      },

      async loadImage(src: string): Promise<HTMLImageElement> {
        // 对于 http/data/asset URL，直接加载（外部图片）
        if (src.startsWith('http') || src.startsWith('data:') || src.startsWith('asset:')) {
          return new Promise((resolve, reject) => {
            const img = new Image()
            img.crossOrigin = 'anonymous'
            img.onload = () => resolve(img)
            img.onerror = (e) =>
              reject(new Error(`图片加载失败: ${(e as ErrorEvent).message || src}`))
            img.src = src
          })
        }

        // 对于本地文件路径，通过 readFile + Blob 加载
        // 避免 convertFileSrc + crossOrigin 导致的 CORS 问题（asset 协议不返回 CORS 头）
        // 同时 Blob URL 是同源的，不会 taint Canvas
        try {
          const data = await readFile(src)
          const blob = new Blob([data])
          const url = URL.createObjectURL(blob)
          return await new Promise((resolve, reject) => {
            const img = new Image()
            img.onload = () => {
              URL.revokeObjectURL(url)
              resolve(img)
            }
            img.onerror = (e) => {
              URL.revokeObjectURL(url)
              reject(new Error(`图片加载失败: ${(e as ErrorEvent).message || src}`))
            }
            img.src = url
          })
        } catch (e) {
          throw new Error(`图片加载失败: ${e instanceof Error ? e.message : String(e)}`)
        }
      },

      blobToArrayBuffer(blob: Blob): Promise<ArrayBuffer> {
        return blob.arrayBuffer()
      },

      dataURLToBlob,

      formatTime(seconds: number): string {
        return formatTime(seconds)
      },

      generateId(): string {
        return `${pluginId}-${Date.now()}-${Math.random().toString(36).substring(2, 11)}`
      },
    },

    // ========== 文件 API ==========
    file: {
      async saveAs(
        data: Blob | Uint8Array | string,
        options: SaveAsOptions = {},
      ): Promise<string | null> {
        requireAction('file.saveAs')

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
        format = 'png',
      ): Promise<string | null> {
        requireAction('file.saveImage')

        const mimeType = `image/${format === 'jpg' ? 'jpeg' : format}`
        let blob: Blob

        if (image instanceof HTMLCanvasElement) {
          blob = await canvasToBlob(image, mimeType, 0.92, 'Canvas 转换失败')
        } else if (image instanceof Blob) {
          blob = image
        } else if (typeof image === 'string' && image.startsWith('data:')) {
          blob = dataURLToBlob(image, { mimeType })
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
        requireAction('clipboard.writeImage')

        try {
          let blob: Blob

          if (image instanceof HTMLCanvasElement) {
            blob = await canvasToBlob(image, 'image/png', 0.92, 'Canvas 转换失败')
          } else if (image instanceof Blob) {
            blob = image
          } else if (typeof image === 'string' && image.startsWith('data:')) {
            blob = dataURLToBlob(image, { fallbackMime: 'image/png' })
          } else {
            throw new Error('不支持的图片格式')
          }

          await navigator.clipboard.write([new ClipboardItem({ [blob.type]: blob })])
          logger.info(`[Plugin:${pluginId}] 图片已复制到剪贴板`)
        } catch (error) {
          logger.error(`[Plugin:${pluginId}] 复制图片失败:`, error)
          throw error
        }
      },

      async writeText(text: string): Promise<void> {
        requireAction('clipboard.writeText')

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
