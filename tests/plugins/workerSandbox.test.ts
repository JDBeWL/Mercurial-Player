/**
 * Worker 沙箱协议测试
 *
 * 用 FakeWorker (微任务队列模拟跨线程 postMessage) 把真实的
 * SandboxWorkerRuntime (Worker 侧) 与 PluginWorkerHost (主窗口侧)
 * 连成闭环,端到端验证:
 * - 模块加载与主函数执行
 * - 同步镜像 / 异步 RPC / 回调桥
 * - 权限预检语义、错误传播、生命周期 (terminate/重启)
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { PluginWorkerHost } from '@/plugins/sandbox/workerSandboxHost'
import { SandboxWorkerRuntime, removeNetworkGlobals } from '@/plugins/sandbox/workerCore'
import { toModuleCode } from '@/plugins/moduleExecutor'
import type { PluginAPI, PluginInstance } from '@/plugins/pluginTypes'

/**
 * 沙箱实例的自定义方法视图:activate/deactivate 之外的字段
 * 经回调句柄还原后均为 RPC 代理函数 (索引签名受 noUncheckedIndexedAccess
 * 影响,调用点需非空断言 `!`)
 */
type AsyncInstance = {
  activate?: () => Promise<void>
  deactivate?: () => Promise<void>
} & Record<string, (...args: unknown[]) => Promise<unknown>>

const asAsyncInstance = (instance: PluginInstance): AsyncInstance => instance as AsyncInstance

/** FakeWorker 不满足完整 Worker 接口,以最小成员断言注入 host */
const asWorker = (w: FakeWorker): Worker => w as unknown as Worker

// ---------------------------------------------------------------------------
// FakeWorker:双向消息通道
// ---------------------------------------------------------------------------

class FakeWorker {
  onmessage: ((ev: { data: unknown }) => void) | null = null
  onerror: ((ev: unknown) => void) | null = null
  terminated = false
  /** Worker 侧消息入口 (host → worker) */
  runtimeListener: ((ev: { data: unknown }) => void) | null = null

  postMessage = (data: unknown): void => {
    if (this.terminated) throw new Error('Worker 已终止')
    queueMicrotask(() => this.runtimeListener?.({ data }))
  }

  terminate = (): void => {
    this.terminated = true
  }

  /** Worker 侧发送 (worker → host) */
  emitToHost = (msg: unknown): void => {
    if (this.terminated) return
    queueMicrotask(() => this.onmessage?.({ data: msg }))
  }
}

/** 排空微任务链 (RPC 往返需要多轮微任务) */
const flushAsync = async (): Promise<void> => {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

// ---------------------------------------------------------------------------
// 测试模块加载器:复用真实 toModuleCode 包装,new Function 求值
// (node 环境不支持 blob: 动态 import)
// ---------------------------------------------------------------------------

const testLoader = async (code: string): Promise<(api: unknown, globals: unknown) => unknown> => {
  const moduleCode = toModuleCode(code)
  const body = moduleCode.replace(/export\s+default\s*/, 'return ')
  // 一次调用 = 模拟模块求值,返回默认导出的工厂函数 (与 blob import 语义一致)
  return new Function(body)() as (api: unknown, globals: unknown) => unknown
}

// ---------------------------------------------------------------------------
// Mock PluginAPI
// ---------------------------------------------------------------------------

function createMockApi(): PluginAPI {
  return {
    pluginId: 'test-plugin',
    permissions: [],
    log: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
    player: {
      getState: vi.fn(() => ({
        currentTrack: null,
        isPlaying: true,
        currentTime: 10,
        duration: 100,
        volume: 0.5,
        repeatMode: 'none',
        isShuffle: false,
      })),
      getLyrics: vi.fn(async () => [{ time: 0, texts: [{ text: 'hello' }] }]),
      getCurrentLyricIndex: vi.fn(() => 0),
      getCoverPath: vi.fn(async () => null),
      play: vi.fn(),
      pause: vi.fn(),
      togglePlay: vi.fn(),
      next: vi.fn(async () => {}),
      previous: vi.fn(async () => {}),
      seek: vi.fn(),
      setVolume: vi.fn(),
      setLyrics: vi.fn(),
    },
    library: {
      getPlaylists: vi.fn(() => []),
      getCurrentPlaylist: vi.fn(() => null),
      getTracks: vi.fn(() => []),
    },
    theme: {
      getCurrent: vi.fn(() => ({ preference: 'dark', isDark: true, primaryColor: '#000000' })),
      setColors: vi.fn(async () => {}),
      getCSSVariable: vi.fn(() => ''),
      getAllColors: vi.fn(() => ({ mdSysColorPrimary: '#6750a4' })),
    },
    ui: {
      registerSettingsPanel: vi.fn(),
      registerMenuItem: vi.fn(),
      registerPlayerDecorator: vi.fn(),
      registerActionButton: vi.fn(),
      unregisterActionButton: vi.fn(),
      showNotification: vi.fn(),
    },
    lyrics: { registerProvider: vi.fn() },
    visualizer: { register: vi.fn() },
    commands: { register: vi.fn(), execute: vi.fn(async () => {}) },
    shortcuts: { register: vi.fn(), unregister: vi.fn() },
    storage: {
      get: vi.fn(),
      set: vi.fn(),
      remove: vi.fn(),
      getAll: vi.fn(() => ({ counter: 5 })),
    },
    events: { on: vi.fn(), off: vi.fn(), emit: vi.fn() },
    network: { fetch: vi.fn() },
    utils: {} as PluginAPI['utils'],
    file: {} as PluginAPI['file'],
    clipboard: {} as PluginAPI['clipboard'],
  } as unknown as PluginAPI
}

interface SandboxFixture {
  host: PluginWorkerHost
  worker: FakeWorker
  api: PluginAPI
}

/** 装配完整的 host ↔ runtime 闭环 */
async function setupSandbox(
  pluginCode: string,
  permissions: string[] = [],
  api: PluginAPI = createMockApi(),
  loader: typeof testLoader = testLoader,
): Promise<SandboxFixture> {
  const worker = new FakeWorker()
  const host = new PluginWorkerHost('test-plugin', pluginCode, permissions, () => asWorker(worker))
  const runtime = new SandboxWorkerRuntime((msg) => worker.emitToHost(msg), loader)
  worker.runtimeListener = (ev) => {
    void runtime.handleMessage(ev.data as Parameters<typeof runtime.handleMessage>[0])
  }
  await host.init()
  return { host, worker, api }
}

/** 常规用例:运行主函数并取回代理实例 */
async function runSandboxMain(host: PluginWorkerHost, api: PluginAPI): Promise<AsyncInstance> {
  return asAsyncInstance(await host.runMain(api))
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

describe('workerSandbox - 模块加载与主函数执行', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('init 阶段模块加载失败会 reject (语法错误/缺少默认导出)', async () => {
    const worker = new FakeWorker()
    const host = new PluginWorkerHost('p', 'broken code', [], () => asWorker(worker))
    const runtime = new SandboxWorkerRuntime(
      (msg) => worker.emitToHost(msg),
      async () => {
        throw new Error('插件模块加载失败（代码可能存在语法错误）: boom')
      },
    )
    worker.runtimeListener = (ev) => {
      void runtime.handleMessage(ev.data as Parameters<typeof runtime.handleMessage>[0])
    }
    await expect(host.init()).rejects.toThrow('语法错误')
  })

  it('runMain 返回 activate/deactivate 为 RPC 代理的实例', async () => {
    const code = `
      export default (api) => {
        return {
          activate: () => api.log.info('activated'),
          deactivate: () => api.log.info('deactivated'),
        }
      }
    `
    const { host, api } = await setupSandbox(code)
    const instance = await host.runMain(api)

    expect(typeof instance.activate).toBe('function')
    await instance.activate!()
    await flushAsync()
    expect(api.log.info).not.toHaveBeenCalled() // 日志走 logger 而非 api.log
  })

  it('插件工厂抛错时 runMain reject 且错误信息保留', async () => {
    const code = `export default () => { throw new Error('factory boom') }`
    const { host, api } = await setupSandbox(code)
    await expect(host.runMain(api)).rejects.toThrow('factory boom')
  })

  it('旧脚本格式 (顶层 plugin 变量) 通过包装后正常执行', async () => {
    const code = `
      var plugin = { activate: function () {} }
    `
    const { host, api } = await setupSandbox(code)
    const instance = await host.runMain(api)
    expect(typeof instance.activate).toBe('function')
  })
})

describe('workerSandbox - 镜像与 RPC', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('同步读 API 从镜像取值 (player.getState 不触发主窗口调用)', async () => {
    const code = `
      export default (api) => ({
        readState: () => api.player.getState(),
        readLyrics: () => api.player.getLyrics(),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, ['player:read'], api)
    const instance = await runSandboxMain(host, api)

    const state = (await instance.readState!()) as { isPlaying: boolean }
    expect(state.isPlaying).toBe(true)
    expect(api.player.getState).toHaveBeenCalledTimes(1) // 仅镜像采集时调用

    // 异步 API 走 RPC
    const lyrics = await instance.readLyrics!()
    expect(api.player.getLyrics).toHaveBeenCalled()
    expect((lyrics as { texts: { text: string }[] }[])[0]!.texts[0]!.text).toBe('hello')
  })

  it('无权限时同步 API 在沙箱内直接抛出权限错误', async () => {
    const code = `
      export default (api) => ({
        readState: () => api.player.getState(),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, [], api) // 无 player:read
    const instance = await runSandboxMain(host, api)

    await expect(instance.readState!()).rejects.toThrow('没有 player:read 权限')
    // 镜像未采集,主窗口 getState 从未被调用
    expect(api.player.getState).not.toHaveBeenCalled()
  })

  it('storage.get 读初始镜像,set 本地生效并 RPC 持久化', async () => {
    const code = `
      export default (api) => ({
        read: (key) => api.storage.get(key),
        write: (key, value) => api.storage.set(key, value),
        readAfterWrite: (key) => api.storage.get(key),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, ['storage'], api)
    const instance = await runSandboxMain(host, api)

    expect(await instance.read!('counter')).toBe(5) // 来自 api.storage.getAll 镜像

    await instance.write!('counter', 6)
    await flushAsync()
    expect(api.storage.set).toHaveBeenCalledWith('counter', 6)
    // Worker 内镜像立即生效 (无需等待主窗口回执)
    expect(await instance.readAfterWrite!('counter')).toBe(6)
  })

  it('命令类同步 API (player.play) 触发主窗口调用', async () => {
    const code = `
      export default (api) => ({
        doPlay: () => api.player.play(),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, ['player:control'], api)
    const instance = await runSandboxMain(host, api)

    await instance.doPlay!()
    await flushAsync()
    expect(api.player.play).toHaveBeenCalledTimes(1)
  })
})

describe('workerSandbox - 回调桥', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('events.on 注册的回调经句柄还原为主窗口 stub,调用经 RPC 回到 Worker', async () => {
    const code = `
      export default (api) => {
        const handler = (data) => ({ received: data })
        return {
          activate: () => api.events.on('player:trackChanged', handler),
        }
      }
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, [], api)
    const instance = await runSandboxMain(host, api)

    await instance.activate!()
    await flushAsync()
    expect(api.events.on).toHaveBeenCalledWith('player:trackChanged', expect.any(Function))

    // 模拟 manager.emit 调用 stub:数据经 RPC 到 Worker 执行并返回结果
    const stub = (api.events.on as ReturnType<typeof vi.fn>).mock.calls[0]![1] as (
      data: unknown,
    ) => Promise<unknown>
    const result = (await stub({ track: 'song-a' })) as { received: { track: string } }
    expect(result.received.track).toBe('song-a')
  })

  it('lyrics.registerProvider 注册的 search 回调可被主窗口调用', async () => {
    const code = `
      export default (api) => ({
        activate: () => api.lyrics.registerProvider({
          id: 'my-source',
          name: 'My Source',
          search: async (query) => [{ id: '1', title: query.title }],
        }),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, ['lyrics:provider'], api)
    const instance = await runSandboxMain(host, api)

    await instance.activate!()
    await flushAsync()
    expect(api.lyrics.registerProvider).toHaveBeenCalledWith(
      expect.objectContaining({ id: 'my-source', search: expect.any(Function) }),
    )

    const provider = (api.lyrics.registerProvider as ReturnType<typeof vi.fn>).mock
      .calls[0]![0] as { search: (q: { title: string }) => Promise<unknown[]> }
    const results = (await provider.search({ title: '歌名' })) as { title: string }[]
    expect(results[0]!.title).toBe('歌名')
  })

  it('沙箱插件调用不支持的 UI 组件注册 API 时抛出明确错误', async () => {
    const code = `
      export default (api) => ({
        tryRegisterPanel: () => api.ui.registerSettingsPanel({ id: 'x', name: 'x' }),
        tryRegisterVisualizer: () => api.visualizer.register({ id: 'v', name: 'v' }),
      })
    `
    const api = createMockApi()
    const { host } = await setupSandbox(code, ['ui:extend', 'visualizer'], api)
    const instance = await runSandboxMain(host, api)

    await expect(instance.tryRegisterPanel!()).rejects.toThrow('沙箱插件不支持注册设置面板')
    await expect(instance.tryRegisterVisualizer!()).rejects.toThrow('沙箱插件不支持注册可视化')
  })
})

describe('workerSandbox - 生命周期', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('terminate 终止 Worker 并 reject 所有进行中的调用', async () => {
    const code = `
      export default (api) => ({
        hang: () => new Promise(() => {}), // 永不 resolve 的 worker 内调用
      })
    `
    const api = createMockApi()
    const { host, worker } = await setupSandbox(code, [], api)
    const instance = await runSandboxMain(host, api)

    // 发起一个会挂起的 RPC (主窗口侧不回复)
    const pending = instance.hang!()
    await flushAsync()

    host.terminate()
    expect(worker.terminated).toBe(true)
    await expect(pending).rejects.toThrow('插件沙箱已终止')
  })

  it('terminate 后再次 runMain 自动重建 Worker', async () => {
    const code = `export default () => ({ mark: () => 'ok' })`
    const api = createMockApi()
    // 工厂每次创建全新 Worker + 独立 runtime (模拟真实 Worker 环境重建)
    const workers: FakeWorker[] = []
    const host = new PluginWorkerHost('test-plugin', code, [], () => {
      const w = new FakeWorker()
      const runtime = new SandboxWorkerRuntime((msg) => w.emitToHost(msg), testLoader)
      w.runtimeListener = (ev) => {
        void runtime.handleMessage(ev.data as Parameters<typeof runtime.handleMessage>[0])
      }
      workers.push(w)
      return asWorker(w)
    })

    const first = await runSandboxMain(host, api)
    expect(await first.mark!()).toBe('ok')

    host.terminate()
    expect(workers[0]!.terminated).toBe(true)

    const second = await runSandboxMain(host, api) // 触发重建
    expect(workers.length).toBe(2)
    expect(await second.mark!()).toBe('ok')
  })

  it('getSandboxAdapter 的 cleanup 调用 terminate', async () => {
    const code = `export default () => ({})`
    const api = createMockApi()
    const { host, worker } = await setupSandbox(code, [], api)
    const adapter = host.getSandboxAdapter()

    adapter.cleanup()
    expect(worker.terminated).toBe(true)
  })
})

describe('workerSandbox - 沙箱全局对象 (第二参数 globals)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('ES 格式插件经第二参数 globals 获得受控 console 与定时器', async () => {
    const code = `
      export default (api, globals) => ({
        note: (msg) => globals.console.log(msg),
        setBoomTimer: () =>
          globals.setTimeout(() => { throw new Error('timer boom') }, 5),
      })
    `
    const worker = new FakeWorker()
    const host = new PluginWorkerHost('test-plugin', code, [], () => asWorker(worker))
    const logged: unknown[][] = []
    const runtime = new SandboxWorkerRuntime((msg) => {
      if (msg.type === 'log') logged.push(msg.args)
      worker.emitToHost(msg)
    }, testLoader)
    worker.runtimeListener = (ev) => {
      void runtime.handleMessage(ev.data as Parameters<typeof runtime.handleMessage>[0])
    }
    await host.init()

    const api = createMockApi()
    const instance = await runSandboxMain(host, api)

    await instance.note!('hello-from-plugin')
    await flushAsync()
    expect(logged.some((args) => args.includes('hello-from-plugin'))).toBe(true)

    // globals.setTimeout 被包装:回调错误会被捕获并转发日志而非静默丢失
    await instance.setBoomTimer!()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(logged.some((args) => String(args[0]).includes('定时器执行错误'))).toBe(true)
  })
})

describe('workerSandbox - 原生网络 API 移除 (removeNetworkGlobals)', () => {
  it('删除 Worker 全局中可配置的原生网络 API', () => {
    const scope: Record<string, unknown> = {
      fetch: () => 'fetch',
      XMLHttpRequest: class {},
      WebSocket: class {},
      EventSource: class {},
      WebSocketStream: class {},
      setTimeout: () => 1, // 非网络 API,必须保留
    }
    removeNetworkGlobals(scope)

    expect('fetch' in scope).toBe(false)
    expect('XMLHttpRequest' in scope).toBe(false)
    expect('WebSocket' in scope).toBe(false)
    expect('EventSource' in scope).toBe(false)
    expect('WebSocketStream' in scope).toBe(false)
    expect(typeof scope.setTimeout).toBe('function')
  })

  it('原型链上的属性以抛错 getter 遮蔽 (fetch 定义在 WorkerGlobalScope.prototype)', () => {
    // 模拟原型链定义:实例上无自身属性
    const proto: Record<string, unknown> = { fetch: () => 'native-fetch' }
    const scope = Object.create(proto) as Record<string, unknown>

    removeNetworkGlobals(scope)

    expect(() => scope.fetch).toThrow('沙箱禁止使用 fetch')
  })

  it('navigator.sendBeacon 被遮蔽', () => {
    const navigator: Record<string, unknown> = Object.create({
      sendBeacon: () => true,
    })
    const scope: Record<string, unknown> = { navigator }

    removeNetworkGlobals(scope)

    expect(() => navigator.sendBeacon).toThrow('沙箱禁止使用 sendBeacon')
  })

  it('不存在目标属性时为无操作,不抛错', () => {
    const scope: Record<string, unknown> = {}
    expect(() => removeNetworkGlobals(scope)).not.toThrow()
  })

  it('自身不可配置属性保留不抛错 (由 CSP 兜底拦截)', () => {
    const scope: Record<string, unknown> = {}
    Object.defineProperty(scope, 'fetch', {
      value: () => 'frozen',
      configurable: false,
      writable: false,
    })

    expect(() => removeNetworkGlobals(scope)).not.toThrow()
    expect(scope.fetch).toBeDefined()
  })
})
