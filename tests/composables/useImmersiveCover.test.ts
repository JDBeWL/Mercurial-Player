// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'

const errorApi = vi.hoisted(() => ({ handle: vi.fn() }))
const picaApi = vi.hoisted(() => ({
  resize: vi.fn(),
  toBlob: vi.fn(),
}))
const workerCtor = vi.hoisted(() => ({
  failOnCreate: false,
  instances: [] as FakeWorker[],
}))

vi.mock('@/utils/errorHandler', () => ({
  default: { handle: errorApi.handle },
  ErrorSeverity: { LOW: 'low', MEDIUM: 'medium', HIGH: 'high' },
}))

vi.mock('pica', () => ({
  default: () => picaApi,
}))

/** 假的放大 worker:测试自行决定何时回包、回什么包 */
class FakeWorker {
  onmessage: ((ev: MessageEvent) => void) | null = null
  onerror: (() => void) | null = null
  terminated = false
  posted: { id: number; blob: Blob; targetSide: number }[] = []

  constructor() {
    if (workerCtor.failOnCreate) throw new Error('worker unavailable')
    workerCtor.instances.push(this)
  }

  postMessage(message: { id: number; blob: Blob; targetSide: number }) {
    this.posted.push(message)
  }

  terminate() {
    this.terminated = true
  }

  get lastJobId(): number {
    return this.posted[this.posted.length - 1]!.id
  }

  respond(payload: { needUpscale?: boolean; buffer?: ArrayBuffer; error?: string }) {
    this.respondAs(this.lastJobId, payload)
  }

  /** 按指定 job id 回包:用于模拟"旧任务姗姗来迟" */
  respondAs(id: number, payload: { needUpscale?: boolean; buffer?: ArrayBuffer; error?: string }) {
    this.onmessage?.({ data: { id, ...payload } } as MessageEvent)
  }
}

vi.mock('@/workers/coverUpscale.worker?worker', () => ({
  default: FakeWorker,
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { mockReadFile, mockConvertFileSrc } = await import('../mocks/tauri')

const WORKER_TIMEOUT_MS = 10_000

const fakeCtx = { drawImage: vi.fn() }

class FakeURL extends URL {
  static createObjectURL = vi.fn(() => 'blob:upscaled')
  static revokeObjectURL = vi.fn()
}

// 在任何 spy 之前捕获原始实现:否则再次取 document.createElement 会拿到被 mock 的版本
const originalCreateElement = document.createElement.bind(document)
let bitmapSize = { width: 200, height: 200 }
let bitmapClose: ReturnType<typeof vi.fn>
const mounted: ReturnType<typeof mount>[] = []

/**
 * 该 composable 持有模块级 worker 通道单例(workerBroken / pendingJobs),
 * 每条用例都要拿到全新的模块实例,否则一个用例弄坏通道会污染后续用例
 */
const loadComposable = async () => {
  vi.resetModules()
  const mod = await import('@/composables/useImmersiveCover')
  return mod.useImmersiveCover
}

const mountCover = async (
  useImmersiveCover: Awaited<ReturnType<typeof loadComposable>>,
  initialPath: string | null | undefined,
  initialEnabled: boolean,
) => {
  const coverPath = ref<string | null | undefined>(initialPath)
  const enabled = ref(initialEnabled)
  const wrapper = mount(
    defineComponent({
      setup() {
        const { coverDisplayUrl } = useImmersiveCover(coverPath, enabled)
        return { coverDisplayUrl }
      },
      template: '<div>{{ coverDisplayUrl }}</div>',
    }),
  )
  mounted.push(wrapper)
  await flushPromises()
  return { coverPath, enabled, url: () => wrapper.vm.coverDisplayUrl as string, wrapper }
}

const original = (path: string) => mockConvertFileSrc(path)

beforeEach(() => {
  vi.clearAllMocks()
  workerCtor.failOnCreate = false
  workerCtor.instances = []
  bitmapSize = { width: 200, height: 200 }
  bitmapClose = vi.fn()

  vi.spyOn(document, 'createElement').mockImplementation(((tag: string) => {
    if (tag !== 'canvas') return originalCreateElement(tag)
    return {
      width: 0,
      height: 0,
      getContext: () => fakeCtx,
    } as unknown as HTMLElement
  }) as typeof document.createElement)

  vi.stubGlobal('createImageBitmap', async () => ({
    ...bitmapSize,
    close: bitmapClose,
  }))
  vi.stubGlobal('URL', FakeURL)
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ blob: async () => new Blob(['remote']) })),
  )

  mockReadFile.mockResolvedValue(new Uint8Array([1, 2, 3]))
  mockConvertFileSrc.mockImplementation((path: string) => `asset://localhost/${encodeURI(path)}`)
  picaApi.resize.mockResolvedValue(undefined)
  picaApi.toBlob.mockResolvedValue(new Blob(['upscaled']))
})

afterEach(() => {
  while (mounted.length > 0) mounted.pop()?.unmount()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('useImmersiveCover > basic behaviour', () => {
  it('shows nothing when there is no cover', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, null, true)
    expect(cover.url()).toBe('')
    expect(mockReadFile).not.toHaveBeenCalled()
  })

  it('clears the url when the cover is removed', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    expect(cover.url()).not.toBe('')

    cover.coverPath.value = null
    await flushPromises()
    expect(cover.url()).toBe('')
  })

  it('shows the raw asset url immediately', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    expect(cover.url()).toBe(original('/a.jpg'))
  })

  it('does not upscale while immersive mode is disabled', async () => {
    const useImmersiveCover = await loadComposable()
    await mountCover(useImmersiveCover, '/a.jpg', false)

    expect(FakeURL.createObjectURL).not.toHaveBeenCalled()
    expect(workerCtor.instances).toHaveLength(0)
  })

  it('starts upscaling when immersive mode is switched on', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', false)

    cover.enabled.value = true
    await flushPromises()

    expect(workerCtor.instances).toHaveLength(1)
  })

  it('targets the window height times the device pixel ratio', async () => {
    const useImmersiveCover = await loadComposable()
    await mountCover(useImmersiveCover, '/a.jpg', true)

    expect(workerCtor.instances[0]!.posted[0]!.targetSide).toBe(
      Math.round(window.innerHeight * (window.devicePixelRatio || 1)),
    )
  })
})

describe('useImmersiveCover > worker channel', () => {
  it('replaces the url with the upscaled bitmap', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    const worker = workerCtor.instances[0]!

    expect(cover.url()).toBe(original('/a.jpg'))

    worker.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()

    expect(FakeURL.createObjectURL).toHaveBeenCalled()
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('keeps the raw url when the worker reports no upscale needed', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)

    workerCtor.instances[0]!.respond({ needUpscale: true })
    await flushPromises()

    expect(cover.url()).toBe(original('/a.jpg'))
    expect(FakeURL.createObjectURL).not.toHaveBeenCalled()
  })

  it('revokes the previous object url before installing a new one', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()

    FakeURL.revokeObjectURL.mockClear()
    cover.coverPath.value = '/b.jpg'
    await flushPromises()
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()

    expect(FakeURL.revokeObjectURL).toHaveBeenCalledWith('blob:upscaled')
  })

  it('falls back to the main thread when the worker reports a structural error', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    const worker = workerCtor.instances[0]!

    worker.respond({ error: 'OffscreenCanvas unsupported' })
    await flushPromises()

    // 通道被停用并终止
    expect(worker.terminated).toBe(true)
    expect(errorApi.handle).toHaveBeenCalled()
    // 回退路径在主线程完成放大
    expect(picaApi.resize).toHaveBeenCalled()
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('permanently disables the worker channel after a load failure', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    const worker = workerCtor.instances[0]!

    worker.onerror?.()
    await flushPromises()

    expect(worker.terminated).toBe(true)
    expect(errorApi.handle).toHaveBeenCalled()

    // 后续请求不再创建 worker
    cover.coverPath.value = '/b.jpg'
    await flushPromises()
    expect(workerCtor.instances).toHaveLength(1)
    expect(picaApi.resize).toHaveBeenCalled()
  })

  it('fails all in-flight jobs when the worker fails to load', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    const worker = workerCtor.instances[0]!

    // 在回包前触发加载失败
    worker.onerror?.()
    await flushPromises()

    expect(errorApi.handle).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'upscale worker failed to load' }),
      expect.anything(),
    )
    // 回退路径仍然给出可用结果
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('falls back when the worker never answers', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)

    await vi.advanceTimersByTimeAsync(WORKER_TIMEOUT_MS)

    expect(errorApi.handle).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'upscale worker timeout' }),
      expect.anything(),
    )
    expect(picaApi.resize).toHaveBeenCalled()
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('falls back to the main thread when the worker cannot be constructed', async () => {
    workerCtor.failOnCreate = true
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)

    expect(errorApi.handle).toHaveBeenCalled()
    expect(picaApi.resize).toHaveBeenCalled()
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('rejects a response that carries neither a buffer nor a flag', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)

    workerCtor.instances[0]!.respond({})
    await flushPromises()

    expect(errorApi.handle).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'worker response missing data' }),
      expect.anything(),
    )
    expect(cover.url()).toBe('blob:upscaled')
  })

  it('ignores a response for an unknown job id', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)

    workerCtor.instances[0]!.onmessage?.({
      data: { id: 999, buffer: new ArrayBuffer(8) },
    } as MessageEvent)
    await flushPromises()

    // 未知 id 不应触发回退,也不应改变展示 URL
    expect(errorApi.handle).not.toHaveBeenCalled()
    expect(cover.url()).toBe(original('/a.jpg'))
  })
})

describe('useImmersiveCover > main thread fallback', () => {
  const mountWithoutWorker = async (path = '/a.jpg') => {
    workerCtor.failOnCreate = true
    const useImmersiveCover = await loadComposable()
    return mountCover(useImmersiveCover, path, true)
  }

  it('closes the decoded bitmap after resizing', async () => {
    await mountWithoutWorker()
    expect(bitmapClose).toHaveBeenCalled()
  })

  it('skips the resize when the source is already large enough', async () => {
    // 768 * 0.98 ≈ 753,800px 的源图无需放大
    bitmapSize = { width: 800, height: 800 }
    const cover = await mountWithoutWorker()

    expect(picaApi.resize).not.toHaveBeenCalled()
    expect(cover.url()).toBe(original('/a.jpg'))
  })

  it('skips the resize when the canvas has no 2d context', async () => {
    vi.spyOn(document, 'createElement').mockImplementation(((tag: string) => {
      if (tag !== 'canvas') return originalCreateElement(tag)
      return { width: 0, height: 0, getContext: () => null } as unknown as HTMLElement
    }) as typeof document.createElement)

    const cover = await mountWithoutWorker()

    expect(picaApi.resize).not.toHaveBeenCalled()
    expect(cover.url()).toBe(original('/a.jpg'))
  })

  it('crops a tall source to a centred square', async () => {
    bitmapSize = { width: 200, height: 400 }
    await mountWithoutWorker()

    // side = min(200, 400) = 200,源区域垂直居中 → sy = (400 - 200) / 2 = 100
    expect(fakeCtx.drawImage).toHaveBeenCalledWith(
      expect.anything(),
      0,
      100,
      200,
      200,
      0,
      0,
      200,
      200,
    )
  })

  it('crops a wide source to the left square', async () => {
    bitmapSize = { width: 400, height: 200 }
    await mountWithoutWorker()

    // side = 200,宽图取左上角方形 → sy = 0
    expect(fakeCtx.drawImage).toHaveBeenCalledWith(
      expect.anything(),
      0,
      0,
      200,
      200,
      0,
      0,
      200,
      200,
    )
  })

  it('reads remote covers through fetch', async () => {
    const fetchMock = vi.mocked(globalThis.fetch)
    await mountWithoutWorker('https://cdn.test/cover.jpg')

    expect(fetchMock).toHaveBeenCalledWith('https://cdn.test/cover.jpg')
    expect(mockReadFile).not.toHaveBeenCalled()
  })

  it('keeps the raw url when the main thread path also fails', async () => {
    picaApi.resize.mockRejectedValue(new Error('pica exploded'))
    const cover = await mountWithoutWorker()

    expect(errorApi.handle).toHaveBeenCalled()
    expect(cover.url()).toBe(original('/a.jpg'))
  })
})

describe('useImmersiveCover > lifecycle', () => {
  it('discards an upscale result for a cover that has already changed', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    const worker = workerCtor.instances[0]!
    const staleJobId = worker.lastJobId

    cover.coverPath.value = '/b.jpg'
    await flushPromises()
    expect(cover.url()).toBe(original('/b.jpg'))

    // 旧曲目(first job)姗姗来迟的结果必须被丢弃
    worker.respondAs(staleJobId, { buffer: new ArrayBuffer(16) })
    await flushPromises()

    expect(cover.url()).toBe(original('/b.jpg'))
  })

  it('revokes the object url on unmount', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()

    FakeURL.revokeObjectURL.mockClear()
    cover.wrapper.unmount()
    await flushPromises()

    expect(FakeURL.revokeObjectURL).toHaveBeenCalledWith('blob:upscaled')
  })

  it('still cleans up when nothing was ever upscaled', async () => {
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, null, true)

    expect(() => cover.wrapper.unmount()).not.toThrow()
    await flushPromises()
  })

  it('re-upscales after a resize that changes the target side enough', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()
    picaApi.resize.mockClear()
    const postsBefore = workerCtor.instances[0]!.posted.length

    Object.defineProperty(window, 'innerHeight', { value: 1600, configurable: true })
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(300)

    // 目标边长从 768 变到 1600,差值远大于 32 → 重新放大
    expect(workerCtor.instances[0]!.posted.length).toBeGreaterThan(postsBefore)

    Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
    expect(cover.url()).toBeDefined()
  })

  it('ignores a resize that barely changes the target side', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()
    picaApi.resize.mockClear()
    const postsBefore = workerCtor.instances[0]!.posted.length

    // 只变 8px,不足 32px 阈值
    Object.defineProperty(window, 'innerHeight', {
      value: window.innerHeight + 8,
      configurable: true,
    })
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(300)

    expect(workerCtor.instances[0]!.posted.length).toBe(postsBefore)
    expect(picaApi.resize).not.toHaveBeenCalled()
  })

  it('coalesces a burst of resize events into a single re-run', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()
    picaApi.resize.mockClear()

    Object.defineProperty(window, 'innerHeight', { value: 1600, configurable: true })
    const postsBefore = workerCtor.instances[0]!.posted.length
    for (let i = 0; i < 5; i++) window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(300)

    expect(workerCtor.instances[0]!.posted.length).toBe(postsBefore + 1)
    Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
  })

  it('does not re-upscale on resize while immersive mode is off', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', false)

    Object.defineProperty(window, 'innerHeight', { value: 1600, configurable: true })
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(300)

    expect(picaApi.resize).not.toHaveBeenCalled()
    expect(cover.url()).toBe(original('/a.jpg'))
    Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
  })

  it('stops listening for resizes after unmount', async () => {
    vi.useFakeTimers()
    const useImmersiveCover = await loadComposable()
    const cover = await mountCover(useImmersiveCover, '/a.jpg', true)
    workerCtor.instances[0]!.respond({ buffer: new ArrayBuffer(16) })
    await flushPromises()
    picaApi.resize.mockClear()
    cover.wrapper.unmount()

    Object.defineProperty(window, 'innerHeight', { value: 1600, configurable: true })
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(300)

    expect(picaApi.resize).not.toHaveBeenCalled()
    Object.defineProperty(window, 'innerHeight', { value: 768, configurable: true })
  })
})
