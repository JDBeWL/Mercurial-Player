import { beforeEach, describe, expect, it, vi } from 'vitest'

const picaApi = vi.hoisted(() => ({ resize: vi.fn() }))

vi.mock('pica', () => ({ default: () => picaApi }))

interface UpscaleRequest {
  id: number
  blob: Blob
  targetSide: number
}

interface UpscaleResponse {
  id: number
  needUpscale?: boolean
  buffer?: ArrayBuffer
  error?: string
}

const fakeSelf = vi.hoisted(() => ({
  onmessage: null as ((ev: MessageEvent<UpscaleRequest>) => void) | null,
  postMessage: vi.fn(),
}))

vi.stubGlobal('self', fakeSelf)

const canvasState = vi.hoisted(() => ({
  drawImage: vi.fn(),
  nextCtxNull: false,
  blobData: new Uint8Array([137, 80, 78, 71]),
}))

// 最小 OffscreenCanvas 替身:src 用 drawImage 记录裁剪几何,dst 用 convertToBlob 产出位图
class FakeOffscreenCanvas {
  width: number
  height: number

  constructor(width: number, height: number) {
    this.width = width
    this.height = height
  }

  getContext(_type: string, _options?: Record<string, unknown>) {
    if (canvasState.nextCtxNull) return null
    return { drawImage: canvasState.drawImage }
  }

  async convertToBlob(_options: { type: string }): Promise<Blob> {
    return new Blob([canvasState.blobData], { type: 'image/png' })
  }
}

vi.stubGlobal('OffscreenCanvas', FakeOffscreenCanvas)

const createImageBitmapMock = vi.hoisted(() => vi.fn())
vi.stubGlobal('createImageBitmap', createImageBitmapMock)

await import('@/workers/coverUpscale.worker')

let bitmapClose = vi.fn()

const runRequest = async (
  request: { id?: number; targetSide: number },
  options: {
    width?: number
    height?: number
    bitmapError?: unknown
    ctxNull?: boolean
    resizeError?: unknown
  } = {},
) => {
  const { width = 200, height = 200, bitmapError, ctxNull = false, resizeError } = options
  canvasState.nextCtxNull = ctxNull
  picaApi.resize.mockReset()
  if (resizeError) {
    picaApi.resize.mockRejectedValue(resizeError)
  } else {
    picaApi.resize.mockResolvedValue(undefined)
  }

  if (bitmapError) {
    createImageBitmapMock.mockRejectedValue(bitmapError)
  } else {
    bitmapClose = vi.fn()
    createImageBitmapMock.mockResolvedValue({ width, height, close: bitmapClose })
  }

  await fakeSelf.onmessage?.({
    data: { id: request.id ?? 1, blob: new Blob(['src']), targetSide: request.targetSide },
  } as MessageEvent<UpscaleRequest>)
}

const lastResponse = (): UpscaleResponse =>
  fakeSelf.postMessage.mock.calls[fakeSelf.postMessage.mock.calls.length - 1]![0] as UpscaleResponse

const lastTransfer = (): Transferable[] | undefined =>
  fakeSelf.postMessage.mock.calls[fakeSelf.postMessage.mock.calls.length - 1]![1] as
    Transferable[] | undefined

beforeEach(() => {
  vi.clearAllMocks()
  canvasState.nextCtxNull = false
})

describe('coverUpscale.worker', () => {
  it('upscales a small cover and transfers the PNG buffer', async () => {
    await runRequest({ id: 7, targetSide: 768 }, { width: 200, height: 200 })

    const response = lastResponse()
    expect(response.id).toBe(7)
    expect(response.needUpscale).toBeUndefined()
    expect(response.error).toBeUndefined()
    expect(response.buffer).toBeInstanceOf(ArrayBuffer)
    // buffer 必须同时出现在 transfer 列表里,实现零拷贝转移
    expect(lastTransfer()).toEqual([response.buffer])
  })

  it('skips the upscale when the source is already large enough', async () => {
    await runRequest({ targetSide: 768 }, { width: 800, height: 800 })

    expect(lastResponse()).toEqual({ id: 1, needUpscale: false })
    expect(picaApi.resize).not.toHaveBeenCalled()
    expect(bitmapClose).toHaveBeenCalled()
  })

  it('respects the 98% threshold for the no-upscale boundary', async () => {
    // 768 * 0.98 = 752.64 → 753 刚好跨过阈值,无需放大
    await runRequest({ targetSide: 768 }, { width: 753, height: 753 })
    expect(lastResponse()).toEqual({ id: 1, needUpscale: false })

    // 752 未跨阈值,需要放大
    await runRequest({ id: 2, targetSide: 768 }, { width: 752, height: 752 })
    expect(lastResponse().id).toBe(2)
    expect(lastResponse().buffer).toBeInstanceOf(ArrayBuffer)
  })

  it('treats a zero-sized source as needing no upscale', async () => {
    await runRequest({ targetSide: 768 }, { width: 0, height: 300 })

    expect(lastResponse()).toEqual({ id: 1, needUpscale: false })
    expect(picaApi.resize).not.toHaveBeenCalled()
  })

  it('reports an error when the canvas has no 2d context', async () => {
    await runRequest({ targetSide: 768 }, { width: 200, height: 200, ctxNull: true })

    expect(lastResponse()).toEqual({ id: 1, error: 'no 2d context' })
    expect(bitmapClose).toHaveBeenCalled()
  })

  it('crops a wide source to the left square', async () => {
    await runRequest({ targetSide: 768 }, { width: 400, height: 200 })

    // side = 200,宽图取左上角方形 → sy = 0
    expect(canvasState.drawImage).toHaveBeenCalledWith(
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

  it('crops a tall source to a vertically centred square', async () => {
    await runRequest({ targetSide: 768 }, { width: 200, height: 400 })

    // side = 200,高图垂直居中 → sy = (400 - 200) / 2 = 100
    expect(canvasState.drawImage).toHaveBeenCalledWith(
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

  it('closes the bitmap before the expensive resize', async () => {
    await runRequest({ targetSide: 768 }, { width: 200, height: 200 })

    expect(bitmapClose).toHaveBeenCalled()
    expect(picaApi.resize).toHaveBeenCalled()
  })

  it('requests the Lanczos3 filter', async () => {
    await runRequest({ targetSide: 768 }, { width: 200, height: 200 })

    expect(picaApi.resize).toHaveBeenCalledWith(expect.anything(), expect.anything(), {
      filter: 'lanczos3',
    })
  })

  it('creates the destination canvas at the requested size', async () => {
    await runRequest({ targetSide: 640 }, { width: 200, height: 200 })

    // 目标画布尺寸即 pica.resize 的第二个参数
    const [, dst] = picaApi.resize.mock.calls[0]! as unknown as [
      unknown,
      FakeOffscreenCanvas,
      unknown,
    ]
    expect((dst as { width: number; height: number }).width).toBe(640)
    expect((dst as { height: number }).height).toBe(640)
  })

  it('reports the resize failure back to the main thread', async () => {
    await runRequest(
      { targetSide: 768 },
      { width: 200, height: 200, resizeError: new Error('pica exploded') },
    )

    expect(lastResponse()).toEqual({ id: 1, error: 'pica exploded' })
  })

  it('reports a decode failure back to the main thread', async () => {
    await runRequest({ targetSide: 768 }, { bitmapError: new Error('unsupported image') })

    expect(lastResponse()).toEqual({ id: 1, error: 'unsupported image' })
    expect(picaApi.resize).not.toHaveBeenCalled()
  })

  it('stringifies non-Error failures', async () => {
    await runRequest({ targetSide: 768 }, { bitmapError: 'plain string failure' })

    expect(lastResponse()).toEqual({ id: 1, error: 'plain string failure' })
  })

  it('keeps serving later requests after a failure', async () => {
    await runRequest({ id: 1, targetSide: 768 }, { bitmapError: new Error('boom') })
    expect(lastResponse()).toEqual({ id: 1, error: 'boom' })

    await runRequest({ id: 2, targetSide: 768 }, { width: 200, height: 200 })

    const response = lastResponse()
    expect(response.id).toBe(2)
    expect(response.buffer).toBeInstanceOf(ArrayBuffer)
  })
})
