/**
 * 插件系统类型契约
 * 集中定义插件 API 的纯类型部分(PluginAPI、清单、实例、权限等),
 * pluginManager.ts 对这些类型做 re-export 以保持向后兼容。
 */

import type { Track } from '@/types'

// 插件 API 使用的曲目类型(规范定义在 @/types,此处仅作 re-export 保持向后兼容)
export type { Track } from '@/types'

// 插件状态
export const PluginState = {
  UNREGISTERED: 'unregistered',
  REGISTERED: 'registered',
  LOADING: 'loading',
  ACTIVE: 'active',
  UNLOADING: 'unloading',
  INACTIVE: 'inactive',
  ERROR: 'error',
  DISABLED: 'disabled',
} as const

export type PluginStateType = (typeof PluginState)[keyof typeof PluginState]

// 插件权限
export const PluginPermission = {
  PLAYER_READ: 'player:read', // 读取播放器状态
  PLAYER_CONTROL: 'player:control', // 控制播放器
  LIBRARY_READ: 'library:read', // 读取音乐库
  LYRICS_PROVIDER: 'lyrics:provider', // 提供歌词源
  UI_EXTEND: 'ui:extend', // 扩展 UI
  VISUALIZER: 'visualizer', // 可视化效果
  THEME: 'theme', // 主题
  STORAGE: 'storage', // 本地存储
  NETWORK: 'network', // 网络请求
} as const

export type PluginPermissionType = (typeof PluginPermission)[keyof typeof PluginPermission]

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
    getCoverPath: () => Promise<string | null>
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
    createCanvas: (
      width: number,
      height: number,
    ) => { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D | null }
    canvasToBlob: (canvas: HTMLCanvasElement, type?: string, quality?: number) => Promise<Blob>
    canvasToDataURL: (canvas: HTMLCanvasElement, type?: string, quality?: number) => string
    loadImage: (src: string) => Promise<HTMLImageElement>
    blobToArrayBuffer: (blob: Blob) => Promise<ArrayBuffer>
    dataURLToBlob: (dataURL: string, options?: { mimeType?: string; fallbackMime?: string }) => Blob
    formatTime: (seconds: number) => string
    generateId: () => string
  }
  file: {
    saveAs: (data: Blob | Uint8Array | string, options?: SaveAsOptions) => Promise<string | null>
    saveImage: (
      image: HTMLCanvasElement | Blob | string,
      defaultName?: string,
      format?: string,
    ) => Promise<string | null>
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

// 注意: LyricLine 与 Playlist 与 @/types 中的同名类型结构不同。
// @/types 的版本是应用内部的规范定义(由 LRC/ASS 解析器产生);
// 此处的版本是插件 API 契约,用于插件间歌词提供者交互与翻译支持,
// 在 pluginAPI.ts 的 convertLyricLine / getPlaylists 中做显式格式转换。
// 修改任一类型时需同步检查转换逻辑。
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

// 外置插件模块可解构使用的沙箱全局子集
// (安全 console 代理与带清理追踪的定时器,由 pluginSandbox 在激活时注入)
export interface PluginSandboxGlobals {
  console: {
    log: (...args: unknown[]) => void
    info: (...args: unknown[]) => void
    warn: (...args: unknown[]) => void
    error: (...args: unknown[]) => void
    debug: (...args: unknown[]) => void
  }
  setTimeout: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearTimeout: (id: number) => void
  setInterval: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearInterval: (id: number) => void
}

// 插件主函数类型(外置插件可接收第二个参数 globals 以使用沙箱全局对象)
export type PluginMainFunction = (
  api: PluginAPI,
  globals?: PluginSandboxGlobals,
) => Promise<PluginInstance> | PluginInstance

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
  /**
   * Worker 沙箱宿主 (外置插件由 pluginLoader 创建并注入)。
   * 存在时 pluginManager 将在 Worker 隔离环境中执行插件,
   * 其生命周期 (激活/停用/卸载) 由宿主管理。
   */
  workerHost?: import('./sandbox/workerSandboxHost').PluginWorkerHost
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

// 插件清单类型(外置插件的 manifest.json 结构)
export interface PluginManifest {
  id: string
  name: string
  version?: string
  author?: string
  description?: string
  permissions?: PluginPermissionType[]
  main?: string
  auto_activate?: boolean
}
