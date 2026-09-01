// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, ref } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import type { WatchStopHandle } from 'vue'

const windowApi = vi.hoisted(() => ({
  onCloseRequested: vi.fn(),
  onMoved: vi.fn(),
  destroy: vi.fn(),
}))

const stores = vi.hoisted(() => ({
  player: {
    initAudio: vi.fn(),
    cleanup: vi.fn(),
    resumeLastSession: vi.fn(),
  },
  config: {
    loadConfig: vi.fn(),
    flushPendingSave: vi.fn(),
    general: {
      language: 'zh',
      theme: 'dark' as string | undefined,
      coverCachePath: '/tmp/covers',
      enableAutoUpdate: false,
    },
    visualizer: { enableVerticalSync: false, targetFps: 60 },
  },
  theme: {
    setThemePreference: vi.fn(),
    applyTheme: vi.fn(),
  },
  pluginManager: { cleanup: vi.fn() },
  setLocale: vi.fn(),
  applyVisualizerFps: vi.fn(),
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => windowApi,
}))

vi.mock('@/stores/player', () => ({ usePlayerStore: () => stores.player }))
vi.mock('@/stores/config', () => ({ useConfigStore: () => stores.config }))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => stores.theme }))
vi.mock('@/plugins', () => ({ pluginManager: stores.pluginManager }))
vi.mock('@/i18n', () => ({ setLocale: stores.setLocale }))
vi.mock('@/utils/visualizerFps', () => ({
  applyVisualizerFps: stores.applyVisualizerFps,
}))
vi.mock('vue-i18n', () => ({
  useI18n: () => ({ t: (key: string) => `i18n:${key}` }),
}))
vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { mockInvoke } = await import('../mocks/tauri')
const logger = (await import('@/utils/logger')).default
const { useAppLifecycle } = await import('@/composables/useAppLifecycle')

type CloseHandler = (event: { preventDefault: () => void }) => void | Promise<void>
type MoveHandler = () => void

let closeHandler: CloseHandler | null = null
let moveHandler: MoveHandler | null = null
let unlistenClose: ReturnType<typeof vi.fn>
let unlistenMove: ReturnType<typeof vi.fn>

const createOptions = () => ({
  checkForUpdates: vi.fn(),
  updateAvailable: ref(false),
  newVersion: ref(''),
  showError: vi.fn(),
  unsubscribeErrorNotification: vi.fn(),
  syncWindowState: vi.fn(),
  stopWatchTrack: vi.fn() as unknown as WatchStopHandle | null,
})

// 记录所有挂载实例:未卸载的组件会一直持有 beforeunload 监听,
// 跨用例累积后会污染后续断言
const mounted: ReturnType<typeof mount>[] = []

const mountLifecycle = (options: ReturnType<typeof createOptions>) => {
  const wrapper = mount(
    defineComponent({
      setup() {
        useAppLifecycle(options)
        return () => null
      },
    }),
  )
  mounted.push(wrapper)
  return wrapper
}

/** 挂载并等待异步 onMounted 序列全部完成 */
const mountAndSettle = async (options: ReturnType<typeof createOptions>) => {
  const wrapper = mountLifecycle(options)
  await flushPromises()
  return wrapper
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.useRealTimers()
  closeHandler = null
  moveHandler = null
  unlistenClose = vi.fn()
  unlistenMove = vi.fn()

  windowApi.onCloseRequested.mockImplementation((handler: CloseHandler) => {
    closeHandler = handler
    return Promise.resolve(unlistenClose)
  })
  windowApi.onMoved.mockImplementation((handler: MoveHandler) => {
    moveHandler = handler
    return Promise.resolve(unlistenMove)
  })
  windowApi.destroy.mockResolvedValue(undefined)

  mockInvoke.mockResolvedValue(undefined)
  stores.player.initAudio.mockResolvedValue(undefined)
  stores.player.cleanup.mockResolvedValue(undefined)
  stores.player.resumeLastSession.mockResolvedValue(undefined)
  stores.config.loadConfig.mockResolvedValue(undefined)
  stores.config.flushPendingSave.mockResolvedValue(undefined)
  stores.config.general.language = 'zh'
  stores.config.general.theme = 'dark'
  stores.config.general.coverCachePath = '/tmp/covers'
  stores.config.general.enableAutoUpdate = false
  stores.config.visualizer = { enableVerticalSync: false, targetFps: 60 }
  stores.pluginManager.cleanup.mockResolvedValue(undefined)
  stores.setLocale.mockReturnValue(undefined)
  stores.applyVisualizerFps.mockResolvedValue({ fps: 60 })
})

afterEach(async () => {
  vi.useRealTimers()
  while (mounted.length > 0) {
    mounted.pop()?.unmount()
  }
  await flushPromises()
})

describe('useAppLifecycle > mount sequence', () => {
  it('loads the config with UI reset enabled', async () => {
    await mountAndSettle(createOptions())
    expect(stores.config.loadConfig).toHaveBeenCalledWith(true)
  })

  it('applies the language from the config', async () => {
    stores.config.general.language = 'en'
    await mountAndSettle(createOptions())
    expect(stores.setLocale).toHaveBeenCalledWith('en')
  })

  it('falls back to zh when no language is configured', async () => {
    stores.config.general.language = ''
    await mountAndSettle(createOptions())
    expect(stores.setLocale).toHaveBeenCalledWith('zh')
  })

  it('applies the saved theme preference and then the theme itself', async () => {
    stores.config.general.theme = 'light'
    await mountAndSettle(createOptions())
    expect(stores.theme.setThemePreference).toHaveBeenCalledWith('light')
    expect(stores.theme.applyTheme).toHaveBeenCalled()
  })

  it('skips the preference when the config has no theme', async () => {
    stores.config.general.theme = undefined
    await mountAndSettle(createOptions())
    expect(stores.theme.setThemePreference).not.toHaveBeenCalled()
    // 主题仍要应用一次,保证首屏配色正确
    expect(stores.theme.applyTheme).toHaveBeenCalled()
  })

  it('pushes the cover cache path to the backend', async () => {
    stores.config.general.coverCachePath = 'D:/cache'
    await mountAndSettle(createOptions())
    expect(mockInvoke).toHaveBeenCalledWith('set_cover_cache_path_command', { path: 'D:/cache' })
  })

  it('initialises audio, resumes the last session and syncs the window state', async () => {
    const options = createOptions()
    await mountAndSettle(options)
    expect(stores.player.initAudio).toHaveBeenCalled()
    expect(stores.player.resumeLastSession).toHaveBeenCalled()
    expect(options.syncWindowState).toHaveBeenCalled()
  })

  it('applies the visualizer target FPS and logs the result', async () => {
    stores.applyVisualizerFps.mockResolvedValue({ fps: 75 })
    await mountAndSettle(createOptions())
    expect(stores.applyVisualizerFps).toHaveBeenCalledWith(stores.config.visualizer)
    expect(logger.info).toHaveBeenCalledWith('Target FPS set to 75')
  })

  it('stays quiet when the backend reports no FPS', async () => {
    stores.applyVisualizerFps.mockResolvedValue(null)
    await mountAndSettle(createOptions())
    expect(logger.info).not.toHaveBeenCalledWith(expect.stringContaining('Target FPS'))
  })

  it('skips the update check when auto update is disabled', async () => {
    const options = createOptions()
    stores.config.general.enableAutoUpdate = false
    await mountAndSettle(options)
    expect(options.checkForUpdates).not.toHaveBeenCalled()
  })

  it('checks for updates on startup when enabled', async () => {
    const options = createOptions()
    stores.config.general.enableAutoUpdate = true
    await mountAndSettle(options)
    expect(options.checkForUpdates).toHaveBeenCalled()
  })

  it('notifies the user when an update is available', async () => {
    const options = createOptions()
    stores.config.general.enableAutoUpdate = true
    options.updateAvailable.value = true
    options.newVersion.value = '2.1.0'

    await mountAndSettle(options)

    expect(options.showError).toHaveBeenCalledWith(
      'i18n:config.updateAvailable v2.1.0',
      'info',
      10000,
    )
  })

  it('does not notify when the app is already up to date', async () => {
    const options = createOptions()
    stores.config.general.enableAutoUpdate = true
    options.updateAvailable.value = false

    await mountAndSettle(options)

    expect(options.showError).not.toHaveBeenCalled()
  })
})

describe('useAppLifecycle > mount failure tolerance', () => {
  it('logs and continues when the config fails to load', async () => {
    stores.config.loadConfig.mockRejectedValue(new Error('bad config'))
    await expect(mountAndSettle(createOptions())).resolves.toBeDefined()
    expect(logger.warn).toHaveBeenCalledWith('Failed to load configuration:', expect.any(Error))
    // 后续步骤不应被阻断
    expect(stores.player.initAudio).toHaveBeenCalled()
  })

  it('logs and continues when the locale cannot be applied', async () => {
    stores.setLocale.mockImplementation(() => {
      throw new Error('unknown locale')
    })
    await mountAndSettle(createOptions())
    expect(logger.error).toHaveBeenCalledWith(
      'Failed to apply language from config:',
      expect.any(Error),
    )
  })

  it('logs and continues when the theme cannot be applied', async () => {
    stores.theme.setThemePreference.mockImplementation(() => {
      throw new Error('bad theme')
    })
    await mountAndSettle(createOptions())
    expect(logger.error).toHaveBeenCalledWith(
      'Failed to apply theme from config:',
      expect.any(Error),
    )
  })

  it('logs and continues when the cover cache path is rejected', async () => {
    mockInvoke.mockRejectedValue(new Error('denied'))
    await mountAndSettle(createOptions())
    expect(logger.warn).toHaveBeenCalledWith('Failed to set cover cache path:', expect.any(Error))
  })

  it('logs and continues when the last session cannot be restored', async () => {
    stores.player.resumeLastSession.mockRejectedValue(new Error('gone'))
    await mountAndSettle(createOptions())
    expect(logger.warn).toHaveBeenCalledWith('Failed to resume last session:', expect.any(Error))
  })

  it('logs and continues when the target FPS cannot be applied', async () => {
    stores.applyVisualizerFps.mockRejectedValue(new Error('unsupported'))
    await mountAndSettle(createOptions())
    expect(logger.warn).toHaveBeenCalledWith('Failed to set target FPS:', expect.any(Error))
  })

  it('logs and continues when the close-requested listener cannot be registered', async () => {
    windowApi.onCloseRequested.mockRejectedValue(new Error('no listener'))
    await mountAndSettle(createOptions())
    expect(logger.warn).toHaveBeenCalledWith('Failed to listen close requested:', expect.any(Error))
  })

  it('logs and continues when the move listener cannot be registered', async () => {
    windowApi.onMoved.mockRejectedValue(new Error('no listener'))
    await mountAndSettle(createOptions())
    expect(logger.warn).toHaveBeenCalledWith('Failed to listen window move:', expect.any(Error))
  })

  it('logs and continues when the update check throws', async () => {
    const options = createOptions()
    stores.config.general.enableAutoUpdate = true
    options.checkForUpdates.mockRejectedValue(new Error('offline'))

    await mountAndSettle(options)

    expect(logger.warn).toHaveBeenCalledWith('Auto update check failed:', expect.any(Error))
    // 窗口状态同步仍应执行
    expect(options.syncWindowState).toHaveBeenCalled()
  })
})

describe('useAppLifecycle > beforeunload fallback', () => {
  it('fires the flush and cleanup IPCs without awaiting them', async () => {
    await mountAndSettle(createOptions())

    window.dispatchEvent(new Event('beforeunload'))

    expect(stores.config.flushPendingSave).toHaveBeenCalled()
    expect(stores.player.cleanup).toHaveBeenCalled()
    expect(stores.pluginManager.cleanup).toHaveBeenCalled()
  })

  it('removes the listener on unmount so it never fires again', async () => {
    const wrapper = await mountAndSettle(createOptions())
    wrapper.unmount()
    await flushPromises()
    vi.clearAllMocks()

    window.dispatchEvent(new Event('beforeunload'))

    expect(stores.config.flushPendingSave).not.toHaveBeenCalled()
    expect(stores.player.cleanup).not.toHaveBeenCalled()
  })
})

describe('useAppLifecycle > close requested', () => {
  const dispatchClose = async () => {
    const event = { preventDefault: vi.fn() }
    await closeHandler?.(event)
    return event
  }

  it('prevents the default close and flushes resources before destroying', async () => {
    await mountAndSettle(createOptions())

    const event = await dispatchClose()

    expect(event.preventDefault).toHaveBeenCalled()
    expect(stores.config.flushPendingSave).toHaveBeenCalled()
    expect(stores.player.cleanup).toHaveBeenCalled()
    expect(stores.pluginManager.cleanup).toHaveBeenCalled()
    expect(windowApi.destroy).toHaveBeenCalled()
  })

  it('still destroys the window when a cleanup step rejects', async () => {
    stores.player.cleanup.mockRejectedValue(new Error('cleanup failed'))
    await mountAndSettle(createOptions())

    await dispatchClose()

    expect(logger.error).toHaveBeenCalledWith(
      'Failed to flush resources on close:',
      expect.any(Error),
    )
    expect(windowApi.destroy).toHaveBeenCalled()
  })

  it('ignores repeated close requests while the cleanup is running', async () => {
    // 让 flush 挂起,模拟清理进行中用户再次点击关闭
    let release: () => void = () => {}
    stores.config.flushPendingSave.mockImplementation(
      () => new Promise<void>((resolve) => (release = resolve)),
    )
    await mountAndSettle(createOptions())

    const first = { preventDefault: vi.fn() }
    void closeHandler?.(first)
    await Promise.resolve()

    const second = { preventDefault: vi.fn() }
    await closeHandler?.(second)

    expect(second.preventDefault).toHaveBeenCalled()
    expect(windowApi.destroy).not.toHaveBeenCalled()

    release()
    await flushPromises()
    expect(windowApi.destroy).toHaveBeenCalledTimes(1)
  })
})

describe('useAppLifecycle > window move', () => {
  it('re-applies the target FPS after the debounce window', async () => {
    vi.useFakeTimers()
    stores.config.visualizer = { enableVerticalSync: true, targetFps: 60 }
    await mountAndSettle(createOptions())
    stores.applyVisualizerFps.mockClear()

    moveHandler?.()
    vi.advanceTimersByTime(499)
    expect(stores.applyVisualizerFps).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(stores.applyVisualizerFps).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  it('coalesces bursts of move events into a single re-apply', async () => {
    vi.useFakeTimers()
    stores.config.visualizer = { enableVerticalSync: true, targetFps: 60 }
    await mountAndSettle(createOptions())
    stores.applyVisualizerFps.mockClear()

    moveHandler?.()
    vi.advanceTimersByTime(100)
    moveHandler?.()
    vi.advanceTimersByTime(100)
    moveHandler?.()
    vi.advanceTimersByTime(500)

    expect(stores.applyVisualizerFps).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  it('does nothing when vertical sync limiting is off', async () => {
    vi.useFakeTimers()
    stores.config.visualizer = { enableVerticalSync: false, targetFps: 60 }
    await mountAndSettle(createOptions())
    stores.applyVisualizerFps.mockClear()

    moveHandler?.()
    vi.advanceTimersByTime(1000)

    expect(stores.applyVisualizerFps).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('logs when the re-apply after a move fails', async () => {
    vi.useFakeTimers()
    stores.config.visualizer = { enableVerticalSync: true, targetFps: 60 }
    stores.applyVisualizerFps.mockRejectedValue(new Error('nope'))
    await mountAndSettle(createOptions())

    moveHandler?.()
    vi.advanceTimersByTime(500)
    await vi.advanceTimersByTimeAsync(0)

    expect(logger.warn).toHaveBeenCalledWith(
      'Failed to re-apply target FPS after window move:',
      expect.any(Error),
    )
    vi.useRealTimers()
  })
})

describe('useAppLifecycle > unmount', () => {
  it('flushes the config and cleans up the player and plugins', async () => {
    const wrapper = await mountAndSettle(createOptions())
    vi.clearAllMocks()

    wrapper.unmount()
    await flushPromises()

    expect(stores.config.flushPendingSave).toHaveBeenCalled()
    expect(stores.player.cleanup).toHaveBeenCalled()
  })

  it('unsubscribes the error notification bridge and stops the track watcher', async () => {
    const options = createOptions()
    const wrapper = await mountAndSettle(options)

    wrapper.unmount()
    await flushPromises()

    expect(options.unsubscribeErrorNotification).toHaveBeenCalled()
    expect(options.stopWatchTrack).toHaveBeenCalled()
  })

  it('tolerates a null track watcher handle', async () => {
    const options = createOptions()
    options.stopWatchTrack = null
    const wrapper = await mountAndSettle(options)

    expect(() => wrapper.unmount()).not.toThrow()
    await flushPromises()
  })

  it('keeps cleaning up when the config flush rejects', async () => {
    const options = createOptions()
    const wrapper = await mountAndSettle(options)
    stores.config.flushPendingSave.mockRejectedValue(new Error('disk full'))

    wrapper.unmount()
    await flushPromises()

    // 即使失败,监听器与播放器清理仍必须执行
    expect(logger.warn).toHaveBeenCalledWith(
      'Failed to flush config on unmount:',
      expect.any(Error),
    )
    expect(options.unsubscribeErrorNotification).toHaveBeenCalled()
    expect(options.stopWatchTrack).toHaveBeenCalled()
    expect(stores.player.cleanup).toHaveBeenCalled()
  })

  it('logs instead of throwing when the player cleanup rejects', async () => {
    const wrapper = await mountAndSettle(createOptions())
    stores.player.cleanup.mockRejectedValue(new Error('device busy'))

    expect(() => wrapper.unmount()).not.toThrow()
    await flushPromises()

    expect(logger.warn).toHaveBeenCalledWith(
      'Failed to cleanup player on unmount:',
      expect.any(Error),
    )
  })

  it('releases both window listeners', async () => {
    const wrapper = await mountAndSettle(createOptions())

    wrapper.unmount()
    await flushPromises()

    expect(unlistenClose).toHaveBeenCalled()
    expect(unlistenMove).toHaveBeenCalled()
  })

  it('drops a pending move debounce timer instead of firing it', async () => {
    vi.useFakeTimers()
    stores.config.visualizer = { enableVerticalSync: true, targetFps: 60 }
    const wrapper = await mountAndSettle(createOptions())
    stores.applyVisualizerFps.mockClear()

    moveHandler?.()
    wrapper.unmount()
    vi.advanceTimersByTime(1000)

    expect(stores.applyVisualizerFps).not.toHaveBeenCalled()
    vi.useRealTimers()
  })
})
