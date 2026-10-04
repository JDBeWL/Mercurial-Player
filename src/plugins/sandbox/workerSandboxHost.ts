/**
 * 插件沙箱宿主,主窗口侧:为单个外置插件管理一个 Dedicated Worker。
 * 职责:把 Worker 内代理发起的 api-call 分发到真实 PluginAPI,按 MirrorData 推送状态镜像,
 * 把插件注册的函数还原为调用 Worker 的 stub,deactivate 时 terminate Worker (定时器与监听随之释放)。
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
import type { HostToWorkerMessage, MirrorData, SerializedError } from './sandboxProtocol'
import { SANDBOX_FN_MARKER, SANDBOX_RESPONSE_MARKER, deserializeError } from './sandboxProtocol'
// blob URL 内联 Worker: 安全考量见下方 defaultWorkerFactory 注释
import SandboxWorker from './workerBootstrap?worker&inline'

/** Worker 工厂 (注入点:测试环境替换为 FakeWorker) */
export type WorkerFactory = () => Worker

/**
 * 沙箱 api-call 路径白名单:精确路径 -> 所需权限 (null = 无需权限)。
 * 权限校验的权威位置在宿主主窗口侧:Worker 内的 requirePermission 与插件共享全局作用域,
 * 可被插件直接 postMessage 伪造 api-call 绕过。
 * 精确匹配取代属性链遍历,未收录的路径 (log.info.constructor 等) 一律拒绝;用 Map 存储以免原型链键干扰查找。
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
  ['file.saveAs', PluginPermission.FILE_WRITE],
  ['file.saveImage', PluginPermission.FILE_WRITE],
  ['file.openScreenshotsDirectory', null],
  ['clipboard.writeImage', PluginPermission.CLIPBOARD_WRITE],
  ['clipboard.writeText', PluginPermission.CLIPBOARD_WRITE],
])

/**
 * 默认 Worker 工厂:blob URL 内联 Worker (vite `?worker&inline`)。
 * Tauri 的 CSP 由 meta 标签注入主文档,资产协议不为 JS 注入 CSP 头,经普通 URL 加载的 Worker 内全无 CSP 约束,
 * 插件即可用原生 fetch / WebSocket / 远程动态 import() 绕过 NETWORK 权限;
 * blob: URL 的 Worker 继承创建文档的 CSP (MDN: CSP in workers),使 script-src / connect-src 在 Worker 内生效,
 * 当前 CSP script-src 已含 blob:。dev 下 vite 用 dev-server URL 建 Worker 不继承 CSP,但那是开发者自身可信环境。
 */
const defaultWorkerFactory = (): Worker => new SandboxWorker()

/** playerState 镜像推送的节流间隔 */
const PLAYER_STATE_THROTTLE_MS = 300

/** init(Worker 启动 + 模块加载)超时 */
const INIT_TIMEOUT_MS = 5_000
/** runMain(插件工厂执行)超时 */
const MAIN_TIMEOUT_MS = 30_000
/** 宿主->Worker 回调 RPC(activate/deactivate/事件回调等)超时 */
const CALLBACK_TIMEOUT_MS = 30_000

/**
 * 上行消息令牌桶参数:log 与 api-call 是插件唯一能持续刷向宿主的通道,可刷爆主窗口事件循环与日志文件;
 * 超过补充速率视为洪泛,直接终止 Worker (失败关闭)。
 */
const LOG_BUCKET_CAPACITY = 200
const LOG_BUCKET_REFILL_PER_SEC = 100
const API_CALL_BUCKET_CAPACITY = 400
const API_CALL_BUCKET_REFILL_PER_SEC = 200
/** 宿主侧同时存活的回调 stub 上限(事件监听/返回值携带函数等),防内存刷爆;Worker 侧对应 workerCore 的 MAX_REGISTERED_CALLBACKS */
const MAX_CALLBACK_STUBS = 10_000
/** 单个插件可注册的扩展点上限(菜单/按钮/命令/快捷键/歌词源等) */
const EXTENSION_LIMIT = 200
/** 计入扩展点配额的注册类 API(注册消耗配额) */
const EXTENSION_REGISTER_PATHS: ReadonlySet<string> = new Set([
  'ui.registerMenuItem',
  'ui.registerActionButton',
  'lyrics.registerProvider',
  'commands.register',
  'shortcuts.register',
])
/** 注销类 API(归还配额) */
const EXTENSION_UNREGISTER_PATHS: ReadonlySet<string> = new Set([
  'ui.unregisterActionButton',
  'shortcuts.unregister',
])

/** 令牌桶:按固定速率补充令牌至容量上限,取令牌失败即超限 */
class TokenBucket {
  private tokens: number
  private lastRefill = Date.now()

  constructor(
    private readonly capacity: number,
    private readonly refillPerSecond: number,
  ) {
    this.tokens = capacity
  }

  tryTake(): boolean {
    const now = Date.now()
    const elapsed = Math.max(0, (now - this.lastRefill) / 1000)
    this.lastRefill = now
    this.tokens = Math.min(this.capacity, this.tokens + elapsed * this.refillPerSecond)
    if (this.tokens >= 1) {
      this.tokens -= 1
      return true
    }
    return false
  }
}

/**
 * 反序列化 (reviveValue) 的资源预算:插件可自行 postMessage 任意 payload,
 * 深度嵌套对象会让无限制递归直接抛 RangeError,该异常从 onmessage 逃逸后等待中的 Promise 永不 settle;
 * 故预算耗尽时改抛可捕获的普通 Error,由调用方拒绝等待者。
 */
const REVIVE_MAX_DEPTH = 32
const REVIVE_MAX_NODES = 20_000

/** 允许插件使用的日志级别白名单 (替代 logger[msg.level] 的任意索引) */
const LOG_LEVELS = new Set(['error', 'warn', 'info', 'debug'])

type LogLevel = 'error' | 'warn' | 'info' | 'debug'

// ---- 不可信消息校验 ----

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

/** callId 必须是安全非负整数:它是挂起 Promise 的查找键 */
function isCallId(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

function isSerializedError(value: unknown): value is SerializedError {
  return isRecord(value) && typeof value.name === 'string' && typeof value.message === 'string'
}

/** 超时即 reject,调用方负责 terminate 清理 */
function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`${label} 超时 (${ms}ms)，插件可能已挂起`))
    }, ms)
    promise.then(
      (value) => {
        clearTimeout(timer)
        resolve(value)
      },
      (error) => {
        clearTimeout(timer)
        reject(error)
      },
    )
  })
}

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
  /** events.on 注册回调的引用计数:events.off 归零时同步释放对应 stub */
  private callbackStubRefs = new Map<number, number>()
  private cbCallPending = new Map<number, PendingCall>()
  private watchStops: WatchStopHandle[] = []
  private nextCbCallId = 1
  private terminated = false
  private logBucket = new TokenBucket(LOG_BUCKET_CAPACITY, LOG_BUCKET_REFILL_PER_SEC)
  private apiCallBucket = new TokenBucket(API_CALL_BUCKET_CAPACITY, API_CALL_BUCKET_REFILL_PER_SEC)
  /** 本插件剩余可注册的扩展点配额(init 时重置) */
  private extensionSlots = EXTENSION_LIMIT

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
    this.extensionSlots = EXTENSION_LIMIT
    const worker = this.workerFactory()
    this.worker = worker
    worker.onmessage = (event: MessageEvent) => {
      // 不做类型断言:消息来自不可信侧,由 handleWorkerMessage 逐字段校验
      this.handleWorkerMessage(event.data)
    }
    worker.onerror = (event: ErrorEvent) => {
      logger.error(`[Plugin:${this.pluginId}] 沙箱 Worker 异常:`, event.message || event)
      this.rejectAllPending(new Error('插件沙箱 Worker 发生未捕获异常'))
    }
    // 宿主->Worker 消息在 Worker 侧反序列化失败时落盘 (默认仅静默丢弃)
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
    // 超时保护:插件模块加载若挂起(如顶层死循环),reject 由调用方 terminate
    await withTimeout(this.initWaiter, INIT_TIMEOUT_MS, `插件 ${this.pluginId} 初始化`)
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
    // 超时保护:插件工厂函数若死循环,resolve 永远不会到达
    return withTimeout(mainPromise, MAIN_TIMEOUT_MS, `插件 ${this.pluginId} 主函数执行`)
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
    this.callbackStubRefs.clear()
    this.api = null
  }

  // ---- 内部:消息分发 ----

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

  /**
   * 处理来自 Worker 的消息:先按类型逐一校验字段形状,不合法直接丢弃 (见 dropMalformed);
   * revive 预算见 REVIVE_MAX_DEPTH / REVIVE_MAX_NODES;整体 try/catch,残留异常必须拒绝全部
   * 挂起 Promise 并 terminate,否则插件会永久卡在半初始化状态且毫无报错。
   */
  private handleWorkerMessage(raw: unknown): void {
    if (!isRecord(raw) || typeof raw.type !== 'string') return
    const type = raw.type

    // 令牌桶限速,参数与失败关闭策略见 LOG_BUCKET_* / API_CALL_BUCKET_*
    if (type === 'log' || type === 'api-call') {
      const bucket = type === 'log' ? this.logBucket : this.apiCallBucket
      if (!bucket.tryTake()) {
        logger.error(`[Plugin:${this.pluginId}] ${type} 消息速率超限,终止插件沙箱`)
        this.rejectAllPending(new Error(`插件 ${type} 消息速率超限`))
        this.terminate()
        return
      }
    }

    try {
      switch (type) {
        case 'init-result': {
          if (typeof raw.ok !== 'boolean') return this.dropMalformed(type, 'ok')
          if (raw.ok) {
            this.initResolve?.()
          } else {
            this.initReject?.(
              deserializeError(
                isSerializedError(raw.error)
                  ? raw.error
                  : { name: 'Error', message: '插件沙箱初始化失败' },
              ),
            )
          }
          this.initResolve = null
          this.initReject = null
          break
        }

        case 'main-result': {
          if (typeof raw.ok !== 'boolean') return this.dropMalformed(type, 'ok')
          const waiter = this.mainWaiter
          this.mainWaiter = null
          if (!waiter) break
          if (raw.ok) {
            waiter.resolve(reviveInstance(raw.instance, (cbId) => this.makeCallbackStub(cbId)))
          } else {
            waiter.reject(
              deserializeError(
                isSerializedError(raw.error)
                  ? raw.error
                  : { name: 'Error', message: '插件主函数执行失败' },
              ),
            )
          }
          break
        }

        case 'api-call': {
          // callId 不合法时无处投递回执,只能丢弃;path/args 由 handleApiCall 校验形状并回 ok:false,
          // 否则 Worker 侧挂起的 Promise 会一直等下去
          if (!isCallId(raw.callId)) return this.dropMalformed(type, 'callId')
          void this.handleApiCall(raw.callId, raw.path as string, raw.args as unknown[])
          break
        }

        case 'callback-result': {
          if (!isCallId(raw.callId)) return this.dropMalformed(type, 'callId')
          const pending = this.cbCallPending.get(raw.callId)
          this.cbCallPending.delete(raw.callId)
          if (!pending) break
          if (raw.ok === true) {
            pending.resolve(reviveValue(raw.value, (cbId) => this.makeCallbackStub(cbId)))
          } else if (raw.ok === false) {
            pending.reject(
              deserializeError(
                isSerializedError(raw.error)
                  ? raw.error
                  : { name: 'Error', message: '插件回调执行失败' },
              ),
            )
          } else {
            return this.dropMalformed(type, 'ok')
          }
          break
        }

        case 'log': {
          if (!Array.isArray(raw.args)) return this.dropMalformed(type, 'args')
          const level: LogLevel = LOG_LEVELS.has(raw.level as string)
            ? (raw.level as LogLevel)
            : 'info'
          logger[level](`[Plugin:${this.pluginId}]`, ...raw.args)
          break
        }

        default:
          logger.warn(`[Plugin:${this.pluginId}] 忽略未知的沙箱消息类型: ${type}`)
      }
    } catch (error) {
      logger.error(`[Plugin:${this.pluginId}] 处理沙箱消息 "${type}" 时出错:`, error)
      this.rejectAllPending(
        new Error(
          `沙箱消息 "${type}" 处理失败: ${error instanceof Error ? error.message : String(error)}`,
        ),
      )
      this.terminate()
    }
  }

  /** 丢弃形状不合法的沙箱消息:不改变宿主状态,不终止插件 */
  private dropMalformed(type: string, field: string): void {
    logger.warn(`[Plugin:${this.pluginId}] 沙箱消息 "${type}" 的字段 ${field} 不合法,已丢弃`)
  }

  /** Worker 代理发起的 API 调用 -> 真实 PluginAPI (宿主侧白名单 + 权限强制校验) */
  private async handleApiCall(callId: number, path: string, rawArgs: unknown[]): Promise<void> {
    // extReserved:本调用是否已预扣扩展配额,需在 try 外声明以便 catch 退还
    let extReserved = false
    try {
      // 校验顺序:消息形状 -> API_CALL_POLICY 白名单与权限 -> 真实 API (其内部还有逐方法校验)
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
      // 扩展点配额:注册消耗配额,注销归还配额,防插件无限堆 UI/命令/歌词源。
      // 并发下 (连续多条 api-call 尚未结算) 必须先扣减预留再检查,否则竞态可让实际调用数越过上限;
      // 后续执行失败 (如 PluginAPI 校验拒绝) 时在 catch 退还。
      const isExtRegister = EXTENSION_REGISTER_PATHS.has(path)
      const isExtUnregister = EXTENSION_UNREGISTER_PATHS.has(path)
      if (isExtRegister) {
        if (this.extensionSlots <= 0) {
          throw new Error(
            `插件 ${this.pluginId} 扩展注册数量超过上限 (${EXTENSION_LIMIT})，请先注销不再使用的扩展`,
          )
        }
        this.extensionSlots -= 1
        extReserved = true
      } // events.on/off 的 cbId 引用计数见 callbackStubRefs:此处 +1,注销成功后才 -1
      const subCbId =
        path === 'events.on' || path === 'events.off' ? this.extractCallbackId(rawArgs) : null
      if (path === 'events.on' && subCbId !== null) {
        this.callbackStubRefs.set(subCbId, (this.callbackStubRefs.get(subCbId) ?? 0) + 1)
      }
      let args = rawArgs.map((arg) => reviveValue(arg, (cbId) => this.makeCallbackStub(cbId)))
      // OffscreenCanvas 转 Blob 的取舍见 flattenCanvasArgs
      if (path === 'file.saveImage' || path === 'clipboard.writeImage') {
        args = await PluginWorkerHost.flattenCanvasArgs(args)
      }
      let value = await this.invokeApi(path, args)
      // 注销归还配额,注册配额已在进入时预留
      if (isExtUnregister) {
        this.extensionSlots = Math.min(EXTENSION_LIMIT, this.extensionSlots + 1)
      }
      // events.off 只在注销成功后归还引用,失败 (如权限拒绝) 时保持 stub 存活
      if (path === 'events.off' && subCbId !== null) {
        const remain = (this.callbackStubRefs.get(subCbId) ?? 1) - 1
        if (remain <= 0) {
          this.callbackStubRefs.delete(subCbId)
          this.callbackStubs.delete(subCbId)
        } else {
          this.callbackStubRefs.set(subCbId, remain)
        }
      }
      value = await this.adaptReturnValue(path, value)
      this.post({ type: 'api-result', callId, ok: true, value })
    } catch (error) {
      // 退还本调用预扣的注册配额
      if (extReserved) this.extensionSlots += 1
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
    // Response -> 序列化表示 (Worker 侧还原为类 Response 对象)
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
    // HTMLImageElement -> ImageBitmap (可克隆,drawImage 兼容)
    if (
      path === 'utils.loadImage' &&
      typeof HTMLImageElement !== 'undefined' &&
      value instanceof HTMLImageElement
    ) {
      return await createImageBitmap(value)
    }
    return value
  }

  /** 从 api-call 原始参数中提取沙箱函数标记的 cbId(仅 events.on/off 的第二个参数) */
  private extractCallbackId(rawArgs: unknown[]): number | null {
    const marker = rawArgs[1]
    if (!isRecord(marker)) return null
    const id = marker[SANDBOX_FN_MARKER]
    if (typeof id === 'number' && Number.isSafeInteger(id) && id >= 0) return id
    return null
  }

  /** 递归把参数中的 OffscreenCanvas 转为 Blob:真实 API 只认 Blob/HTMLCanvasElement,Worker 侧画布无法直接投递 */
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

  // ---- 内部:回调桥 ----

  /** 主窗口侧回调 stub:调用时 RPC 到 Worker 内执行插件函数 */
  private makeCallbackStub(cbId: number): (...args: unknown[]) => Promise<unknown> {
    const existing = this.callbackStubs.get(cbId)
    if (existing) return existing
    // stub 数量上限见 MAX_CALLBACK_STUBS
    if (this.callbackStubs.size >= MAX_CALLBACK_STUBS) {
      throw new Error(`插件 ${this.pluginId} 回调数量超过上限 (${MAX_CALLBACK_STUBS})`)
    }

    const stub = (...args: unknown[]): Promise<unknown> =>
      new Promise<unknown>((resolve, reject) => {
        if (!this.worker || this.terminated) {
          reject(new Error('插件沙箱已终止'))
          return
        }
        // callId 与 cbId 是两个独立 id 空间,分工见 sandboxProtocol 的 callback-call 变体
        const callId = this.nextCbCallId++
        // 超时保护:插件回调 (activate/deactivate/事件处理) 死循环时只拒绝本次调用,避免宿主调用方永久挂起;
        // 不 terminate 整个 Worker,单次回调失败仍可恢复
        const timer = setTimeout(() => {
          if (this.cbCallPending.delete(callId)) {
            reject(new Error(`插件 ${this.pluginId} 回调执行超时 (${CALLBACK_TIMEOUT_MS}ms)`))
          }
        }, CALLBACK_TIMEOUT_MS)
        const wrappedResolve: (value: unknown) => void = (value) => {
          clearTimeout(timer)
          resolve(value)
        }
        const wrappedReject: (error: Error) => void = (error) => {
          clearTimeout(timer)
          reject(error)
        }
        this.cbCallPending.set(callId, { resolve: wrappedResolve, reject: wrappedReject })
        try {
          this.post({ type: 'callback-call', callId, cbId, args })
        } catch (error) {
          clearTimeout(timer)
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

  // ---- 内部:状态镜像 ----

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

  /**
   * 推送状态镜像:postMessage 走结构化克隆,任一字段不可克隆 (函数/异常代理值等) 整条消息就会失败,
   * 故失败时先丢弃最不可控的 storage 字段降级重试,保证核心状态仍能送达。
   */
  private pushMirror(data: Partial<MirrorData>): void {
    try {
      this.post({ type: 'mirror', data })
    } catch (error) {
      if (data.storage === undefined) {
        logger.warn(`[Plugin:${this.pluginId}] 状态镜像推送失败:`, error)
        return
      }
      try {
        this.post({ type: 'mirror', data: { ...data, storage: null } })
        logger.warn(`[Plugin:${this.pluginId}] 状态镜像 storage 字段不可克隆,已降级推送`)
      } catch (retryError) {
        logger.error(`[Plugin:${this.pluginId}] 状态镜像推送失败:`, retryError)
      }
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

    // 主题变化 -> 重推 theme + colors (applyTheme 同步更新 CSS 变量)
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

// ---- 序列化辅助 (主窗口侧) ----

type StubFactory = (cbId: number) => (...args: unknown[]) => unknown

interface ReviveBudget {
  depth: number
  nodes: number
}

/** 预算耗尽时抛出的错误类型,约束见 REVIVE_MAX_DEPTH / REVIVE_MAX_NODES */
class ReviveLimitError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ReviveLimitError'
  }
}

/**
 * 还原 Worker 传来的值:函数句柄标记 -> 调用 Worker 的 stub 函数。
 * 递归受 REVIVE_MAX_DEPTH / REVIVE_MAX_NODES 预算约束。
 */
function reviveValue(value: unknown, makeStub: StubFactory): unknown {
  return reviveInner(value, makeStub, { depth: 0, nodes: 0 })
}

function reviveInner(value: unknown, makeStub: StubFactory, budget: ReviveBudget): unknown {
  if (value === null || typeof value !== 'object') return value

  budget.nodes += 1
  if (budget.nodes > REVIVE_MAX_NODES) {
    throw new ReviveLimitError(`沙箱消息对象过大（超过 ${REVIVE_MAX_NODES} 个节点）`)
  }
  if (budget.depth >= REVIVE_MAX_DEPTH) {
    throw new ReviveLimitError(`沙箱消息嵌套过深（超过 ${REVIVE_MAX_DEPTH} 层）`)
  }
  budget.depth += 1
  try {
    if (Array.isArray(value)) {
      return value.map((item) => reviveInner(item, makeStub, budget))
    }
    const proto = Object.getPrototypeOf(value)
    if (proto !== Object.prototype && proto !== null) return value
    const record = value as Record<string, unknown>
    const fnId = record[SANDBOX_FN_MARKER]
    if (typeof fnId === 'number') {
      return makeStub(fnId)
    }
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(record)) {
      out[key] = reviveInner(item, makeStub, budget)
    }
    return out
  } finally {
    budget.depth -= 1
  }
}

function reviveInstance(value: unknown, makeStub: StubFactory): PluginInstance {
  return reviveValue(value, makeStub) as PluginInstance
}
