/**
 * 插件管理器
 * 提供插件的加载、卸载、生命周期管理
 */

import { reactive, markRaw, watch, type WatchStopHandle } from 'vue'
import logger from '../utils/logger'
import { createPluginAPI } from './pluginAPI'
import { createPluginSandbox, type PluginSandbox } from './pluginSandbox'
import { usePlayerStore } from '../stores/player'

// 插件存储的 localStorage key 前缀
const STORAGE_PREFIX = 'mercurial-plugin-storage-'

// 插件状态
export const PluginState = {
  INACTIVE: 'inactive',
  ACTIVE: 'active',
  ERROR: 'error',
  DISABLED: 'disabled'
} as const

export type PluginStateType = typeof PluginState[keyof typeof PluginState]

// 插件权限
export const PluginPermission = {
  PLAYER_READ: 'player:read',       // 读取播放器状态
  PLAYER_CONTROL: 'player:control', // 控制播放器
  LIBRARY_READ: 'library:read',     // 读取音乐库
  LYRICS_PROVIDER: 'lyrics:provider', // 提供歌词源
  UI_EXTEND: 'ui:extend',           // 扩展 UI
  VISUALIZER: 'visualizer',         // 可视化效果
  THEME: 'theme',                   // 主题
  STORAGE: 'storage',               // 本地存储
  NETWORK: 'network',               // 网络请求
} as const

export type PluginPermissionType = typeof PluginPermission[keyof typeof PluginPermission]

// 插件 API 类型
export interface PluginAPI {
  pluginId: string
  permissions: readonly string[]
  log: {
    info: (...args: unknown[]) => void
    warn: (...args: unknown[]) => void
    error: (...args: unknown[]) => void
    debug: (...args: unknown[]) => void
  }
  player: {
    getState: () => PlayerState
    getLyrics: () => Promise<LyricLine[] | null>
    getCurrentLyricIndex: () => number
    play: () => void
    pause: () => void
    togglePlay: () => void
    next: () => Promise<void>
    previous: () => Promise<void>
    seek: (time: number) => void
    setVolume: (volume: number) => void
    setLyrics: (lyrics: LyricLine[]) => void
  }
  library: {
    getPlaylists: () => Playlist[]
    getCurrentPlaylist: () => Playlist | null
    getTracks: () => Track[]
  }
  theme: {
    getCurrent: () => ThemeInfo
    setColors: (colors: Record<string, string>) => Promise<void>
    getCSSVariable: (name: string) => string
    getAllColors: () => Record<string, string>
  }
  ui: {
    registerSettingsPanel: (panel: SettingsPanel) => void
    registerMenuItem: (item: MenuItem) => void
    registerPlayerDecorator: (decorator: PlayerDecorator) => void
    registerActionButton: (button: ActionButton) => void
    unregisterActionButton: (buttonId: string) => void
    showNotification: (message: string, type?: 'error' | 'warning' | 'info') => void
  }
  lyrics: {
    registerProvider: (provider: LyricsProvider) => void
  }
  visualizer: {
    register: (visualizer: Visualizer) => void
  }
  commands: {
    register: (command: Command) => void
    execute: (commandId: string) => Promise<void>
  }
  shortcuts: {
    register: (shortcut: Shortcut) => void
    unregister: (shortcutId: string) => void
  }
  storage: {
    get: <T>(key: string, defaultValue?: T) => T
    set: <T>(key: string, value: T) => void
    remove: (key: string) => void
    getAll: () => Record<string, unknown>
  }
  events: {
    on: (event: string, callback: EventCallback) => void
    off: (event: string, callback: EventCallback) => void
    emit: (event: string, data?: unknown) => void
  }
  network: {
    fetch: (url: string, options?: RequestInit) => Promise<Response>
  }
  utils: {
    createCanvas: (width: number, height: number) => { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D | null }
    canvasToBlob: (canvas: HTMLCanvasElement, type?: string, quality?: number) => Promise<Blob>
    canvasToDataURL: (canvas: HTMLCanvasElement, type?: string, quality?: number) => string
    loadImage: (src: string) => Promise<HTMLImageElement>
    blobToArrayBuffer: (blob: Blob) => Promise<ArrayBuffer>
    dataURLToBlob: (dataURL: string) => Blob
    formatTime: (seconds: number) => string
    generateId: () => string
  }
  file: {
    saveAs: (data: Blob | Uint8Array | string, options?: SaveAsOptions) => Promise<string | null>
    saveImage: (image: HTMLCanvasElement | Blob | string, defaultName?: string, format?: string) => Promise<string | null>
    openScreenshotsDirectory: () => Promise<void>
  }
  clipboard: {
    writeImage: (image: HTMLCanvasElement | Blob | string) => Promise<void>
    writeText: (text: string) => Promise<void>
  }
}

// 辅助类型定义
export interface PlayerState {
  currentTrack: Track | null
  isPlaying: boolean
  currentTime: number
  duration: number
  volume: number
  repeatMode: string
  isShuffle: boolean
}

export interface Track {
  path: string
  title?: string
  artist?: string
  album?: string
  duration?: number
  [key: string]: unknown
}

export interface LyricLine {
  time: number
  texts: { text: string; translation?: string }[]
  [key: string]: unknown
}

export interface Playlist {
  id: string
  name: string
  tracks?: Track[]
  [key: string]: unknown
}

export interface ThemeInfo {
  preference: string
  isDark: boolean
  primaryColor: string
}

export interface SettingsPanel {
  id: string
  name: string
  component: unknown
  [key: string]: unknown
}

export interface MenuItem {
  id: string
  name: string
  action: () => void
  [key: string]: unknown
}

export interface PlayerDecorator {
  id: string
  component: unknown
  [key: string]: unknown
}

export interface ActionButton {
  id: string
  name: string
  icon: string
  action: () => void
  location?: string
  [key: string]: unknown
}

export interface LyricsProvider {
  id: string
  name: string
  search: (query: LyricsSearchQuery) => Promise<LyricsSearchResult[]>
  [key: string]: unknown
}

export interface LyricsSearchQuery {
  title: string
  artist?: string
  album?: string
  duration?: number
}

export interface LyricsSearchResult {
  id: string
  title: string
  artist?: string
  lyrics?: string
  [key: string]: unknown
}

export interface Visualizer {
  id: string
  name: string
  render: (ctx: CanvasRenderingContext2D, data: Float32Array) => void
  [key: string]: unknown
}

export interface Command {
  id: string
  name: string
  execute: () => void | Promise<void>
  [key: string]: unknown
}

export interface Shortcut {
  id: string
  name: string
  key: string
  action: () => void
  description?: string
  [key: string]: unknown
}

export interface SaveAsOptions {
  defaultName?: string
  filters?: { name: string; extensions: string[] }[]
  title?: string
}

export type EventCallback = (data?: unknown) => void

// 插件主函数类型
export type PluginMainFunction = (api: PluginAPI) => Promise<PluginInstance> | PluginInstance

// 插件实例类型
export interface PluginInstance {
  activate?: () => void | Promise<void>
  deactivate?: () => void | Promise<void>
  [key: string]: unknown
}

// 插件定义类型
export interface PluginDefinition {
  id: string
  name: string
  version?: string
  author?: string
  description?: string
  permissions?: PluginPermissionType[]
  main: PluginMainFunction
}

// 内置插件定义类型（main 可以是简化形式）
export interface BuiltinPluginDefinition {
  id: string
  name: string
  version?: string
  author?: string
  description?: string
  permissions?: PluginPermissionType[]
  main: PluginMainFunction | ((api: PluginAPI) => PluginInstance)
}

// 插件类型
export interface Plugin {
  id: string
  name: string
  version: string
  author: string
  description: string
  permissions: PluginPermissionType[]
  state: PluginStateType
  error: string | null
  main: PluginMainFunction
}

// 插件实例数据
interface PluginInstanceData {
  instance: PluginInstance
  api: PluginAPI
  sandbox: PluginSandbox
}

// 扩展注册表类型
interface Extensions {
  lyricsProviders: (LyricsProvider & { pluginId: string })[]
  visualizers: (Visualizer & { pluginId: string })[]
  themes: { pluginId: string; [key: string]: unknown }[]
  menuItems: (MenuItem & { pluginId: string })[]
  settingsPanels: (SettingsPanel & { pluginId: string })[]
  playerDecorators: (PlayerDecorator & { pluginId: string })[]
  commands: (Command & { pluginId: string })[]
  shortcuts: (Shortcut & { pluginId: string })[]
  actionButtons: (ActionButton & { pluginId: string })[]
}

// 事件监听器类型
interface EventListener {
  pluginId: string
  callback: EventCallback
}

class PluginManager {
  // 已注册的插件
  plugins: Map<string, Plugin>
  // 插件实例
  private instances: Map<string, PluginInstanceData>
  // 扩展点注册表
  extensions: Extensions
  // 事件监听器
  private eventListeners: Map<string, EventListener[]>
  // 插件存储
  private storage: Map<string, Record<string, unknown>>
  // 播放器状态监听器
  private _playerWatcherStop: WatchStopHandle | null

  constructor() {
    this.plugins = reactive(new Map()) as Map<string, Plugin>
    this.instances = new Map()
    this.extensions = reactive({
      lyricsProviders: [],
      visualizers: [],
      themes: [],
      menuItems: [],
      settingsPanels: [],
      playerDecorators: [],
      commands: [],
      shortcuts: [],
      actionButtons: [],
    })
    this.eventListeners = new Map()
    this.storage = new Map()
    this._playerWatcherStop = null
  }

  /**
   * 初始化插件管理器
   */
  async init(): Promise<void> {
    const playerStore = usePlayerStore()
    
    this._playerWatcherStop = watch(
      () => ({
        track: playerStore.currentTrack,
        isPlaying: playerStore.isPlaying,
      }),
      (newState, oldState) => {
        const newTrackPath = newState.track?.path
        const oldTrackPath = oldState?.track?.path
        
        if (newTrackPath !== oldTrackPath) {
          this.emit('player:trackChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }
        
        if (newState.isPlaying !== oldState?.isPlaying) {
          this.emit('player:stateChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }
      },
      { immediate: false }
    )
    
    logger.info('插件管理器已初始化')
  }

  /**
   * 清理插件管理器
   */
  cleanup(): void {
    if (this._playerWatcherStop) {
      this._playerWatcherStop()
      this._playerWatcherStop = null
    }
  }

  /**
   * 注册插件
   */
  async register(pluginDef: PluginDefinition | BuiltinPluginDefinition): Promise<Plugin> {
    const { id, name, version, author, description, permissions = [], main } = pluginDef

    if (!id || !name || !main) {
      throw new Error('插件必须包含 id, name 和 main')
    }

    if (this.plugins.has(id)) {
      throw new Error(`插件 ${id} 已存在`)
    }

    const plugin: Plugin = reactive({
      id,
      name,
      version: version || '1.0.0',
      author: author || 'Unknown',
      description: description || '',
      permissions: permissions as PluginPermissionType[],
      state: PluginState.INACTIVE,
      error: null,
      main: markRaw(main as PluginMainFunction),
    })

    this.plugins.set(id, plugin)
    logger.info(`插件已注册: ${name} (${id})`)

    return plugin
  }

  /**
   * 激活插件
   */
  async activate(pluginId: string): Promise<void> {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) {
      throw new Error(`插件 ${pluginId} 不存在`)
    }

    if (plugin.state === PluginState.ACTIVE) {
      return
    }

    try {
      const api = createPluginAPI(pluginId, plugin.permissions, this)
      const sandbox = createPluginSandbox(api)
      const instance = await sandbox.execute(plugin.main)
      
      if (instance && typeof instance.activate === 'function') {
        await sandbox.execute(() => instance.activate!())
      }

      this.instances.set(pluginId, { instance, api, sandbox })
      plugin.state = PluginState.ACTIVE
      plugin.error = null

      logger.info(`插件已激活: ${plugin.name}`)
      this.emit('plugin:activated', { pluginId, plugin })

    } catch (error) {
      plugin.state = PluginState.ERROR
      plugin.error = error instanceof Error ? error.message : String(error)
      logger.error(`插件激活失败: ${plugin.name}`, error)
      throw error
    }
  }

  /**
   * 停用插件
   */
  async deactivate(pluginId: string): Promise<void> {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) return

    const instanceData = this.instances.get(pluginId)
    if (instanceData) {
      try {
        if (instanceData.instance && typeof instanceData.instance.deactivate === 'function') {
          if (instanceData.sandbox) {
            await instanceData.sandbox.execute(() => instanceData.instance.deactivate!())
          } else {
            await instanceData.instance.deactivate()
          }
        }
        
        if (instanceData.sandbox && typeof instanceData.sandbox.cleanup === 'function') {
          instanceData.sandbox.cleanup()
        }
      } catch (error) {
        logger.error(`插件停用出错: ${plugin.name}`, error)
      }

      this.instances.delete(pluginId)
    }

    this.cleanupPluginExtensions(pluginId)

    plugin.state = PluginState.INACTIVE
    logger.info(`插件已停用: ${plugin.name}`)
    this.emit('plugin:deactivated', { pluginId, plugin })
  }

  /**
   * 卸载插件
   */
  async uninstall(pluginId: string, clearStorage = false): Promise<void> {
    await this.deactivate(pluginId)
    this.plugins.delete(pluginId)
    this.storage.delete(pluginId)
    
    if (clearStorage) {
      try {
        localStorage.removeItem(STORAGE_PREFIX + pluginId)
        logger.info(`插件存储已清除: ${pluginId}`)
      } catch (e) {
        logger.warn(`清除插件存储失败: ${pluginId}`, e)
      }
    }
    
    logger.info(`插件已卸载: ${pluginId}`)
    this.emit('plugin:uninstalled', { pluginId })
  }

  /**
   * 清理插件注册的扩展
   */
  cleanupPluginExtensions(pluginId: string): void {
    for (const key of Object.keys(this.extensions) as (keyof Extensions)[]) {
      (this.extensions[key] as { pluginId: string }[]) = 
        this.extensions[key].filter(ext => ext.pluginId !== pluginId)
    }
    
    for (const [event, listeners] of this.eventListeners) {
      this.eventListeners.set(
        event,
        listeners.filter(l => l.pluginId !== pluginId)
      )
    }
  }

  /**
   * 注册扩展
   */
  registerExtension<K extends keyof Extensions>(
    type: K, 
    pluginId: string, 
    extension: Omit<Extensions[K][number], 'pluginId'>
  ): void {
    if (!this.extensions[type]) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this.extensions as any)[type] = []
    }
    (this.extensions[type] as { pluginId: string }[]).push({ ...extension, pluginId })
    logger.debug(`插件 ${pluginId} 注册了 ${type} 扩展`)
  }

  /**
   * 获取扩展
   */
  getExtensions<K extends keyof Extensions>(type: K): Extensions[K] {
    return this.extensions[type] || []
  }

  /**
   * 事件系统
   */
  on(event: string, pluginId: string, callback: EventCallback): void {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, [])
    }
    this.eventListeners.get(event)!.push({ pluginId, callback })
  }

  off(event: string, pluginId: string, callback: EventCallback): void {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      const index = listeners.findIndex(l => l.pluginId === pluginId && l.callback === callback)
      if (index > -1) {
        listeners.splice(index, 1)
      }
    }
  }

  emit(event: string, data?: unknown): void {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      for (const { callback } of listeners) {
        try {
          callback(data)
        } catch (error) {
          logger.error(`事件处理出错: ${event}`, error)
        }
      }
    }
  }

  /**
   * 插件存储
   */
  getStorage(pluginId: string): Record<string, unknown> {
    if (!this.storage.has(pluginId)) {
      const storageKey = STORAGE_PREFIX + pluginId
      let savedData: Record<string, unknown> = {}
      
      try {
        const saved = localStorage.getItem(storageKey)
        if (saved) {
          savedData = JSON.parse(saved)
        }
      } catch (e) {
        logger.warn(`加载插件 ${pluginId} 存储失败:`, e)
      }
      
      const storage = reactive(savedData)
      const maxStorageSize = 1024 * 1024

      const safeSave = (target: Record<string, unknown>) => {
        try {
          const json = JSON.stringify(target)
          if (json.length > maxStorageSize) {
            logger.warn(`插件 ${pluginId} 存储超过限制`)
            for (const key of Object.keys(target)) {
              if (Array.isArray(target[key]) && (target[key] as unknown[]).length > 10) {
                target[key] = (target[key] as unknown[]).slice(-Math.floor((target[key] as unknown[]).length / 2))
              }
            }
          }
          localStorage.setItem(storageKey, JSON.stringify(target))
        } catch (e) {
          if ((e as Error).name === 'QuotaExceededError') {
            logger.error(`插件 ${pluginId} 存储空间不足`)
            for (const key of Object.keys(target)) {
              if (Array.isArray(target[key])) {
                target[key] = (target[key] as unknown[]).slice(-10)
              }
            }
            try {
              localStorage.setItem(storageKey, JSON.stringify(target))
            } catch {
              localStorage.removeItem(storageKey)
            }
          } else {
            logger.warn(`保存插件 ${pluginId} 存储失败:`, e)
          }
        }
      }
      
      let saveTimeout: ReturnType<typeof setTimeout> | null = null
      const debouncedSave = (target: Record<string, unknown>) => {
        if (saveTimeout) clearTimeout(saveTimeout)
        saveTimeout = setTimeout(() => safeSave(target), 500)
      }
      
      const persistentStorage = new Proxy(storage, {
        set(target, key: string, value) {
          target[key] = value
          debouncedSave(target)
          return true
        },
        deleteProperty(target, key: string) {
          delete target[key]
          debouncedSave(target)
          return true
        }
      })
      
      this.storage.set(pluginId, persistentStorage)
    }
    return this.storage.get(pluginId)!
  }

  /**
   * 获取所有插件
   */
  getAllPlugins(): Plugin[] {
    return Array.from(this.plugins.values())
  }

  /**
   * 获取活跃插件
   */
  getActivePlugins(): Plugin[] {
    return this.getAllPlugins().filter(p => p.state === PluginState.ACTIVE)
  }
}

// 单例
export const pluginManager = new PluginManager()
export { PluginManager }
export default pluginManager
