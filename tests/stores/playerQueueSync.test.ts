// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, reactive } from 'vue'

// 平台可控:getPlatform 返回的 promise 可被用例悬住(latest-wins 用例需要控制时序)
const platformApi = vi.hoisted(() => {
  let release: (p: string) => void = () => {}
  let promise = Promise.resolve('android')
  return {
    get promise() {
      return promise
    },
    /** 直接给定已决平台(默认 android) */
    set(p: string) {
      promise = Promise.resolve(p)
    },
    /** 让下一次 getPlatform 悬住,直到 release */
    hold() {
      promise = new Promise((r) => {
        release = r
      })
    },
    release(p: string) {
      release(p)
    },
  }
})

vi.mock('@/services/appService', () => ({
  getPlatform: () => platformApi.promise,
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeApi.invoke,
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const invokeApi = vi.hoisted(() => ({
  invoke: vi.fn(async (_cmd: string, _args?: Record<string, unknown>) => undefined),
}))

interface QueueStore {
  _isDestroyed: boolean
  playlist: Array<{ path: string; title?: string }>
  currentTrack: { path: string; title?: string } | null
  repeatMode: string
  isShuffle: boolean
  _shuffleOrder: number[]
}

const track = (path: string) => ({ path, title: path })

const createStore = (): QueueStore =>
  reactive({
    _isDestroyed: false,
    playlist: [track('/a.mp3'), track('/b.mp3')],
    currentTrack: track('/a.mp3'),
    repeatMode: 'none',
    isShuffle: false,
    _shuffleOrder: [] as number[],
  })

/** queue 模块缓存了平台判定(androidPromise),每条用例重新加载拿到干净的模块 */
const loadQueueModule = async () => {
  vi.resetModules()
  return import('@/stores/player/queue')
}

const queueCalls = () => invokeApi.invoke.mock.calls.filter(([cmd]) => cmd === 'set_play_queue')

const lastQueueArgs = () => queueCalls().at(-1)?.[1] as Record<string, unknown>

const flush = async () => {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve()
    await nextTick()
  }
}

// 每条用例加载的模块:afterEach 借它停掉模块级 watch,避免泄漏到后续用例
let mod: Awaited<ReturnType<typeof loadQueueModule>> | null = null

beforeEach(() => {
  invokeApi.invoke.mockClear()
  // 复位默认平台:上一条用例(如 desktop)换掉的状态不能泄漏
  platformApi.set('android')
})

afterEach(() => {
  mod?.stopPlayQueueWatch()
  mod = null
})

describe('watchPlayQueue > snapshot sync', () => {
  it('sends the full snapshot on registration while on android', async () => {
    mod = await loadQueueModule()
    const store = createStore()
    mod.watchPlayQueue(store as never)
    await flush()

    expect(queueCalls()).toHaveLength(1)
    expect(lastQueueArgs()).toMatchObject({
      currentIndex: 0,
      repeatMode: 'none',
      autoAdvance: true,
    })
    expect((lastQueueArgs().tracks as unknown[])).toHaveLength(2)
  })

  it('does not resend the queue when only the current track changes', async () => {
    mod = await loadQueueModule()
    const store = createStore()
    mod.watchPlayQueue(store as never)
    await flush()
    expect(queueCalls()).toHaveLength(1)

    // 切歌的下标由 play_track 命令在 Rust 侧同步,这里不得重发整份队列
    store.currentTrack = track('/b.mp3')
    await flush()

    expect(queueCalls()).toHaveLength(1)
  })

  it('skips the snapshot entirely on desktop', async () => {
    platformApi.set('windows')
    mod = await loadQueueModule()
    const store = createStore()
    mod.watchPlayQueue(store as never)
    await flush()

    store.playlist.push(track('/c.mp3'))
    await flush()

    // 桌面端 Rust 队列无消费者,一次都不发
    expect(queueCalls()).toHaveLength(0)
  })

  it('resends when the playlist or shuffle order changes', async () => {
    mod = await loadQueueModule()
    const store = createStore()
    mod.watchPlayQueue(store as never)
    await flush()
    expect(queueCalls()).toHaveLength(1)

    store.playlist.push(track('/c.mp3'))
    await flush()
    expect(queueCalls()).toHaveLength(2)

    // 重新随机只换 _shuffleOrder(isShuffle/列表都不动),快照顺序已变,必须重发
    store.isShuffle = true
    store._shuffleOrder = [2, 0, 1]
    await flush()
    expect(queueCalls()).toHaveLength(3)
    const paths = (lastQueueArgs().tracks as Array<{ path: string }>).map((t) => t.path)
    expect(paths).toEqual(['/c.mp3', '/a.mp3', '/b.mp3'])
  })

  it('drops the stale sync when a newer one is pending (latest-wins)', async () => {
    // 判平台悬住:第一次同步卡在 await,期间队列又变了
    platformApi.hold()
    mod = await loadQueueModule()
    const store = createStore()
    mod.watchPlayQueue(store as never)
    store.playlist.push(track('/c.mp3'))
    await flush()
    expect(queueCalls()).toHaveLength(0)

    platformApi.release('android')
    await flush()

    // 旧请求放弃,只有最新那份快照发出(3 首)
    expect(queueCalls()).toHaveLength(1)
    expect(lastQueueArgs().tracks as unknown[]).toHaveLength(3)
  })
})
