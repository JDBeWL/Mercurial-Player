/**
 * Worker 沙箱核心 —— 运行在 Dedicated Worker 中
 *
 * 外置插件代码在本 Worker 内以 ES 模块 (blob URL 动态 import) 求值,
 * 与主窗口的完整权限 (DOM / localStorage / Tauri IPC) 物理隔离:
 * Worker 中不存在 __TAURI_INTERNALS__ 与 window 对象,插件只能通过
 * postMessage RPC 访问主窗口受权限控制的 PluginAPI。
 *
 * 本模块保持纯逻辑 (不直接引用 self),便于在测试中以 FakeWorker
 * 通道驱动真实的协议实现。
 */

import { toModuleCode } from '../moduleExecutor'
import {
  PluginPermission,
  type PluginAPI,
  type SaveAsOptions,
  type LyricLine,
  type MenuItem,
  type ActionButton,
  type LyricsProvider,
  type Command,
  type Shortcut,
} from '../pluginTypes'
import type { HostToWorkerMessage, MirrorData, WorkerToHostMessage } from './sandboxProtocol'
import {
  SANDBOX_FN_MARKER,
  SANDBOX_RESPONSE_MARKER,
  deserializeError,
  serializeError,
} from './sandboxProtocol'

// ---------------------------------------------------------------------------
// 模块加载
// ---------------------------------------------------------------------------

/** 插件工厂函数 (模块默认导出):接收 api 与沙箱全局,返回插件实例 */
export type PluginFactory = (api: PluginAPI, globals: unknown) => unknown

/** 模块加载器:将插件源码求值为工厂函数 (生产用 blob import,测试可注入) */
export type ModuleLoader = (code: string) => Promise<PluginFactory>

/** 默认模块加载器:blob URL + 动态 import (Worker 中 CSP script-src blob: 允许) */
async function importPluginModule(code: string): Promise<PluginFactory> {
  const moduleCode = toModuleCode(code)
  const blob = new Blob([moduleCode], { type: 'text/javascript' })
  const url = URL.createObjectURL(blob)
  let mod: { default?: unknown }
  try {
    mod = (await import(/* @vite-ignore */ url)) as { default?: unknown }
  } catch (error) {
    throw new Error(
      `插件模块加载失败（代码可能存在语法错误）: ${error instanceof Error ? error.message : String(error)}`,
    )
  } finally {
    URL.revokeObjectURL(url)
  }
  if (typeof mod.default !== 'function') {
    throw new Error('插件模块缺少默认导出函数 (export default function/api => {...})')
  }
  return mod.default as PluginFactory
}

// ---------------------------------------------------------------------------
// 序列化
// ---------------------------------------------------------------------------

type CallbackRegistrar = (fn: (...args: unknown[]) => unknown) => number

/**
 * 序列化发送给主窗口的值:
 * 函数替换为回调句柄标记,可直接结构化克隆的类型原样保留。
 */
function serializeForHost(value: unknown, registerCallback: CallbackRegistrar): unknown {
  if (typeof value === 'function') {
    return { [SANDBOX_FN_MARKER]: registerCallback(value as (...args: unknown[]) => unknown) }
  }
  if (Array.isArray(value)) {
    return value.map((item) => serializeForHost(item, registerCallback))
  }
  if (value && typeof value === 'object') {
    // 仅展开普通对象:带原型的宿主对象 (Date/Map/Blob/Node 的 Timeout 等,
    // 内部常含循环引用) 原样交给结构化克隆处理,展开会导致无限递归。
    // OffscreenCanvas/ImageBitmap 在纯 Node 测试环境不存在,需 typeof 守卫
    const proto = Object.getPrototypeOf(value)
    if (proto !== Object.prototype && proto !== null) {
      return value
    }
    if (
      value instanceof ArrayBuffer ||
      (typeof OffscreenCanvas !== 'undefined' && value instanceof OffscreenCanvas) ||
      (typeof ImageBitmap !== 'undefined' && value instanceof ImageBitmap)
    ) {
      return value
    }
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(value)) {
      out[key] = serializeForHost(item, registerCallback)
    }
    return out
  }
  return value
}

/** 日志参数消毒:函数/Symbol 等不可克隆值替换为占位符 */
function sanitizeLogArgs(args: unknown[]): unknown[] {
  return args.map((arg) => {
    if (typeof arg === 'function') return '[Function]'
    if (typeof arg === 'symbol') return String(arg)
    if (typeof arg === 'bigint') return `${arg}n`
    if (arg instanceof WeakMap || arg instanceof WeakSet || arg instanceof WeakRef) {
      return String(arg)
    }
    if (arg && typeof arg === 'object') {
      // Error/Map/Set/Blob/循环引用均可结构化克隆,原样传递
      return arg
    }
    return arg
  })
}

// ---------------------------------------------------------------------------
// Worker 本地工具实现 (不经过主窗口)
// ---------------------------------------------------------------------------

/** Worker 全局中需移除的原生网络 API
 *  (插件的全部网络访问必须经 api.network.fetch 的权限代理走后端 HTTP) */
const NETWORK_GLOBAL_KEYS = [
  'fetch',
  'XMLHttpRequest',
  'WebSocket',
  'EventSource',
  'WebSocketStream',
] as const

/**
 * 从 Worker 全局作用域移除原生网络 API。
 *
 * blob Worker 的 CSP 继承 (script-src/connect-src 生效) 之外的纵深防御:
 * 即使 CSP 继承在某个 WebView 版本中失效,插件也无法直接发起网络请求。
 * - 自身可配置属性:直接删除
 * - 原型链上的属性:以抛错的 getter 遮蔽
 * - 自身不可配置属性:保留 (由 CSP 兜底拦截)
 */
export function removeNetworkGlobals(scope: Record<string, unknown>): void {
  for (const key of NETWORK_GLOBAL_KEYS) {
    neutralizeGlobal(scope, key)
  }
  // navigator.sendBeacon 是另一个数据外传通道 (定义在原型链上)
  const navigator = scope['navigator']
  if (navigator && typeof navigator === 'object') {
    neutralizeGlobal(navigator as Record<string, unknown>, 'sendBeacon')
  }
}

function neutralizeGlobal(target: Record<string, unknown>, key: string): void {
  try {
    if (!(key in target)) return
    const desc = Object.getOwnPropertyDescriptor(target, key)
    if (desc) {
      if (desc.configurable) delete target[key]
    } else {
      // 属性在原型链上:定义自身抛错 getter 遮蔽
      Object.defineProperty(target, key, {
        configurable: true,
        get() {
          throw new Error(`沙箱禁止使用 ${key}，请使用 api.network.fetch`)
        },
      })
    }
  } catch {
    // 部分环境不允许修改该属性:忽略 (CSP 继承兜底)
  }
}

/** 与 pluginAPI.formatTime 行为一致 */
function formatTime(seconds: number): string {
  if (isNaN(seconds) || !isFinite(seconds)) return '0:00'
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)
  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
  }
  return `${minutes}:${secs.toString().padStart(2, '0')}`
}

/** 与 pluginAPI.dataURLToBlob 行为一致 */
function dataURLToBlob(dataURL: string): Blob {
  const arr = dataURL.split(',')
  const mimeMatch = arr[0]!.match(/:(.*?);/)
  const mime = mimeMatch ? mimeMatch[1]! : 'application/octet-stream'
  const bstr = atob(arr[1]!)
  let n = bstr.length
  const u8arr = new Uint8Array(n)
  while (n--) {
    u8arr[n] = bstr.charCodeAt(n)
  }
  return new Blob([u8arr], { type: mime })
}

async function arrayBufferToBase64(buffer: ArrayBuffer): Promise<string> {
  const bytes = new Uint8Array(buffer)
  let binary = ''
  const chunkSize = 0x8000
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize))
  }
  return btoa(binary)
}

/** CSS 变量名 → 驼峰键 (与 pluginAPI.getAllColors 的键生成规则一致) */
function cssVarToCamel(name: string): string {
  return name.replace(/^--/, '').replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())
}

// ---------------------------------------------------------------------------
// network.fetch 的 Response 序列化包装 (Worker 侧)
// ---------------------------------------------------------------------------

interface SerializedResponse {
  [SANDBOX_RESPONSE_MARKER]: true
  ok: boolean
  status: number
  statusText: string
  url: string
  headers: Record<string, string>
  body: ArrayBuffer
}

export type ResponseLike = {
  ok: boolean
  status: number
  statusText: string
  url: string
  headers: Record<string, string>
  arrayBuffer: () => Promise<ArrayBuffer>
  text: () => Promise<string>
  json: () => Promise<unknown>
  blob: () => Promise<Blob>
}

function reviveResponse(serialized: SerializedResponse): ResponseLike {
  const decoder = new TextDecoder()
  return {
    ok: serialized.ok,
    status: serialized.status,
    statusText: serialized.statusText,
    url: serialized.url,
    headers: serialized.headers,
    arrayBuffer: async () => serialized.body,
    text: async () => decoder.decode(serialized.body),
    json: async () => JSON.parse(decoder.decode(serialized.body)),
    blob: async () => new Blob([serialized.body]),
  }
}

/** api-result 值的反序列化 (主窗口 → Worker 方向) */
function reviveHostValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(reviveHostValue)
  }
  if (value && typeof value === 'object') {
    // 仅展开普通对象;Date/Map/ImageBitmap 等保留克隆原样 (展开会丢失类型)
    const proto = Object.getPrototypeOf(value)
    if (proto !== Object.prototype && proto !== null) {
      return value
    }
    const record = value as Record<string, unknown>
    if (record[SANDBOX_RESPONSE_MARKER] === true) {
      return reviveResponse(record as unknown as SerializedResponse)
    }
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(record)) {
      out[key] = reviveHostValue(item)
    }
    return out
  }
  return value
}

// ---------------------------------------------------------------------------
// 沙箱 Worker 运行时
// ---------------------------------------------------------------------------

interface PendingCall {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
}

/**
 * 处理主窗口消息,维护镜像状态与回调注册表,
 * 向插件代码暴露与原 PluginAPI 契约一致的代理对象。
 */
export class SandboxWorkerRuntime {
  private pluginId = ''
  private permissions: readonly string[] = []
  private factory: PluginFactory | null = null
  private mirror: MirrorData
  private callbackMap = new Map<number, (...args: unknown[]) => unknown>()
  private pendingCalls = new Map<number, PendingCall>()
  private nextCallId = 1
  private nextCbId = 1
  /** events.off 需要按注册函数反查句柄 id */
  private eventHandlerIds = new Map<string, Map<(...args: unknown[]) => unknown, number>>()

  constructor(
    private post: (msg: WorkerToHostMessage) => void,
    private loadModule: ModuleLoader = importPluginModule,
  ) {
    this.mirror = {
      playerState: {
        currentTrack: null,
        isPlaying: false,
        currentTime: 0,
        duration: 0,
        volume: 1,
        repeatMode: 'none',
        isShuffle: false,
      },
      currentLyricIndex: -1,
      theme: { preference: 'system', isDark: false, primaryColor: '' },
      colors: {},
      storage: null,
      playlists: [],
      currentPlaylist: null,
      tracks: [],
    }
  }

  /** Worker 内未捕获的 Promise 拒绝 (生产构建 drop console,必须转发落盘) */
  reportUnhandledRejection(reason: unknown): void {
    this.post({
      type: 'log',
      level: 'error',
      args: ['[沙箱未捕获拒绝]', ...(reason instanceof Error ? [reason] : [String(reason)])],
    })
  }

  async handleMessage(msg: HostToWorkerMessage): Promise<void> {
    switch (msg.type) {
      case 'init':
        this.pluginId = msg.pluginId
        this.permissions = msg.permissions
        try {
          this.factory = await this.loadModule(msg.code)
          this.post({ type: 'init-result', ok: true })
        } catch (error) {
          this.post({ type: 'init-result', ok: false, error: serializeError(error) })
        }
        break

      case 'run-main':
        await this.runMain()
        break

      case 'mirror':
        Object.assign(this.mirror, msg.data)
        break

      case 'api-result': {
        const pending = this.pendingCalls.get(msg.callId)
        this.pendingCalls.delete(msg.callId)
        if (!pending) return // fire-and-forget 调用的结果,忽略
        if (msg.ok) {
          pending.resolve(reviveHostValue(msg.value))
        } else {
          pending.reject(deserializeError(msg.error))
        }
        break
      }

      case 'callback-call': {
        const fn = this.callbackMap.get(msg.cbId)
        if (!fn) {
          this.post({
            type: 'callback-result',
            callId: msg.callId,
            ok: false,
            error: { name: 'Error', message: `沙箱回调不存在或已被清理: ${msg.cbId}` },
          })
          return
        }
        try {
          const result = await fn(...msg.args)
          this.post({
            type: 'callback-result',
            callId: msg.callId,
            ok: true,
            value: serializeForHost(result, (f) => this.registerCallback(f)),
          })
        } catch (error) {
          this.post({
            type: 'callback-result',
            callId: msg.callId,
            ok: false,
            error: serializeError(error),
          })
        }
        break
      }
    }
  }

  // -------------------------------------------------------------------------
  // 内部:主流程与代理构建
  // -------------------------------------------------------------------------

  private async runMain(): Promise<void> {
    if (!this.factory) {
      this.post({
        type: 'main-result',
        ok: false,
        error: { name: 'Error', message: '插件模块尚未初始化' },
      })
      return
    }
    try {
      const instance = await this.factory(this.buildApiProxy(), this.buildSandboxGlobals())
      this.post({
        type: 'main-result',
        ok: true,
        instance: serializeForHost(instance, (f) => this.registerCallback(f)),
      })
    } catch (error) {
      this.post({ type: 'log', level: 'error', args: ['插件执行错误:', error] })
      this.post({ type: 'main-result', ok: false, error: serializeError(error) })
    }
  }

  private registerCallback(fn: (...args: unknown[]) => unknown): number {
    const id = this.nextCbId++
    this.callbackMap.set(id, fn)
    return id
  }

  /** 权限预检:与 pluginAPI 的同步 throw 语义一致 */
  private requirePermission(permission: string, action: string): void {
    if (!this.permissions.includes(permission)) {
      throw new Error(`插件 ${this.pluginId} 没有 ${permission} 权限，无法执行 ${action}`)
    }
  }

  private log(level: 'info' | 'warn' | 'error' | 'debug', args: unknown[]): void {
    this.post({ type: 'log', level, args: sanitizeLogArgs(args) })
  }

  /** 异步 RPC:等待主窗口执行真实 API 后返回 */
  private rpc<T = unknown>(path: string, args: unknown[] = []): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const callId = this.nextCallId++
      try {
        this.post({
          type: 'api-call',
          callId,
          path,
          args: serializeForHost(args, (f) => this.registerCallback(f)) as unknown[],
        })
      } catch (error) {
        reject(
          new Error(
            `调用 ${path} 失败: 参数无法序列化传输 (${error instanceof Error ? error.message : String(error)})`,
          ),
        )
        return
      }
      this.pendingCalls.set(callId, {
        resolve: resolve as (value: unknown) => void,
        reject,
      })
    })
  }

  /** 同步命令 (原 API 返回 void):权限预检后异步派发,错误由主窗口记录日志 */
  private fire(path: string, args: unknown[] = []): void {
    const callId = this.nextCallId++
    try {
      this.post({
        type: 'api-call',
        callId,
        path,
        args: serializeForHost(args, (f) => this.registerCallback(f)) as unknown[],
      })
    } catch (error) {
      this.log('error', [`调用 ${path} 失败: 参数无法序列化`, error])
    }
  }

  /** 沙箱全局对象 (插件工厂第二参数,与 pluginSandbox 语义一致) */
  private buildSandboxGlobals() {
    // 与 SafeTimers 一致的参数钳制: setTimeout 上限 60s,setInterval 下限 100ms;
    // 回调错误被捕获并转发日志 (Worker 终止时所有未触发定时器一并释放,无需 cleanup)
    const sandboxSetTimeout = (
      callback: (...a: unknown[]) => void,
      delay?: number,
      ...rest: unknown[]
    ): number =>
      setTimeout(
        (...a: unknown[]) => {
          try {
            callback(...a)
          } catch (e) {
            this.log('error', ['定时器执行错误:', e])
          }
        },
        Math.min(delay || 0, 60000),
        ...rest,
      ) as unknown as number

    const sandboxSetInterval = (
      callback: (...a: unknown[]) => void,
      delay?: number,
      ...rest: unknown[]
    ): number =>
      setInterval(
        (...a: unknown[]) => {
          try {
            callback(...a)
          } catch (e) {
            this.log('error', ['定时器执行错误:', e])
          }
        },
        Math.max(delay || 100, 100),
        ...rest,
      ) as unknown as number

    return {
      console: {
        log: (...args: unknown[]) => this.log('info', args),
        info: (...args: unknown[]) => this.log('info', args),
        warn: (...args: unknown[]) => this.log('warn', args),
        error: (...args: unknown[]) => this.log('error', args),
        debug: (...args: unknown[]) => this.log('debug', args),
      },
      setTimeout: sandboxSetTimeout,
      clearTimeout: (id: number) => clearTimeout(id),
      setInterval: sandboxSetInterval,
      clearInterval: (id: number) => clearInterval(id),
    }
  }

  /** Worker 侧 PluginAPI 代理:同步读走镜像,命令/异步调用走 RPC */
  private buildApiProxy(): PluginAPI {
    const P = PluginPermission
    const mirror = this.mirror

    return {
      pluginId: this.pluginId,
      permissions: this.permissions,

      log: {
        info: (...args: unknown[]) => this.log('info', args),
        warn: (...args: unknown[]) => this.log('warn', args),
        error: (...args: unknown[]) => this.log('error', args),
        debug: (...args: unknown[]) => this.log('debug', args),
      },

      player: {
        getState: () => {
          this.requirePermission(P.PLAYER_READ, 'player.getState')
          return structuredClone(mirror.playerState)
        },
        getLyrics: () => this.rpc('player.getLyrics'),
        getCurrentLyricIndex: () => {
          this.requirePermission(P.PLAYER_READ, 'player.getCurrentLyricIndex')
          return mirror.currentLyricIndex
        },
        getCoverPath: () => this.rpc('player.getCoverPath'),
        play: () => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.play')
          this.fire('player.play')
        },
        pause: () => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.pause')
          this.fire('player.pause')
        },
        togglePlay: () => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.togglePlay')
          this.fire('player.togglePlay')
        },
        next: () => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.next')
          return this.rpc('player.next')
        },
        previous: () => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.previous')
          return this.rpc('player.previous')
        },
        seek: (time: number) => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.seek')
          this.fire('player.seek', [time])
        },
        setVolume: (volume: number) => {
          this.requirePermission(P.PLAYER_CONTROL, 'player.setVolume')
          this.fire('player.setVolume', [volume])
        },
        setLyrics: (lyrics: LyricLine[]) => {
          this.requirePermission(P.LYRICS_PROVIDER, 'player.setLyrics')
          this.fire('player.setLyrics', [lyrics])
        },
      },

      library: {
        getPlaylists: () => {
          this.requirePermission(P.LIBRARY_READ, 'library.getPlaylists')
          return structuredClone(mirror.playlists)
        },
        getCurrentPlaylist: () => {
          this.requirePermission(P.LIBRARY_READ, 'library.getCurrentPlaylist')
          return structuredClone(mirror.currentPlaylist)
        },
        getTracks: () => {
          this.requirePermission(P.LIBRARY_READ, 'library.getTracks')
          return structuredClone(mirror.tracks)
        },
      },

      theme: {
        getCurrent: () => structuredClone(mirror.theme),
        setColors: (colors: Record<string, string>) => {
          this.requirePermission(P.THEME, 'theme.setColors')
          return this.rpc('theme.setColors', [colors])
        },
        getCSSVariable: (name: string) => mirror.colors[cssVarToCamel(name)] ?? '',
        getAllColors: () => ({ ...mirror.colors }),
      },

      ui: {
        // Vue 组件无法跨 Worker 边界渲染,这类扩展只对内置插件开放
        registerSettingsPanel: () => {
          throw new Error(
            '沙箱插件不支持注册设置面板 (UI 组件无法跨沙箱渲染)，请以内置插件形式实现',
          )
        },
        registerPlayerDecorator: () => {
          throw new Error(
            '沙箱插件不支持注册播放器装饰器 (UI 组件无法跨沙箱渲染)，请以内置插件形式实现',
          )
        },
        registerMenuItem: (item: MenuItem) => {
          this.requirePermission(P.UI_EXTEND, 'ui.registerMenuItem')
          this.fire('ui.registerMenuItem', [item])
        },
        registerActionButton: (button: ActionButton) => {
          if (!button.id || !button.name || !button.icon || !button.action) {
            throw new Error('按钮必须包含 id, name, icon 和 action')
          }
          this.fire('ui.registerActionButton', [button])
        },
        unregisterActionButton: (buttonId: string) => {
          this.fire('ui.unregisterActionButton', [buttonId])
        },
        showNotification: (message: string, type?: 'error' | 'warning' | 'info') => {
          this.fire('ui.showNotification', [message, type])
        },
      },

      lyrics: {
        registerProvider: (provider: LyricsProvider) => {
          this.requirePermission(P.LYRICS_PROVIDER, 'lyrics.registerProvider')
          if (!provider.id || !provider.name || !provider.search) {
            throw new Error('歌词源必须包含 id, name 和 search 方法')
          }
          this.fire('lyrics.registerProvider', [provider])
        },
      },

      visualizer: {
        // render 回调需要主窗口 CanvasRenderingContext2D,无法跨沙箱传递
        register: () => {
          throw new Error(
            '沙箱插件不支持注册可视化效果 (渲染回调依赖主窗口 Canvas 上下文)，请以内置插件形式实现',
          )
        },
      },

      commands: {
        register: (command: Command) => {
          if (!command.id || !command.name || !command.execute) {
            throw new Error('命令必须包含 id, name 和 execute 方法')
          }
          this.fire('commands.register', [command])
        },
        execute: (commandId: string) => this.rpc('commands.execute', [commandId]),
      },

      shortcuts: {
        register: (shortcut: Shortcut) => {
          if (!shortcut.id || !shortcut.name || !shortcut.key || !shortcut.action) {
            throw new Error('快捷键必须包含 id, name, key 和 action')
          }
          this.fire('shortcuts.register', [shortcut])
        },
        unregister: (shortcutId: string) => {
          this.fire('shortcuts.unregister', [shortcutId])
        },
      },

      storage: {
        get: <T>(key: string, defaultValue: T | null = null): T => {
          this.requirePermission(P.STORAGE, 'storage.get')
          return (mirror.storage?.[key] as T) ?? (defaultValue as T)
        },
        set: <T>(key: string, value: T): void => {
          this.requirePermission(P.STORAGE, 'storage.set')
          if (!mirror.storage) mirror.storage = {}
          mirror.storage[key] = value
          this.fire('storage.set', [key, value])
        },
        remove: (key: string): void => {
          this.requirePermission(P.STORAGE, 'storage.remove')
          if (mirror.storage) delete mirror.storage[key]
          this.fire('storage.remove', [key])
        },
        getAll: (): Record<string, unknown> => {
          this.requirePermission(P.STORAGE, 'storage.getAll')
          return { ...mirror.storage }
        },
      },

      events: {
        on: (event: string, callback: (data?: unknown) => void): void => {
          const id = this.registerCallback(callback)
          let handlers = this.eventHandlerIds.get(event)
          if (!handlers) {
            handlers = new Map()
            this.eventHandlerIds.set(event, handlers)
          }
          handlers.set(callback, id)
          this.fire('events.on', [event, { [SANDBOX_FN_MARKER]: id }])
        },
        off: (event: string, callback: (data?: unknown) => void): void => {
          const handlers = this.eventHandlerIds.get(event)
          const id = handlers?.get(callback)
          if (id === undefined) return
          handlers!.delete(callback)
          this.fire('events.off', [event, { [SANDBOX_FN_MARKER]: id }])
        },
        emit: (event: string, data?: unknown): void => {
          this.fire('events.emit', [event, data])
        },
      },

      network: {
        fetch: (url: string, options: RequestInit = {}): Promise<ResponseLike> => {
          this.requirePermission(P.NETWORK, 'network.fetch')
          // AbortSignal 不可结构化克隆,Worker 沙箱不支持取消信号
          const { signal: _signal, ...rest } = options
          return this.rpc('network.fetch', [url, rest]) as Promise<ResponseLike>
        },
      },

      utils: {
        createCanvas: (width: number, height: number) => {
          if (typeof OffscreenCanvas === 'undefined') {
            throw new Error('当前环境不支持 OffscreenCanvas')
          }
          const canvas = new OffscreenCanvas(width, height)
          const ctx = canvas.getContext('2d')
          return { canvas, ctx: ctx as unknown as CanvasRenderingContext2D | null }
        },
        canvasToBlob: (canvas: OffscreenCanvas, type = 'image/png', quality = 0.92) =>
          canvas.convertToBlob({ type, quality }),
        canvasToDataURL: async (canvas: OffscreenCanvas, type = 'image/png', quality = 0.92) => {
          const blob = await canvas.convertToBlob({ type, quality })
          return `data:${blob.type};base64,${await arrayBufferToBase64(await blob.arrayBuffer())}`
        },
        loadImage: (src: string) => this.rpc('utils.loadImage', [src]),
        blobToArrayBuffer: (blob: Blob) => blob.arrayBuffer(),
        dataURLToBlob: (dataURL: string) => dataURLToBlob(dataURL),
        formatTime: (seconds: number) => formatTime(seconds),
        generateId: () =>
          `${this.pluginId}-${Date.now()}-${Math.random().toString(36).substring(2, 11)}`,
      },

      file: {
        saveAs: (data: Blob | Uint8Array | string, options: SaveAsOptions = {}) => {
          this.requirePermission(P.STORAGE, 'file.saveAs')
          return this.rpc('file.saveAs', [data, options])
        },
        saveImage: (
          image: OffscreenCanvas | Blob | string,
          defaultName = 'image.png',
          format = 'png',
        ) => {
          this.requirePermission(P.STORAGE, 'file.saveImage')
          return this.rpc('file.saveImage', [image, defaultName, format])
        },
        openScreenshotsDirectory: () => this.rpc('file.openScreenshotsDirectory'),
      },

      clipboard: {
        writeImage: (image: OffscreenCanvas | Blob | string) => {
          this.requirePermission(P.STORAGE, 'clipboard.writeImage')
          return this.rpc('clipboard.writeImage', [image])
        },
        writeText: (text: string) => {
          this.requirePermission(P.STORAGE, 'clipboard.writeText')
          return this.rpc('clipboard.writeText', [text])
        },
      },
    } as unknown as PluginAPI
  }
}
