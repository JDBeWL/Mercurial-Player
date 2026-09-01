// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'

type RGBA = [number, number, number, number]

const errorApi = vi.hoisted(() => ({
  handle: vi.fn(),
}))

vi.mock('@/utils/errorHandler', () => ({
  default: { handle: errorApi.handle },
  ErrorSeverity: { LOW: 'low', MEDIUM: 'medium', HIGH: 'high' },
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { mockReadFile } = await import('../mocks/tauri')
const { useDominantColor } = await import('@/composables/useDominantColor')

const SIZE = 32

/** 构造 32x32 的 RGBA 像素缓冲,填充函数按坐标决定每个像素 */
const makePixels = (fill: (x: number, y: number) => RGBA): Uint8ClampedArray => {
  const data = new Uint8ClampedArray(SIZE * SIZE * 4)
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      const i = (y * SIZE + x) * 4
      const [r, g, b, a] = fill(x, y)
      data[i] = r
      data[i + 1] = g
      data[i + 2] = b
      data[i + 3] = a
    }
  }
  return data
}

const solid = (color: RGBA): Uint8ClampedArray => makePixels(() => color)

const fakeCtx = {
  drawImage: vi.fn(),
  getImageData: vi.fn(),
}

const fakeCanvas = {
  width: 0,
  height: 0,
  getContext: vi.fn(),
}

const realCreateElement = document.createElement.bind(document)
let pixels: Uint8ClampedArray = solid([255, 0, 0, 255])
let bitmapClose: ReturnType<typeof vi.fn>

class FakeURL extends URL {
  static createObjectURL = vi.fn(() => 'blob:cover')
  static revokeObjectURL = vi.fn()
}

/** 源码在 typeof createImageBitmap !== 'function' 时走 <img> 降级路径 */
const installLegacyImageDecoder = () => {
  class FakeImage {
    onload: (() => void) | null = null
    onerror: (() => void) | null = null
    naturalWidth = SIZE
    naturalHeight = SIZE
    set src(_value: string) {
      setTimeout(() => this.onload?.(), 0)
    }
  }
  vi.stubGlobal('Image', FakeImage)
  vi.stubGlobal('createImageBitmap', undefined)
}

const setPixels = (next: Uint8ClampedArray) => {
  pixels = next
  fakeCtx.getImageData.mockImplementation(() => ({ data: pixels }))
}

/** 取色链路是异步的(decode → pick),冲刷微任务与定时器队列 */
const settle = async () => {
  await nextTick()
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
}

const mount = (path: string | null | undefined, mode: 'album' | 'fusion' = 'album') => {
  const coverPath = ref<string | null | undefined>(path)
  const scheme = ref<'album' | 'fusion'>(mode)
  const api = useDominantColor(coverPath, scheme)
  return { coverPath, scheme, ...api }
}

const parseRgb = (value: string): [number, number, number] => {
  const parts = value.match(/\d+/g)
  expect(parts).not.toBeNull()
  return parts!.slice(0, 3).map(Number) as [number, number, number]
}

beforeEach(() => {
  vi.clearAllMocks()
  bitmapClose = vi.fn()
  pixels = solid([255, 0, 0, 255])

  fakeCtx.getImageData.mockImplementation(() => ({ data: pixels }))
  fakeCtx.drawImage.mockReturnValue(undefined)
  fakeCanvas.getContext.mockReturnValue(fakeCtx)

  vi.spyOn(document, 'createElement').mockImplementation(((tag: string) =>
    tag === 'canvas'
      ? (fakeCanvas as unknown as HTMLElement)
      : realCreateElement(tag)) as typeof document.createElement)

  vi.stubGlobal('createImageBitmap', async () => ({
    width: SIZE,
    height: SIZE,
    close: bitmapClose,
  }))
  vi.stubGlobal('URL', FakeURL)

  mockReadFile.mockResolvedValue(new Uint8Array([1, 2, 3]))
  // K-Means++ 用 Math.random 选种子,固定取值保证结果可复现
  vi.spyOn(Math, 'random').mockReturnValue(0.5)
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ blob: async () => new Blob(['x']) })),
  )
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('useDominantColor > empty input', () => {
  it('reports no color when the cover path is missing', async () => {
    const api = mount(undefined)
    await settle()
    expect(api.dominantColor.value).toBe('')
    expect(api.dominantLuminance.value).toBeNull()
  })

  it('clears a previously resolved color when the path becomes empty', async () => {
    const api = mount('/cover.jpg')
    await settle()
    expect(api.dominantColor.value).not.toBe('')

    api.coverPath.value = null
    await settle()
    expect(api.dominantColor.value).toBe('')
    expect(api.dominantLuminance.value).toBeNull()
  })

  it('never reads a file for a missing path', async () => {
    mount(null)
    await settle()
    expect(mockReadFile).not.toHaveBeenCalled()
  })
})

describe('useDominantColor > decoding', () => {
  it('reads local files through the fs plugin', async () => {
    mount('/covers/a.jpg')
    await settle()
    expect(mockReadFile).toHaveBeenCalledWith('/covers/a.jpg')
  })

  it('derives the mime type from the extension', async () => {
    const seen: string[] = []
    vi.stubGlobal('createImageBitmap', async (blob: Blob) => {
      seen.push(blob.type)
      return { width: SIZE, height: SIZE, close: vi.fn() }
    })

    mount('/covers/a.png')
    await settle()
    mount('/covers/a.webp')
    await settle()
    // 未知扩展名一律按 jpeg 处理
    mount('/covers/a.bmp')
    await settle()

    expect(seen).toEqual(['image/png', 'image/webp', 'image/jpeg'])
  })

  it('fetches remote and inline covers instead of reading from disk', async () => {
    const fetchMock = vi.mocked(globalThis.fetch)
    for (const path of [
      'https://cdn.test/c.jpg',
      'http://cdn.test/c.jpg',
      'data:image/png;base64,AA',
      'asset://localhost/cover.jpg',
    ]) {
      fetchMock.mockClear()
      mockReadFile.mockClear()
      mount(path)
      await settle()
      expect(fetchMock).toHaveBeenCalledWith(path)
      expect(mockReadFile).not.toHaveBeenCalled()
    }
  })

  it('falls back to an <img> element when createImageBitmap is unavailable', async () => {
    installLegacyImageDecoder()
    setPixels(solid([0, 128, 255, 255]))

    const api = mount('/covers/a.jpg')
    await settle()

    expect(api.dominantColor.value).toMatch(/^rgb\(\d{1,3}, \d{1,3}, \d{1,3}\)$/)
    expect(FakeURL.createObjectURL).toHaveBeenCalled()
    expect(FakeURL.revokeObjectURL).toHaveBeenCalledWith('blob:cover')
  })

  it('rejects when the <img> fallback fails to decode', async () => {
    installLegacyImageDecoder()
    class BrokenImage {
      onload: (() => void) | null = null
      onerror: (() => void) | null = null
      naturalWidth = 0
      naturalHeight = 0
      set src(_value: string) {
        setTimeout(() => this.onerror?.(), 0)
      }
    }
    vi.stubGlobal('Image', BrokenImage)

    const api = mount('/covers/a.jpg')
    await settle()

    expect(errorApi.handle).toHaveBeenCalled()
    expect(api.dominantColor.value).toBe('')
    // 失败也要回收 object URL,否则 blob 一直占着内存
    expect(FakeURL.revokeObjectURL).toHaveBeenCalledWith('blob:cover')
  })

  it('releases the bitmap after drawing', async () => {
    mount('/covers/a.jpg')
    await settle()
    expect(bitmapClose).toHaveBeenCalled()
  })

  it('leaves the color empty when the canvas has no 2d context', async () => {
    fakeCanvas.getContext.mockReturnValue(null)
    const api = mount('/covers/a.jpg')
    await settle()
    expect(api.dominantColor.value).toBe('')
    expect(api.dominantLuminance.value).toBeNull()
  })

  it('reports the failure through the error handler and clears the color', async () => {
    mockReadFile.mockRejectedValue(new Error('file missing'))
    const api = mount('/covers/a.jpg')
    await settle()

    expect(errorApi.handle).toHaveBeenCalledWith(expect.any(Error), {
      severity: 'low',
      showToUser: false,
    })
    expect(api.dominantColor.value).toBe('')
    expect(api.dominantLuminance.value).toBeNull()
  })

  it('sizes the sampling canvas to 32x32', async () => {
    mount('/covers/a.jpg')
    await settle()
    expect(fakeCanvas.width).toBe(32)
    expect(fakeCanvas.height).toBe(32)
  })

  it('crops a tall source to cover the canvas, centred vertically', async () => {
    vi.stubGlobal('createImageBitmap', async () => ({
      width: 32,
      height: 64,
      close: vi.fn(),
    }))

    mount('/covers/tall.jpg')
    await settle()

    // scale = max(32/32, 32/64) = 1 → 高 64,垂直居中后顶部偏移 (32-64)/2 = -16
    expect(fakeCtx.drawImage).toHaveBeenCalledWith(expect.anything(), 0, -16, 32, 64)
  })

  it('crops a wide source to cover the canvas, aligned to the left', async () => {
    vi.stubGlobal('createImageBitmap', async () => ({
      width: 128,
      height: 32,
      close: vi.fn(),
    }))

    mount('/covers/wide.jpg')
    await settle()

    // scale = max(32/128, 32/32) = 1 → 宽 128,左对齐且高度正好铺满
    expect(fakeCtx.drawImage).toHaveBeenCalledWith(expect.anything(), 0, 0, 128, 32)
  })
})

describe('useDominantColor > fallback colors', () => {
  it('returns the dark grey fallback for a fully transparent image', async () => {
    setPixels(solid([255, 0, 0, 0]))
    const api = mount('/covers/a.png')
    await settle()

    expect(api.dominantColor.value).toBe('rgb(40, 40, 40)')
    expect(api.dominantLuminance.value).toBeCloseTo(0.19)
  })

  it('ignores pixels below the alpha threshold', async () => {
    // alpha 127 < 128 → 全部像素被跳过,等效于空图
    setPixels(makePixels(() => [10, 200, 30, 127]))
    const api = mount('/covers/a.png')
    await settle()

    expect(api.dominantColor.value).toBe('rgb(40, 40, 40)')
  })

  it('uses the averaged color when only a few pixels qualify', async () => {
    // 仅第 0 行前 10 个像素不透明 → 命中 "pts.length < 20" 分支
    setPixels(makePixels((x, y) => (y === 0 && x < 10 ? [200, 40, 40, 255] : [0, 0, 0, 0])))
    const api = mount('/covers/a.png')
    await settle()

    const [r, g, b] = parseRgb(api.dominantColor.value)
    // 平均色为红色调,但彩度被减半,不应是纯红
    expect(r).toBeGreaterThan(g)
    expect(r).toBeGreaterThan(b)
    expect(api.dominantLuminance.value).toBeGreaterThan(0)
  })
})

describe('useDominantColor > album mode', () => {
  it('produces an opaque rgb color for a uniform cover', async () => {
    setPixels(solid([255, 0, 0, 255]))
    const api = mount('/covers/a.jpg')
    await settle()

    expect(api.dominantColor.value).toMatch(/^rgb\(\d{1,3}, \d{1,3}, \d{1,3}\)$/)
    expect(api.dominantLuminance.value).toBeGreaterThan(0)
    expect(api.dominantLuminance.value).toBeLessThanOrEqual(0.95)
  })

  it('compresses the luminance of a near-white cover', async () => {
    setPixels(solid([255, 255, 255, 255]))
    const api = mount('/covers/a.jpg')
    await settle()

    expect(api.dominantLuminance.value).toBeLessThanOrEqual(0.95)
    expect(api.dominantLuminance.value).toBeGreaterThan(0.9)
  })

  it('keeps a neutral cover neutral', async () => {
    setPixels(solid([128, 128, 128, 255]))
    const api = mount('/covers/a.jpg')
    await settle()

    const [r, g, b] = parseRgb(api.dominantColor.value)
    // 彩度低于阈值会被抑制,输出应接近中性灰
    expect(Math.abs(r - g)).toBeLessThanOrEqual(2)
    expect(Math.abs(g - b)).toBeLessThanOrEqual(2)
  })

  it('keeps a very dark cover dark', async () => {
    setPixels(solid([5, 5, 5, 255]))
    const api = mount('/covers/a.jpg')
    await settle()

    expect(api.dominantLuminance.value).toBeLessThan(0.2)
  })

  it('picks a chromatic cluster from a two-color cover', async () => {
    // 左半深蓝、右半亮黄
    setPixels(makePixels((x) => (x < SIZE / 2 ? [10, 20, 120, 255] : [240, 210, 40, 255])))
    const api = mount('/covers/a.jpg')
    await settle()

    const [r, g, b] = parseRgb(api.dominantColor.value)
    expect(r + g + b).toBeGreaterThan(0)
    expect(api.dominantLuminance.value).toBeGreaterThan(0)
    expect(api.dominantLuminance.value).toBeLessThanOrEqual(0.95)
  })

  it('stays within the sRGB gamut for highly saturated input', async () => {
    for (const color of [
      [255, 0, 255, 255],
      [0, 255, 255, 255],
      [255, 255, 0, 255],
    ] as RGBA[]) {
      setPixels(solid(color))
      const api = mount('/covers/a.jpg')
      await settle()

      for (const c of parseRgb(api.dominantColor.value)) {
        expect(c).toBeGreaterThanOrEqual(0)
        expect(c).toBeLessThanOrEqual(255)
      }
    }
  })

  it('converges to a stable color for a pure single-color cover', async () => {
    setPixels(solid([0, 120, 215, 255]))
    const first = mount('/covers/a.jpg')
    await settle()

    const second = mount('/covers/a.jpg')
    await settle()

    expect(second.dominantColor.value).toBe(first.dominantColor.value)
  })
})

describe('useDominantColor > fusion mode', () => {
  it('samples only the rightmost strip of the cover', async () => {
    // 整张蓝,只有最右两列是橙色
    setPixels(makePixels((x) => (x >= SIZE - 2 ? [255, 140, 0, 255] : [0, 0, 200, 255])))
    const api = mount('/covers/a.jpg', 'fusion')
    await settle()

    const [r, g, b] = parseRgb(api.dominantColor.value)
    expect(r).toBeGreaterThan(b)
    expect(g).toBeGreaterThan(b)
  })

  it('falls back to the dark grey when the right strip is transparent', async () => {
    setPixels(makePixels((x) => (x >= SIZE - 2 ? [255, 140, 0, 0] : [0, 0, 200, 255])))
    const api = mount('/covers/a.jpg', 'fusion')
    await settle()

    expect(api.dominantColor.value).toBe('rgb(40, 40, 40)')
  })

  it('averages the strip rather than picking a single cluster', async () => {
    // 右两列一红一蓝:平均色应同时含红蓝分量,而不是二选一
    setPixels(
      makePixels((x) => {
        if (x < SIZE - 2) return [255, 255, 255, 255]
        return x % 2 === 0 ? [255, 0, 0, 255] : [0, 0, 255, 255]
      }),
    )
    const api = mount('/covers/a.jpg', 'fusion')
    await settle()

    const [r, , b] = parseRgb(api.dominantColor.value)
    expect(r).toBeGreaterThan(0)
    expect(b).toBeGreaterThan(0)
  })
})

describe('useDominantColor > concurrency', () => {
  it('ignores a slow response for a cover that has already changed', async () => {
    let releaseFirst: (value: Uint8ClampedArray) => void = () => {}
    const pending = new Promise<Uint8ClampedArray>((resolve) => {
      releaseFirst = resolve
    })

    let call = 0
    vi.stubGlobal('createImageBitmap', async () => {
      call += 1
      if (call === 1) {
        // 首次解码挂起,模拟慢封面
        return { ...(await pending), width: SIZE, height: SIZE, close: vi.fn() }
      }
      return { width: SIZE, height: SIZE, close: vi.fn() }
    })

    const api = mount('/covers/first.jpg')
    await nextTick()

    // 切到第二张:generation 递增,第一次的解码结果应被丢弃
    setPixels(solid([0, 255, 0, 255]))
    api.coverPath.value = '/covers/second.jpg'
    await settle()
    const afterSwitch = api.dominantColor.value
    expect(afterSwitch).not.toBe('')

    // 让第一次解码带着旧像素姗姗来迟
    setPixels(solid([255, 0, 0, 255]))
    releaseFirst(solid([255, 0, 0, 255]))
    await settle()

    expect(api.dominantColor.value).toBe(afterSwitch)
  })

  it('re-runs the extraction when the scheme changes', async () => {
    setPixels(makePixels((x) => (x >= SIZE - 2 ? [255, 140, 0, 255] : [0, 0, 200, 255])))
    const api = mount('/covers/a.jpg', 'album')
    await settle()
    const albumColor = api.dominantColor.value

    api.scheme.value = 'fusion'
    await settle()

    expect(api.dominantColor.value).not.toBe(albumColor)
  })
})
