/**
 * 插件沙箱宿主 —— 主窗口侧
 *
 * 为单个外置插件管理一个 Dedicated Worker:
 * - 消息 RPC:把 Worker 内代理发起的 api-call 分发到真实 PluginAPI
 * - 状态镜像:向 Worker 推送 player/theme/storage/library 快照,
 *   使沙箱插件的同步读 API 保持原契约
 * - 回调桥:把插件注册的函数 (歌词源 search / 事件监听 / activate 等)
 *   还原为调用 Worker 的 stub 函数
 * - 生命周期:deactivate 时 terminate Worker (定时器/监听随之全部释放)
 */

import { watch, type WatchStopHandle } from 'vue'
import logger from '../../utils/logger'
import { usePlayerStore } from '../../stores/player'
import { useMusicLibraryStore } from '../../stores/musicLibrary'
import { useThemeStore } from '../../stores/theme'
import {
  PluginPermission,
  type PluginAPI,
  type PluginInstance,
  type PluginMainFunction,
  type PluginPermissionType,
} from '../pluginTypes'
import type { PluginSandbox } from '../pluginSandbox'
import type { HostToWorkerMessage, MirrorData, WorkerToHostMessage } from './sandboxProtocol'
import { SANDBOX_FN_MARKER, SANDBOX_RESPONSE_MARKER, deserializeError } from './sandboxProtocol'
// blob URL 内联 Worker: 安全考量见下方 defaultWorkerFactory 注释
import SandboxWorker from './workerBootstrap?worker&inline'

/** Worker 工厂 (注入点:测试环境替换为 FakeWorker) */
export type WorkerFactory = () => Worker

/**
 * 沙箱 api-call 路径白名单:精确路径 → 所需权限 (null = 无需权限)。
 *
 * 权限校验的权威位置在宿主 (主窗口可信侧):Worker 内的 requirePermission
 * (workerCore) 与插件代码共享同一全局作用域,可被插件以
 * self.postMessage({type:'api-call',...}) 直接绕过。本表以精确匹配取代
 * 任意属性链遍历,不在表中的路径 (如 log.info.constructor、
 * permissions.__proto__.push) 一律拒绝。
 * 使用 Map 避免对象原型链键 (constructor 等) 干扰白名单查找。
 */
const API_CALL_POLICY: ReadonlyMap<string, PluginPermissionType | null> = new Map([
  ['player.getLyrics', PluginPermission.PLAYER_READ],
  ['player.getCoverPath', PluginPermission.PLAYER_READ],
  ['player.play', PluginPermission.PLAYER_CONTROL],
  ['player.pause', PluginPermission.PLAYER_CONTROL],
  ['player.togglePlay', PluginPermission.PLAYER_CONTROL],
  ['player.next', PluginPermission.PLAYER_CONTROL],
  ['player.previous', PluginPermission.PLAYER_CONTROL],
  ['player.seek', PluginPermission.PLAYER_CONTROL],
  ['player.setVolume', PluginPermission.PLAYER_CONTROL],
  ['player.setLyrics', PluginPermission.LYRICS_PROVIDER],
  ['theme.setColors', PluginPermission.THEME],
  ['ui.registerMenuItem', PluginPermission.UI_EXTEND],
  ['ui.registerActionButton', PluginPermission.UI_EXTEND],
  ['ui.unregisterActionButton', PluginPermission.UI_EXTEND],
  ['ui.showNotification', null],
  ['lyrics.registerProvider', PluginPermission.LYRICS_PROVIDER],
  ['commands.register', PluginPermission.UI_EXTEND],
  // 仅可执行本插件注册的命令 (pluginAPI 侧按 pluginId 校验)
  ['commands.execute', null],
  ['shortcuts.register', PluginPermission.UI_EXTEND],
  ['shortcuts.unregister', PluginPermission.UI_EXTEND],
  ['storage.set', PluginPermission.STORAGE],
  ['storage.remove', PluginPermission.STORAGE],
  // 事件白名单与权限由 pluginAPI.events.on 在可信侧校验
  ['events.on', null],
  ['events.off', null],
  ['events.emit', null],
  ['network.fetch', PluginPermission.NETWORK],
  ['utils.loadImage', null],
  ['file.saveAs', PluginPermission.STORAGE],
  ['file.saveImage', PluginPermission.STORAGE],
  ['file.openScreenshotsDirectory', null],
  ['clipboard.writeImage', PluginPermission.STORAGE],
  ['clipboard.writeText', PluginPermission.STORAGE],
])

/**
 * 默认 Worker 工厂:blob URL 内联 Worker (vite `?worker&inline`)。
 *
 * 安全原因: Tauri 的 CSP 以 meta 标签注入主文档,资产协议不为 JS 文件注入 CSP 头;
 * 经普通 URL 加载的 Worker,其 CSP 只来自脚本响应自身 → Worker 内完全没有
 * CSP 约束,插件可用原生 fetch / WebSocket / 远程动态 import() 绕过 NETWORK
 * 权限 (远程 import 的 URL 本身即可携带外传数据)。
 * 而 blob: URL 创建的 Worker 会继承创建文档的 CSP (MDN: CSP in workers),
 * 使 script-src / connect-src 在 Worker 内生效;当前 CSP 的 script-src 已含
 * blob:,blob Worker 的创建也被允许。
 * (dev 模式下 vite 以 dev-server URL 创建 Worker,不继承 CSP,但 dev 为
 * 开发者自身的可信环境;生产构建为 blob URL。)
 */
const defaultWorkerFactory = (): Worker => new SandboxWorker()

/** playerState 镜像推送的节流间隔 */
const PLAYER_STATE_THROTTLE_MS = 300

interface PendingCall {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
}

interface PendingMain {
  resolve: (instance: PluginInstance) => void
  reject: (error: Error) => void
}

export class PluginWorkerHost {
  private worker: Worker | null = null
  private workerFactory: WorkerFactory
  private api: PluginAPI | null = null
  private permissions: string[]
  private pluginId: string
  private code: string

  private initWaiter: Promise<void> | null = null
  private mainWaiter: PendingMain | null = null
  private pendingCalls = new Map<number, PendingCall>()
  private callbackStubs = new Map<number, (...args: unknown[]) => Promise<unknown>>()
  private cbCallPending = new Map<number, PendingCall>()
  private watchStops: WatchStopHandle[] = []
  private nextCbCallId = 1
  private terminated = false

  constructor(
    pluginId: string,
    code: string,
    permissions: string[],
    workerFactory?: WorkerFactory,
  ) {
    this.pluginId = pluginId
    this.code = code
    this.permissions = permissions
    this.workerFactory = workerFactory ?? defaultWorkerFactory
  }

  /** 启动 Worker 并加载插件模块 (语法错误/缺少默认导出在此失败) */
  async init(): Promise<void> {
    if (this.worker) return
    this.terminated = false
    const worker = this.workerFactory()
    this.worker = worker
    worker.onmessage = (event: MessageEvent) => {
      this.handleWorkerMessage(event.data as WorkerToHostMessage)
    }
    worker.onerror = (event: ErrorEvent) => {
      logger.error(`[Plugin:${this.pluginId}] 沙箱 Worker 异常:`, event.message || event)
      this.rejectAllPending(new Error('插件沙箱 Worker 发生未捕获异常'))
    }
    // 宿主→Worker 消息在 Worker 侧反序列化失败时落盘 (默认仅静默丢弃)
    worker.onmessageerror = (event: MessageEvent) => {
      logger.warn(`[Plugin:${this.pluginId}] 沙箱消息反序列化失败:`, event.data)
    }

    const initMsg: HostToWorkerMessage = {
      type: 'init',
      pluginId: this.pluginId,
      permissions: this.permissions,
      code: this.code,
    }
    this.initWaiter = new Promise<void>((resolve, reject) => {
      this.initResolve = resolve
      this.initReject = reject
    })
    worker.postMessage(initMsg)
    await this.initWaiter
  }

  private initResolve: (() => void) | null = null
  private initReject: ((error: Error) => void) | null = null

  /** 返回给 pluginManager.register 的主函数 */
  createMainFunction(): (api: PluginAPI) => Promise<PluginInstance> {
    return async (api: PluginAPI) => this.runMain(api)
  }

  /** 在 Worker 中执行插件工厂,返回 activate/deactivate 均为 RPC 代理的实例 */
  async runMain(api: PluginAPI): Promise<PluginInstance> {
    if (this.terminated || !this.worker) {
      // 上一次 activate 失败或 deactivate 已 terminate:重建 Worker 重试
      this.worker = null
      await this.init()
    }
    this.api = api
    this.startMirrorWatchers()
    this.pushMirror(this.collectMirror())

    const mainPromise = new Promise<PluginInstance>((resolve, reject) => {
      this.mainWaiter = { resolve, reject }
    })
    this.post({ type: 'run-main' })
    return mainPromise
  }

  /** 适配 pluginManager 的 PluginSandbox 接口 (execute 在主窗口闭包执行,cleanup 终止 Worker) */
  getSandboxAdapter(): PluginSandbox {
    return {
      globals: Object.freeze({}) as PluginSandbox['globals'],
      execute: async <T>(fn: PluginMainFunction | (() => T | Promise<T>)) => {
        return (await (fn as () => T | Promise<T>)()) as T | PluginInstance
      },
      cleanup: () => this.terminate(),
    }
  }

  /** 终止 Worker 并释放所有资源 (幂等) */
  terminate(): void {
    this.terminated = true
    for (const stop of this.watchStops) stop()
    this.watchStops = []
    this.worker?.terminate()
    this.worker = null
    this.rejectAllPending(new Error('插件沙箱已终止'))
    this.callbackStubs.clear()
    this.api = null
  }

  // -------------------------------------------------------------------------
  // 内部:消息分发
  // -------------------------------------------------------------------------

  private post(msg: HostToWorkerMessage): void {
    this.worker?.postMessage(msg)
  }

  private rejectAllPending(error: Error): void {
    this.initReject?.(error)
    this.initResolve = null
    this.initReject = null
    this.mainWaiter?.reject(error)
    this.mainWaiter = null
    for (const { reject } of this.pendingCalls.values()) reject(error)
    this.pendingCalls.clear()
    for (const { reject } of this.cbCallPending.values()) reject(error)
    this.cbCallPending.clear()
  }

  private handleWorkerMessage(msg: WorkerToHostMessage): void {
    if (!msg || typeof msg.type !== 'string') return
    switch (msg.type) {
      case 'init-result':
        if (msg.ok) {
          this.initResolve?.()
        } else {
          this.initReject?.(deserializeError(msg.error))
        }
        this.initResolve = null
        this.initReject = null
        break

      case 'main-result':
        if (msg.ok) {
          this.mainWaiter?.resolve(
            reviveInstance(msg.instance, (cbId) => this.makeCallbackStub(cbId)),
          )
        } else {
          this.mainWaiter?.reject(deserializeError(msg.error))
        }
        this.mainWaiter = null
        break

      case 'api-call':
        void this.handleApiCall(msg.callId, msg.path, msg.args)
        break

      case 'callback-result': {
        const pending = this.cbCallPending.get(msg.callId)
        this.cbCallPending.delete(msg.callId)
        if (!pending) return
        if (msg.ok) {
          pending.resolve(reviveValue(msg.value, (cbId) => this.makeCallbackStub(cbId)))
        } else {
          pending.reject(deserializeError(msg.error))
        }
        break
      }

      case 'log': {
        const args = msg.args ?? []
        logger[msg.level](`[Plugin:${this.pluginId}]`, ...args)
        break
      }
    }
  }

  /** Worker 代理发起的 API 调用 → 真实 PluginAPI (宿主侧白名单 + 权限强制校验) */
  private async handleApiCall(callId: number, path: string, rawArgs: unknown[]): Promise<void> {
    try {
      // 可信侧强制校验:Worker 内的消息可能来自插件伪造 (共享全局作用域),
      // 先校验消息形状,再查路径白名单与权限,最后进入真实 API (其内部还有逐方法校验)
      if (typeof path !== 'string' || !Array.isArray(rawArgs)) {
        throw new Error('非法的沙箱 API 调用消息 (path/args 形状不合法)')
      }
      const requiredPermission = API_CALL_POLICY.get(path)
      if (requiredPermission === undefined) {
        throw new Error(`未知或禁止的插件 API 调用: ${String(path)}`)
      }
      if (requiredPermission !== null && !this.hasPerm(requiredPermission)) {
        throw new Error(
          `插件 ${this.pluginId} 没有 ${requiredPermission} 权限，无法调用 ${String(path)}`,
        )
      }
      let args = rawArgs.map((arg) => reviveValue(arg, (cbId) => this.makeCallbackStub(cbId)))
      // Worker 侧绘制的 OffscreenCanvas 转为 Blob (真实 API 只认 Blob/HTMLCanvasElement)
      if (path === 'file.saveImage' || path === 'clipboard.writeImage') {
        args = await PluginWorkerHost.flattenCanvasArgs(args)
      }
      let value = await this.invokeApi(path, args)
      value = await this.adaptReturnValue(path, value)
      this.post({ type: 'api-result', callId, ok: true, value })
    } catch (error) {
      logger.debug(`[Plugin:${this.pluginId}] 沙箱 API 调用失败: ${path}`, error)
      this.post({
        type: 'api-result',
        callId,
        ok: false,
        error: {
          name: error instanceof Error ? error.name : 'Error',
          message: error instanceof Error ? error.message : String(error),
          stack: error instanceof Error ? error.stack : undefined,
        },
      })
    }
  }

  private async invokeApi(path: string, args: unknown[]): Promise<unknown> {
    const api = this.api
    if (!api) throw new Error(`插件沙箱未就绪,无法调用 ${path}`)
    const parts = path.split('.')
    let target: unknown = api
    for (const key of parts.slice(0, -1)) {
      if (target == null || typeof target !== 'object') break
      target = (target as Record<string, unknown>)[key]
    }
    const methodKey = parts[parts.length - 1] ?? ''
    const method =
      target && typeof target === 'object'
        ? (target as Record<string, unknown>)[methodKey]
        : undefined
    if (typeof method !== 'function') {
      throw new Error(`未知的插件 API: ${path}`)
    }
    return await (method as (...a: unknown[]) => unknown).apply(target, args)
  }

  /** 返回值适配:不可结构化克隆的主窗口对象转换为可传输表示 */
  private async adaptReturnValue(path: string, value: unknown): Promise<unknown> {
    // Response → 序列化表示 (Worker 侧还原为类 Response 对象)
    if (path === 'network.fetch' && typeof Response !== 'undefined' && value instanceof Response) {
      const headers: Record<string, string> = {}
      value.headers.forEach((v, k) => {
        headers[k] = v
      })
      return {
        [SANDBOX_RESPONSE_MARKER]: true,
        ok: value.ok,
        status: value.status,
        statusText: value.statusText,
        url: value.url,
        headers,
        body: await value.arrayBuffer(),
      }
    }
    // HTMLImageElement → ImageBitmap (可克隆,drawImage 兼容)
    if (
      path === 'utils.loadImage' &&
      typeof HTMLImageElement !== 'undefined' &&
      value instanceof HTMLImageElement
    ) {
      return await createImageBitmap(value)
    }
    return value
  }

  /** 参数中的 OffscreenCanvas (Worker 侧绘制结果) 转为 Blob 后交给真实 API */
  private static async flattenCanvasArgs(args: unknown[]): Promise<unknown[]> {
    const result: unknown[] = []
    for (const arg of args) {
      if (typeof OffscreenCanvas !== 'undefined' && arg instanceof OffscreenCanvas) {
        result.push(await arg.convertToBlob())
      } else if (Array.isArray(arg)) {
        result.push(await PluginWorkerHost.flattenCanvasArgs(arg))
      } else {
        result.push(arg)
      }
    }
    return result
  }

  // -------------------------------------------------------------------------
  // 内部:回调桥
  // -------------------------------------------------------------------------

  /** 主窗口侧回调 stub:调用时 RPC 到 Worker 内执行插件函数 */
  private makeCallbackStub(cbId: number): (...args: unknown[]) => Promise<unknown> {
    const existing = this.callbackStubs.get(cbId)
    if (existing) return existing

    const stub = (...args: unknown[]): Promise<unknown> =>
      new Promise<unknown>((resolve, reject) => {
        if (!this.worker || this.terminated) {
          reject(new Error('插件沙箱已终止'))
          return
        }
        // callId 与 cbId 是两个独立的 id 空间:
        // callId 关联本次挂起调用 (callback-result 按 callId 回执),cbId 定位 Worker 内回调
        const callId = this.nextCbCallId++
        this.cbCallPending.set(callId, { resolve, reject })
        try {
          this.post({ type: 'callback-call', callId, cbId, args })
        } catch (error) {
          this.cbCallPending.delete(callId)
          reject(
            new Error(
              `回调参数无法序列化传输: ${error instanceof Error ? error.message : String(error)}`,
            ),
          )
        }
      })

    this.callbackStubs.set(cbId, stub)
    return stub
  }

  // -------------------------------------------------------------------------
  // 内部:状态镜像
  // -------------------------------------------------------------------------

  private hasPerm(permission: string): boolean {
    return this.permissions.includes(permission)
  }

  private collectMirror(): Partial<MirrorData> {
    const api = this.api
    if (!api) return {}
    const data: Partial<MirrorData> = {
      theme: api.theme.getCurrent(),
      colors: api.theme.getAllColors(),
    }
    if (this.hasPerm(PluginPermission.PLAYER_READ)) {
      data.playerState = api.player.getState()
      data.currentLyricIndex = api.player.getCurrentLyricIndex()
    }
    if (this.hasPerm(PluginPermission.LIBRARY_READ)) {
      data.playlists = api.library.getPlaylists()
      data.currentPlaylist = api.library.getCurrentPlaylist()
      data.tracks = api.library.getTracks()
    }
    data.storage = this.hasPerm(PluginPermission.STORAGE) ? api.storage.getAll() : null
    return data
  }

  private pushMirror(data: Partial<MirrorData>): void {
    try {
      this.post({ type: 'mirror', data })
    } catch (error) {
      logger.warn(`[Plugin:${this.pluginId}] 状态镜像推送失败:`, error)
    }
  }

  private startMirrorWatchers(): void {
    if (this.watchStops.length > 0) return // 已在运行 (重复 activate)

    const playerStore = usePlayerStore()
    const musicLibraryStore = useMusicLibraryStore()
    const themeStore = useThemeStore()

    // playerState 节流推送 (currentTime 高频变化)
    let lastPush = 0
    let rescheduled = false
    const pushPlayerState = (): void => {
      const now = Date.now()
      const elapsed = now - lastPush
      if (elapsed >= PLAYER_STATE_THROTTLE_MS) {
        lastPush = now
        const api = this.api
        if (api && this.hasPerm(PluginPermission.PLAYER_READ)) {
          this.pushMirror({
            playerState: api.player.getState(),
            currentLyricIndex: api.player.getCurrentLyricIndex(),
          })
        }
      } else if (!rescheduled) {
        rescheduled = true
        setTimeout(() => {
          rescheduled = false
          pushPlayerState()
        }, PLAYER_STATE_THROTTLE_MS - elapsed)
      }
    }

    this.watchStops.push(
      watch(
        () => [
          playerStore.currentTrack,
          playerStore.isPlaying,
          playerStore.currentTime,
          playerStore.duration,
          playerStore.volume,
          playerStore.repeatMode,
          playerStore.isShuffle,
        ],
        pushPlayerState,
      ),
      watch(() => playerStore.currentLyricIndex, pushPlayerState),
    )

    // 主题变化 → 重推 theme + colors (applyTheme 同步更新 CSS 变量)
    this.watchStops.push(
      watch(
        () => [themeStore.themePreference, themeStore.isDarkMode, themeStore.primaryColor],
        () => {
          const api = this.api
          if (api) {
            this.pushMirror({ theme: api.theme.getCurrent(), colors: api.theme.getAllColors() })
          }
        },
      ),
    )

    // 音乐库/播放列表 (浅层:数组引用替换时触发)
    const pushLibrary = (): void => {
      const api = this.api
      if (api && this.hasPerm(PluginPermission.LIBRARY_READ)) {
        this.pushMirror({
          playlists: api.library.getPlaylists(),
          currentPlaylist: api.library.getCurrentPlaylist(),
          tracks: api.library.getTracks(),
        })
      }
    }
    this.watchStops.push(
      watch(() => musicLibraryStore.playlists, pushLibrary),
      watch(() => musicLibraryStore.currentPlaylist, pushLibrary),
      watch(() => playerStore.playlist, pushLibrary),
    )
  }
}

// ---------------------------------------------------------------------------
// 序列化辅助 (主窗口侧)
// ---------------------------------------------------------------------------

type StubFactory = (cbId: number) => (...args: unknown[]) => unknown

/** 还原 Worker 传来的值:函数句柄标记 → 调用 Worker 的 stub 函数 */
function reviveValue(value: unknown, makeStub: StubFactory): unknown {
  if (Array.isArray(value)) {
    return value.map((item) => reviveValue(item, makeStub))
  }
  if (value && typeof value === 'object') {
    const proto = Object.getPrototypeOf(value)
    if (proto !== Object.prototype && proto !== null) return value
    const record = value as Record<string, unknown>
    const fnId = record[SANDBOX_FN_MARKER]
    if (typeof fnId === 'number') {
      return makeStub(fnId)
    }
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(record)) {
      out[key] = reviveValue(item, makeStub)
    }
    return out
  }
  return value
}

function reviveInstance(value: unknown, makeStub: StubFactory): PluginInstance {
  return reviveValue(value, makeStub) as PluginInstance
}
