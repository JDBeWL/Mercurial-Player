// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { readFile, writeFile } from '@tauri-apps/plugin-fs'
import { PluginPermission, type PluginPermissionType } from '@/plugins/pluginTypes'

const stores = vi.hoisted(() => ({
  player: {} as Record<string, unknown>,
  musicLibrary: {} as Record<string, unknown>,
  theme: {} as Record<string, unknown>,
}))

vi.mock('@/utils/logger', () => ({
  default: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}))

vi.mock('@/utils/fileUtils', () => ({ default: { findLyricsFile: vi.fn() } }))
vi.mock('@/utils/lyricsParser', () => ({ findLyricIndex: vi.fn(() => 0) }))
vi.mock('@/stores/player', () => ({ usePlayerStore: () => stores.player }))
vi.mock('@/stores/musicLibrary', () => ({ useMusicLibraryStore: () => stores.musicLibrary }))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => stores.theme }))

const { createPluginAPI } = await import('@/plugins/pluginAPI')
const logger = (await import('@/utils/logger')).default

const ALL_PERMISSIONS = Object.values(PluginPermission) as PluginPermissionType[]

function createMockManager() {
  const storage: Record<string, unknown> = {}
  return {
    extensions: {
      settingsPanels: [],
      menuItems: [],
      playerDecorators: [],
      actionButtons: [],
      lyricsProviders: [],
      visualizers: [],
      commands: [],
      shortcuts: [],
    },
    registerExtension: vi.fn(),
    getExtensions: vi.fn(() => []),
    getStorage: vi.fn(() => storage),
    on: vi.fn(),
    off: vi.fn(),
    emit: vi.fn(),
  }
}

const api = (permissions: PluginPermissionType[] = ALL_PERMISSIONS) =>
  createPluginAPI('io-plugin', permissions, createMockManager() as never)

/**
 * 构造一个通过 `instanceof HTMLCanvasElement` 检查的替身:
 * happy-dom 未实现 Canvas 2D,真实元素的 toBlob/toDataURL 不可用。
 */
function fakeCanvas(dataUrl = 'data:image/png;base64,AAAA') {
  const canvas = Object.create(HTMLCanvasElement.prototype) as HTMLCanvasElement & {
    toBlob: ReturnType<typeof vi.fn>
    toDataURL: ReturnType<typeof vi.fn>
  }
  canvas.toBlob = vi.fn((cb: BlobCallback) => cb(new Blob(['png'], { type: 'image/png' })))
  canvas.toDataURL = vi.fn(() => dataUrl)
  return canvas
}

/** 替换全局 Image,使其加载成功或失败 */
function stubImage(shouldFail = false, naturalWidth = 8, naturalHeight = 4) {
  class FakeImage {
    crossOrigin = ''
    onload: (() => void) | null = null
    onerror: ((e: unknown) => void) | null = null
    naturalWidth = naturalWidth
    naturalHeight = naturalHeight
    set src(_value: string) {
      setTimeout(() => {
        if (shouldFail) this.onerror?.(new ErrorEvent('error', { message: 'decode failed' }))
        else this.onload?.()
      }, 0)
    }
  }
  vi.stubGlobal('Image', FakeImage)
}

const clipboardWrite = vi.fn().mockResolvedValue(undefined)
const clipboardWriteText = vi.fn().mockResolvedValue(undefined)

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(invoke).mockResolvedValue(null)
  Object.assign(stores.player, { currentTrack: null, lyrics: [] })
  Object.assign(stores.musicLibrary, { playlists: [], currentPlaylist: null })
  Object.assign(stores.theme, { themePreference: 'dark', isDark: true, primaryColor: '#000' })

  clipboardWrite.mockResolvedValue(undefined)
  clipboardWriteText.mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { write: clipboardWrite, writeText: clipboardWriteText },
  })
  vi.stubGlobal(
    'ClipboardItem',
    class {
      items: Record<string, Blob>
      constructor(items: Record<string, Blob>) {
        this.items = items
      }
    },
  )
  if (!URL.createObjectURL) {
    URL.createObjectURL = vi.fn(() => 'blob:mock')
    URL.revokeObjectURL = vi.fn()
  }
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('utils API', () => {
  describe('createCanvas', () => {
    it('creates a canvas with the requested size', () => {
      const { canvas, ctx } = api().utils.createCanvas(320, 180)
      expect(canvas.width).toBe(320)
      expect(canvas.height).toBe(180)
      // happy-dom 未实现 2D 上下文,ctx 为 null 属预期
      expect(ctx === null || typeof ctx === 'object').toBe(true)
    })
  })

  describe('canvasToBlob', () => {
    it('resolves the blob produced by canvas.toBlob', async () => {
      const canvas = fakeCanvas()
      const blob = await api().utils.canvasToBlob(canvas)
      expect(blob).toBeInstanceOf(Blob)
    })

    it('rejects with the default message when toBlob yields nothing', async () => {
      const canvas = fakeCanvas()
      canvas.toBlob = vi.fn((cb: BlobCallback) => cb(null))

      await expect(api().utils.canvasToBlob(canvas)).rejects.toThrow('Canvas 转换 Blob 失败')
    })

    it('forwards the type and quality', async () => {
      const canvas = fakeCanvas()
      await api().utils.canvasToBlob(canvas, 'image/jpeg', 0.5)
      expect(canvas.toBlob).toHaveBeenCalledWith(expect.any(Function), 'image/jpeg', 0.5)
    })
  })

  describe('canvasToDataURL', () => {
    it('delegates to canvas.toDataURL', () => {
      const canvas = fakeCanvas('data:image/webp;base64,ZZZ')
      expect(api().utils.canvasToDataURL(canvas)).toBe('data:image/webp;base64,ZZZ')
      expect(canvas.toDataURL).toHaveBeenCalledWith('image/png', 0.92)
    })

    it('forwards a custom type and quality', () => {
      const canvas = fakeCanvas()
      api().utils.canvasToDataURL(canvas, 'image/jpeg', 0.4)
      expect(canvas.toDataURL).toHaveBeenCalledWith('image/jpeg', 0.4)
    })
  })

  describe('dataURLToBlob', () => {
    it('parses the mime type from the data URL header', () => {
      const blob = api().utils.dataURLToBlob('data:image/gif;base64,R0lGODlhAQABAAAAACw=')
      expect(blob.type).toBe('image/gif')
      expect(blob.size).toBeGreaterThan(0)
    })

    it('uses the explicit mime type when given', () => {
      const blob = api().utils.dataURLToBlob('data:image/png;base64,AAAA', {
        mimeType: 'image/jpeg',
      })
      expect(blob.type).toBe('image/jpeg')
    })

    it('falls back to the provided default mime type', () => {
      // 头部没有 ";xxx" 段落 => 正则无法解析出 mime
      const blob = api().utils.dataURLToBlob('data:,', { fallbackMime: 'image/png' })
      expect(blob.type).toBe('image/png')
    })

    it('falls back to application/octet-stream when the header is unparsable', () => {
      const blob = api().utils.dataURLToBlob('data:,')
      expect(blob.type).toBe('application/octet-stream')
    })

    it('round-trips binary content', async () => {
      const blob = api().utils.dataURLToBlob(
        `data:application/octet-stream;base64,${btoa('hello')}`,
      )
      expect(await blob.text()).toBe('hello')
    })
  })

  describe('loadImage', () => {
    it('loads http/data/asset urls directly through Image', async () => {
      stubImage()
      const img = await api().utils.loadImage('https://cdn.example/cover.jpg')
      expect(img.naturalWidth).toBe(8)
    })

    it('accepts data: and asset: urls without touching the filesystem', async () => {
      stubImage()
      await api().utils.loadImage('data:image/png;base64,AAAA')
      await api().utils.loadImage('asset://localhost/cover.png')
      expect(readFile).not.toHaveBeenCalled()
    })

    it('rejects with the source when the image fails to load', async () => {
      stubImage(true)
      await expect(api().utils.loadImage('https://cdn.example/missing.jpg')).rejects.toThrow(
        '图片加载失败: decode failed',
      )
    })

    it('reads local files through the fs plugin and wraps them in a blob url', async () => {
      const createObjectURL = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:cover')
      const revokeObjectURL = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([1, 2, 3]))
      stubImage()

      const img = await api().utils.loadImage('/music/cover.jpg')

      expect(readFile).toHaveBeenCalledWith('/music/cover.jpg')
      expect(createObjectURL).toHaveBeenCalled()
      expect(revokeObjectURL).toHaveBeenCalledWith('blob:cover')
      expect(img.naturalHeight).toBe(4)
    })

    it('rejects when a local file cannot be read', async () => {
      vi.mocked(readFile).mockRejectedValue(new Error('ENOENT'))
      await expect(api().utils.loadImage('/missing/cover.jpg')).rejects.toThrow(
        '图片加载失败: ENOENT',
      )
    })

    it('rejects when a local file decodes to a broken image', async () => {
      vi.mocked(readFile).mockResolvedValue(new Uint8Array([0]))
      stubImage(true)
      await expect(api().utils.loadImage('/broken/cover.jpg')).rejects.toThrow('图片加载失败')
    })
  })

  describe('blobToArrayBuffer', () => {
    it('returns the underlying bytes', async () => {
      const buffer = await api().utils.blobToArrayBuffer(new Blob(['abc']))
      expect(new TextDecoder().decode(buffer)).toBe('abc')
    })
  })

  describe('formatTime', () => {
    it('formats durations through the shared helper', () => {
      expect(api().utils.formatTime(0)).toBe('0:00')
      expect(api().utils.formatTime(75)).toBe('1:15')
      expect(api().utils.formatTime(3725)).toBe('1:02:05')
    })

    it('clamps invalid values', () => {
      expect(api().utils.formatTime(Number.NaN)).toBe('0:00')
      expect(api().utils.formatTime(-10)).toBe('0:00')
    })
  })

  describe('generateId', () => {
    it('prefixes the id with the plugin id', () => {
      expect(api().utils.generateId()).toMatch(/^io-plugin-\d+-[a-z0-9]+$/)
    })

    it('produces unique ids', () => {
      const ids = new Set(Array.from({ length: 50 }, () => api().utils.generateId()))
      expect(ids.size).toBe(50)
    })
  })
})

describe('file API', () => {
  describe('saveAs', () => {
    it('writes string data as UTF-8', async () => {
      vi.mocked(save).mockResolvedValue('/tmp/out.txt')
      await api().file.saveAs('hello', { defaultName: 'out.txt' })

      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({ defaultPath: 'out.txt', title: '保存文件' }),
      )
      expect(writeFile).toHaveBeenCalledWith('/tmp/out.txt', new TextEncoder().encode('hello'))
      expect(logger.info).toHaveBeenCalledWith('[Plugin:io-plugin] 文件已保存: /tmp/out.txt')
    })

    it('writes Uint8Array data as-is', async () => {
      vi.mocked(save).mockResolvedValue('/tmp/bin.dat')
      const data = new Uint8Array([1, 2, 3])
      await api().file.saveAs(data)

      expect(writeFile).toHaveBeenCalledWith('/tmp/bin.dat', data)
    })

    it('unwraps a Blob into bytes', async () => {
      vi.mocked(save).mockResolvedValue('/tmp/blob.bin')
      await api().file.saveAs(new Blob(['ab']))

      expect(writeFile).toHaveBeenCalledWith('/tmp/blob.bin', new Uint8Array([97, 98]))
    })

    it('uses the caller supplied filters and title', async () => {
      vi.mocked(save).mockResolvedValue('/tmp/x.png')
      await api().file.saveAs(new Blob(['x']), {
        filters: [{ name: '图片', extensions: ['png'] }],
        title: '导出封面',
      })

      expect(save).toHaveBeenCalledWith(
        expect.objectContaining({
          filters: [{ name: '图片', extensions: ['png'] }],
          title: '导出封面',
        }),
      )
    })

    it('returns null when the user cancels the dialog', async () => {
      vi.mocked(save).mockResolvedValue(null)
      await expect(api().file.saveAs('x')).resolves.toBeNull()
      expect(writeFile).not.toHaveBeenCalled()
    })

    it('logs and rethrows write failures', async () => {
      vi.mocked(save).mockResolvedValue('/tmp/out.txt')
      vi.mocked(writeFile).mockRejectedValue(new Error('read-only'))

      await expect(api().file.saveAs('x')).rejects.toThrow('read-only')
      expect(logger.error).toHaveBeenCalled()
    })

    it('requires STORAGE', async () => {
      await expect(api([]).file.saveAs('x')).rejects.toThrow(/file:write/)
      expect(save).not.toHaveBeenCalled()
    })
  })

  describe('saveImage', () => {
    it('converts a canvas and forwards the bytes to the backend', async () => {
      vi.mocked(invoke).mockResolvedValue('/shots/cover.png')
      const canvas = fakeCanvas()

      const result = await api().file.saveImage(canvas, 'cover.png', 'png')

      expect(result).toBe('/shots/cover.png')
      const [, payload] = vi.mocked(invoke).mock.calls[0] as [
        string,
        { filename: string; data: number[] },
      ]
      expect(payload.filename).toBe('cover.png')
      expect(payload.data.length).toBe(3) // 'png'
    })

    it('maps the jpg extension to image/jpeg', async () => {
      vi.mocked(invoke).mockResolvedValue('/shots/cover.jpg')
      const canvas = fakeCanvas()
      canvas.toBlob = vi.fn((cb: BlobCallback) => cb(new Blob(['jpeg'], { type: 'image/jpeg' })))

      await api().file.saveImage(canvas, 'cover.jpg', 'jpg')

      expect(canvas.toBlob).toHaveBeenCalledWith(expect.any(Function), 'image/jpeg', 0.92)
      expect(vi.mocked(invoke).mock.calls[0]?.[0]).toBe('save_screenshot')
    })

    it('propagates a canvas conversion failure', async () => {
      const canvas = fakeCanvas()
      canvas.toBlob = vi.fn((cb: BlobCallback) => cb(null))

      await expect(api().file.saveImage(canvas)).rejects.toThrow('Canvas 转换失败')
    })

    it('accepts a Blob directly', async () => {
      vi.mocked(invoke).mockResolvedValue('/shots/b.png')
      await api().file.saveImage(new Blob(['png']))
      expect(invoke).toHaveBeenCalledWith('save_screenshot', {
        filename: 'image.png',
        data: [112, 110, 103],
      })
    })

    it('decodes a data URL with the requested mime type', async () => {
      vi.mocked(invoke).mockResolvedValue('/shots/d.webp')
      await api().file.saveImage('data:image/webp;base64,AAAA', 'd.webp', 'webp')
      expect(invoke).toHaveBeenCalledWith(
        'save_screenshot',
        expect.objectContaining({ filename: 'd.webp' }),
      )
    })

    it('rejects an unsupported input type', async () => {
      await expect(api().file.saveImage('/not/an/image.png')).rejects.toThrow('不支持的图片格式')
      expect(invoke).not.toHaveBeenCalled()
    })

    it('requires STORAGE', async () => {
      await expect(api([]).file.saveImage(new Blob(['x']))).rejects.toThrow(/file:write/)
    })
  })

  describe('openScreenshotsDirectory', () => {
    it('invokes the backend command', async () => {
      await api().file.openScreenshotsDirectory()
      expect(invoke).toHaveBeenCalledWith('open_screenshots_directory')
    })
  })
})

describe('clipboard API', () => {
  it('copies a canvas as a PNG clipboard item', async () => {
    const canvas = fakeCanvas()
    await api().clipboard.writeImage(canvas)

    expect(clipboardWrite).toHaveBeenCalledTimes(1)
    const [items] = clipboardWrite.mock.calls[0] as [{ items: Record<string, Blob> }[]]
    expect(Object.keys(items[0]!.items)).toEqual(['image/png'])
  })

  it('copies a Blob using its own mime type', async () => {
    await api().clipboard.writeImage(new Blob(['gif'], { type: 'image/gif' }))

    const [items] = clipboardWrite.mock.calls[0] as [{ items: Record<string, Blob> }[]]
    expect(Object.keys(items[0]!.items)).toEqual(['image/gif'])
  })

  it('decodes a data URL with a PNG fallback', async () => {
    await api().clipboard.writeImage('data:image/png;base64,AAAA')

    const [items] = clipboardWrite.mock.calls[0] as [{ items: Record<string, Blob> }[]]
    expect(Object.keys(items[0]!.items)).toEqual(['image/png'])
  })

  it('rejects an unsupported image input', async () => {
    await expect(api().clipboard.writeImage(123 as never)).rejects.toThrow('不支持的图片格式')
    expect(clipboardWrite).not.toHaveBeenCalled()
  })

  it('logs and rethrows clipboard failures', async () => {
    clipboardWrite.mockRejectedValueOnce(new Error('denied'))
    await expect(api().clipboard.writeImage(new Blob(['x']))).rejects.toThrow('denied')
    expect(logger.error).toHaveBeenCalled()
  })

  it('copies text', async () => {
    await api().clipboard.writeText('hello')
    expect(clipboardWriteText).toHaveBeenCalledWith('hello')
    expect(logger.info).toHaveBeenCalledWith('[Plugin:io-plugin] 文本已复制到剪贴板')
  })

  it('logs and rethrows text copy failures', async () => {
    clipboardWriteText.mockRejectedValueOnce(new Error('denied'))
    await expect(api().clipboard.writeText('hello')).rejects.toThrow('denied')
    expect(logger.error).toHaveBeenCalled()
  })

  it('requires STORAGE for both clipboard methods', async () => {
    await expect(api([]).clipboard.writeImage(new Blob(['x']))).rejects.toThrow(/clipboard:write/)
    await expect(api([]).clipboard.writeText('x')).rejects.toThrow(/clipboard:write/)
  })
})
