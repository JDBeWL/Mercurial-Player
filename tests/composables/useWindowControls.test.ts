// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'

const windowApi = vi.hoisted(() => ({
  minimize: vi.fn(),
  close: vi.fn(),
  setFullscreen: vi.fn(),
  unmaximize: vi.fn(),
  isFullscreen: vi.fn(),
  isMaximized: vi.fn(),
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowApi,
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { useWindowControls } = await import('@/composables/useWindowControls')
const logger = (await import('@/utils/logger')).default

beforeEach(() => {
  vi.clearAllMocks()
  windowApi.minimize.mockResolvedValue(undefined)
  windowApi.close.mockResolvedValue(undefined)
  windowApi.setFullscreen.mockResolvedValue(undefined)
  windowApi.unmaximize.mockResolvedValue(undefined)
  windowApi.isFullscreen.mockResolvedValue(false)
  windowApi.isMaximized.mockResolvedValue(false)
})

describe('useWindowControls', () => {
  it('starts with both flags false', () => {
    const { isFullscreen, isMaximized } = useWindowControls()
    expect(isFullscreen.value).toBe(false)
    expect(isMaximized.value).toBe(false)
  })

  it('minimizes the window', async () => {
    await useWindowControls().minimizeWindow()
    expect(windowApi.minimize).toHaveBeenCalled()
  })

  it('closes the window', async () => {
    await useWindowControls().closeWindow()
    expect(windowApi.close).toHaveBeenCalled()
  })

  it('logs instead of throwing when minimize fails', async () => {
    windowApi.minimize.mockRejectedValue(new Error('nope'))
    await expect(useWindowControls().minimizeWindow()).resolves.toBeUndefined()
    expect(logger.error).toHaveBeenCalledWith('Failed to minimize window:', expect.any(Error))
  })

  it('logs instead of throwing when close fails', async () => {
    windowApi.close.mockRejectedValue(new Error('nope'))
    await expect(useWindowControls().closeWindow()).resolves.toBeUndefined()
    expect(logger.error).toHaveBeenCalledWith('Failed to close window:', expect.any(Error))
  })

  describe('toggleFullscreen', () => {
    it('unmaximizes before entering fullscreen', async () => {
      windowApi.isMaximized.mockResolvedValue(true)
      const { toggleFullscreen, isFullscreen } = useWindowControls()

      await toggleFullscreen()

      expect(windowApi.unmaximize).toHaveBeenCalled()
      expect(windowApi.setFullscreen).toHaveBeenCalledWith(true)
      expect(isFullscreen.value).toBe(true)
    })

    it('skips unmaximize when the window is not maximized', async () => {
      windowApi.isMaximized.mockResolvedValue(false)
      await useWindowControls().toggleFullscreen()

      expect(windowApi.unmaximize).not.toHaveBeenCalled()
      expect(windowApi.setFullscreen).toHaveBeenCalledWith(true)
    })

    it('exits fullscreen when already fullscreen', async () => {
      const { toggleFullscreen, isFullscreen } = useWindowControls()
      isFullscreen.value = true

      await toggleFullscreen()

      expect(windowApi.setFullscreen).toHaveBeenCalledWith(false)
      expect(isFullscreen.value).toBe(false)
    })

    it('keeps the flag false when entering fullscreen fails', async () => {
      windowApi.setFullscreen.mockRejectedValue(new Error('denied'))
      const { toggleFullscreen, isFullscreen } = useWindowControls()

      await toggleFullscreen()

      expect(isFullscreen.value).toBe(false)
      expect(logger.error).toHaveBeenCalledWith('Failed to toggle fullscreen:', expect.any(Error))
    })
  })

  describe('syncWindowState', () => {
    it('reads both flags from the window', async () => {
      windowApi.isFullscreen.mockResolvedValue(true)
      windowApi.isMaximized.mockResolvedValue(true)
      const { syncWindowState, isFullscreen, isMaximized } = useWindowControls()

      await syncWindowState()

      expect(isFullscreen.value).toBe(true)
      expect(isMaximized.value).toBe(true)
    })

    it('logs instead of throwing when the query fails', async () => {
      windowApi.isFullscreen.mockRejectedValue(new Error('nope'))
      const { syncWindowState, isFullscreen } = useWindowControls()

      await expect(syncWindowState()).resolves.toBeUndefined()
      expect(isFullscreen.value).toBe(false)
      expect(logger.error).toHaveBeenCalledWith('Failed to check window state:', expect.any(Error))
    })
  })
})
