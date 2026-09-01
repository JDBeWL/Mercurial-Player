// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, nextTick } from 'vue'
import { mount } from '@vue/test-utils'
import type { LyricLine } from '@/types'

const errorApi = vi.hoisted(() => ({ handle: vi.fn() }))
const eventApi = vi.hoisted(() => ({ listen: vi.fn() }))
const loggerApi = vi.hoisted(() => ({
  debug: vi.fn(),
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}))

// 该 composable 持有模块级单例(isInitialized / refCount / 定时器 / 去重缓存),
// 因此整个文件只加载一次模块;卸载最后一个组件会复位除"去重缓存"外的全部状态,
// 各用例使用互不相同的播放状态即可避免被上一条用例的去重缓存吞掉。

vi.mock('@/utils/errorHandler', () => ({
  default: { handle: errorApi.handle },
  ErrorSeverity: { LOW: 'low', MEDIUM: 'medium', HIGH: 'high' },
}))
vi.mock('@/utils/logger', () => ({ default: loggerApi }))
vi.mock('@tauri-apps/api/event', () => ({ listen: eventApi.listen }))

const stores = vi.hoisted(() => ({
  player: null as PlayerState | null,
  config: null as ConfigState | null,
}))

interface PlayerState {
  lyrics: LyricLine[] | null
  currentLyricIndex: number
  currentTime: number
  lyricsOffset: number
  isPlaying: boolean
  currentTrack: { path: string } | null
}

interface ConfigState {
  setDesktopLyricsConfig: ReturnType<typeof vi.fn>
  lyrics: {
    lyricsFontFamily: string
    translationFontFamily: string
    desktopLyrics: {
      enabled: boolean
      locked: boolean
      fontSize: number
      colorPreset: string
    }
  }
}

const createPlayerState = (): PlayerState => ({
  lyrics: null,
  currentLyricIndex: -1,
  currentTime: 0,
  lyricsOffset: 0,
  isPlaying: false,
  currentTrack: null,
})

const createConfigState = (): ConfigState => ({
  setDesktopLyricsConfig: vi.fn(),
  lyrics: {
    lyricsFontFamily: 'Noto Sans SC',
    translationFontFamily: '',
    desktopLyrics: { enabled: false, locked: true, fontSize: 28, colorPreset: 'auto' },
  },
})

// 返回 reactive 代理:composable 的 watch 与测试共用同一份响应式状态
vi.mock('@/stores/player', async () => {
  const vue = await import('vue')
  stores.player ??= vue.reactive(createPlayerState())
  return { usePlayerStore: () => stores.player }
})
vi.mock('@/stores/config', async () => {
  const vue = await import('vue')
  stores.config ??= vue.reactive(createConfigState())
  return { useConfigStore: () => stores.config }
})

const { mockInvoke } = await import('../mocks/tauri')
const { useDesktopLyrics } = await import('@/composables/useDesktopLyrics')

const POLL_INTERVAL_MS = 16

type EventHandler = (event: { payload: unknown }) => void
let eventHandlers: Record<string, EventHandler>
let frameCallbacks: Array<() => void>
const mounted: ReturnType<typeof mount>[] = []

const mountDesktopLyrics = async () => {
  const wrapper = mount(
    defineComponent({
      setup() {
        useDesktopLyrics()
        return () => null
      },
    }),
  )
  mounted.push(wrapper)
  await settle()
  return wrapper
}

/**
 * 替代 @vue/test-utils 的 flushPromises:它内部用 setTimeout,
 * 在 fake timers 下永远不会 resolve
 */
const settle = async () => {
  await vi.advanceTimersByTimeAsync(0)
  await Promise.resolve()
  await Promise.resolve()
}

const flushFrames = async () => {
  const callbacks = frameCallbacks
  frameCallbacks = []
  for (const cb of callbacks) cb()
  await settle()
}

/** 推进一次轮询:触发 interval → 调度 rAF → 执行更新 */
const tick = async () => {
  await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS)
  await flushFrames()
}

const invokeCalls = (command: string) => mockInvoke.mock.calls.filter(([cmd]) => cmd === command)

const lastUpdatePayload = () => {
  const calls = invokeCalls('update_desktop_lyric')
  return calls[calls.length - 1]?.[1] as Record<string, unknown> | undefined
}

const player = () => stores.player as PlayerState
const config = () => stores.config as ConfigState

const karaokeLyrics = (): LyricLine[] => [
  {
    time: 0,
    text: 'Hello',
    texts: ['Hello', '世界'],
    words: [
      { text: 'Hel', start: 0, end: 1 },
      { text: 'lo', start: 1, end: 2 },
    ],
  } as unknown as LyricLine,
]

const resetState = () => {
  Object.assign(player(), createPlayerState())
  Object.assign(config().lyrics.desktopLyrics, {
    enabled: false,
    locked: true,
    fontSize: 28,
    colorPreset: 'auto',
  })
  config().lyrics.lyricsFontFamily = 'Noto Sans SC'
  config().lyrics.translationFontFamily = ''
  config().setDesktopLyricsConfig.mockClear()
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
  eventHandlers = {}
  frameCallbacks = []
  mounted.length = 0
  resetState()

  eventApi.listen.mockImplementation((name: string, handler: EventHandler) => {
    eventHandlers[name] = handler
    return Promise.resolve(vi.fn())
  })

  vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
    frameCallbacks.push(cb)
    return frameCallbacks.length
  })
  vi.stubGlobal('cancelAnimationFrame', () => {})

  mockInvoke.mockResolvedValue(undefined)
})

afterEach(() => {
  while (mounted.length > 0) mounted.pop()?.unmount()
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('useDesktopLyrics > mount', () => {
  it('does not touch the window while desktop lyrics are disabled', async () => {
    await mountDesktopLyrics()

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 5)
    await flushFrames()

    expect(invokeCalls('show_desktop_lyrics')).toHaveLength(0)
    expect(invokeCalls('update_desktop_lyric')).toHaveLength(0)
  })

  it('shows the window and syncs every setting when enabled', async () => {
    config().lyrics.desktopLyrics.enabled = true
    config().lyrics.desktopLyrics.locked = false
    config().lyrics.desktopLyrics.fontSize = 36
    config().lyrics.desktopLyrics.colorPreset = 'dark'

    await mountDesktopLyrics()

    expect(mockInvoke).toHaveBeenCalledWith('show_desktop_lyrics')
    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_locked', { locked: false })
    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_font_size', { size: 36 })
    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_font_family', {
      fontFamily: 'Noto Sans SC',
      translationFontFamily: '',
    })
    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_color_preset', { preset: 'dark' })
  })

  it('polls the backend while enabled', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 0.5
    await mountDesktopLyrics()

    await tick()

    expect(invokeCalls('update_desktop_lyric').length).toBeGreaterThan(0)
  })

  it('logs but does not throw when showing the window fails', async () => {
    config().lyrics.desktopLyrics.enabled = true
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === 'show_desktop_lyrics'
        ? Promise.reject(new Error('no window'))
        : Promise.resolve(undefined),
    )

    await expect(mountDesktopLyrics()).resolves.toBeDefined()

    expect(loggerApi.error).toHaveBeenCalledWith(
      'Failed to show desktop lyrics:',
      expect.any(Error),
    )
  })
})

describe('useDesktopLyrics > lyric payload', () => {
  it('sends the current line, its translation and the karaoke words', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 0.5
    player().isPlaying = true
    await mountDesktopLyrics()

    await tick()

    const payload = lastUpdatePayload()
    expect(payload).toMatchObject({
      currentLine: 'Hello',
      subLine: '世界',
      currentTime: 0.5,
      isPlaying: true,
    })
    expect(payload?.words).toEqual([
      { text: 'Hel', start: 0, end: 1 },
      { text: 'lo', start: 1, end: 2 },
    ])
  })

  it('computes the karaoke progress from the passed characters', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 0.4
    await mountDesktopLyrics()

    await tick()

    // 共 5 个字符:第 1 个词 3 字符走过 0.4/1 → 1.2 / 5 = 0.24
    expect(lastUpdatePayload()?.progress).toBeCloseTo(0.24)
  })

  it('clamps the progress to the line boundaries', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = -10
    await mountDesktopLyrics()
    await tick()
    expect(lastUpdatePayload()?.progress).toBe(0)

    player().currentTime = 99
    await tick()
    expect(lastUpdatePayload()?.progress).toBe(1)
  })

  it('reports zero progress for plain LRC without word timings', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = [{ time: 0, text: 'plain', texts: ['plain'] }] as unknown as LyricLine[]
    player().currentLyricIndex = 0
    player().currentTime = 1
    await mountDesktopLyrics()

    await tick()

    expect(lastUpdatePayload()?.progress).toBe(0)
    expect(lastUpdatePayload()?.words).toEqual([])
  })

  it('falls back to the plain text when there is no texts array', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = [{ time: 0, text: 'fallback line' }] as unknown as LyricLine[]
    player().currentLyricIndex = 0
    await mountDesktopLyrics()

    await tick()

    expect(lastUpdatePayload()).toMatchObject({ currentLine: 'fallback line', subLine: '' })
  })

  it('ignores malformed karaoke words', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = [
      {
        time: 0,
        text: 'x',
        texts: ['x'],
        words: [
          { text: '', start: 0, end: 1 },
          { text: 'bad', start: 2, end: 2 },
          { text: 'nan', start: Number.NaN, end: 3 },
        ],
      },
    ] as unknown as LyricLine[]
    player().currentLyricIndex = 0
    player().currentTime = 0.5
    await mountDesktopLyrics()

    await tick()

    // 空文本 / end <= start / 非有限时间戳全部被过滤
    expect(lastUpdatePayload()?.words).toEqual([])
    expect(lastUpdatePayload()?.progress).toBe(0)
  })

  it('sends empty strings when nothing is playing', async () => {
    config().lyrics.desktopLyrics.enabled = true
    await mountDesktopLyrics()

    await tick()

    expect(lastUpdatePayload()).toMatchObject({ currentLine: '', subLine: '', progress: 0 })
  })

  it('applies the lyric offset to the reported time', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 5
    player().lyricsOffset = 2
    await mountDesktopLyrics()

    await tick()

    expect(lastUpdatePayload()?.currentTime).toBe(3)
  })

  it('skips repeated updates while the state is unchanged', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 0.25
    await mountDesktopLyrics()

    await tick()
    const afterFirst = invokeCalls('update_desktop_lyric').length

    await tick()
    await tick()

    expect(invokeCalls('update_desktop_lyric')).toHaveLength(afterFirst)
  })

  it('re-sends when the lyric index changes', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = [
      { time: 0, text: 'first', texts: ['first'] },
      { time: 1, text: 'second', texts: ['second'] },
    ] as unknown as LyricLine[]
    player().currentLyricIndex = 0
    await mountDesktopLyrics()
    await tick()
    expect(lastUpdatePayload()?.currentLine).toBe('first')

    player().currentLyricIndex = 1
    await nextTick()
    await flushFrames()

    expect(lastUpdatePayload()?.currentLine).toBe('second')
  })

  it('re-sends after the track changes', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 0.75
    await mountDesktopLyrics()
    await tick()
    const before = invokeCalls('update_desktop_lyric').length

    player().currentTrack = { path: '/next.flac' }
    await nextTick()
    await flushFrames()

    expect(invokeCalls('update_desktop_lyric').length).toBeGreaterThan(before)
  })

  it('re-sends after the lyrics offset changes', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 1.25
    await mountDesktopLyrics()
    await tick()
    const before = invokeCalls('update_desktop_lyric').length

    player().lyricsOffset = 0.5
    await nextTick()
    await flushFrames()

    expect(invokeCalls('update_desktop_lyric').length).toBeGreaterThan(before)
  })

  it('re-sends when the lyrics array is replaced', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    await mountDesktopLyrics()
    await tick()
    const before = invokeCalls('update_desktop_lyric').length

    player().lyrics = [{ time: 0, text: 'new', texts: ['new'] }] as unknown as LyricLine[]
    await nextTick()
    await flushFrames()

    expect(invokeCalls('update_desktop_lyric').length).toBeGreaterThan(before)
    expect(lastUpdatePayload()?.currentLine).toBe('new')
  })

  it('reports the failure through the error handler when the update fails', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    mockInvoke.mockRejectedValue(new Error('window gone'))
    await mountDesktopLyrics()

    await tick()

    expect(errorApi.handle).toHaveBeenCalledWith(expect.any(Error), {
      severity: 'low',
      showToUser: false,
    })
  })
})

describe('useDesktopLyrics > settings sync', () => {
  it('shows and hides the window when the toggle flips', async () => {
    await mountDesktopLyrics()

    config().lyrics.desktopLyrics.enabled = true
    await nextTick()
    expect(mockInvoke).toHaveBeenCalledWith('show_desktop_lyrics')

    config().lyrics.desktopLyrics.enabled = false
    await nextTick()
    expect(mockInvoke).toHaveBeenCalledWith('hide_desktop_lyrics')
  })

  it('syncs the lock state when it changes', async () => {
    await mountDesktopLyrics()

    config().lyrics.desktopLyrics.locked = false
    await nextTick()

    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_locked', { locked: false })
  })

  it('syncs the font size when it changes', async () => {
    await mountDesktopLyrics()

    config().lyrics.desktopLyrics.fontSize = 44
    await nextTick()

    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_font_size', { size: 44 })
  })

  it('syncs the font family when it changes', async () => {
    await mountDesktopLyrics()

    config().lyrics.lyricsFontFamily = 'LXGW WenKai'
    await nextTick()

    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_font_family', {
      fontFamily: 'LXGW WenKai',
      translationFontFamily: '',
    })
  })

  it('syncs the translation font family too', async () => {
    await mountDesktopLyrics()

    config().lyrics.translationFontFamily = 'Roboto'
    await nextTick()

    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_font_family', {
      fontFamily: 'Noto Sans SC',
      translationFontFamily: 'Roboto',
    })
  })

  it('syncs the color preset when it changes', async () => {
    await mountDesktopLyrics()

    config().lyrics.desktopLyrics.colorPreset = 'light'
    await nextTick()

    expect(mockInvoke).toHaveBeenCalledWith('set_desktop_lyrics_color_preset', { preset: 'light' })
  })

  it('silently degrades when a sync command is rejected', async () => {
    await mountDesktopLyrics()
    mockInvoke.mockRejectedValue(new Error('unsupported'))

    config().lyrics.desktopLyrics.fontSize = 40
    await nextTick()

    expect(errorApi.handle).toHaveBeenCalledWith(expect.any(Error), {
      severity: 'low',
      showToUser: false,
    })
  })

  it('logs but does not throw when hiding the window fails', async () => {
    config().lyrics.desktopLyrics.enabled = true
    await mountDesktopLyrics()
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === 'hide_desktop_lyrics'
        ? Promise.reject(new Error('nope'))
        : Promise.resolve(undefined),
    )

    config().lyrics.desktopLyrics.enabled = false
    await nextTick()

    expect(loggerApi.error).toHaveBeenCalledWith(
      'Failed to hide desktop lyrics:',
      expect.any(Error),
    )
  })
})

describe('useDesktopLyrics > window events', () => {
  it('disables desktop lyrics when the window is closed by the user', async () => {
    await mountDesktopLyrics()

    eventHandlers['desktop-lyrics-closed']?.({ payload: undefined })

    expect(config().setDesktopLyricsConfig).toHaveBeenCalledWith({ enabled: false })
    expect(loggerApi.info).toHaveBeenCalledWith('Desktop lyrics closed from window button')
  })

  it('records the lock state pushed from the desktop window', async () => {
    await mountDesktopLyrics()

    eventHandlers['desktop-lyrics-lock-changed']?.({ payload: false })

    expect(config().setDesktopLyricsConfig).toHaveBeenCalledWith({ locked: false })
  })
})

describe('useDesktopLyrics > lifecycle', () => {
  const trackListeners = () => {
    const listeners: Array<ReturnType<typeof vi.fn>> = []
    eventApi.listen.mockImplementation((name: string, handler: EventHandler) => {
      eventHandlers[name] = handler
      const unlisten = vi.fn()
      listeners.push(unlisten)
      return Promise.resolve(unlisten)
    })
    return listeners
  }

  it('keeps the shared listeners alive while another component still uses them', async () => {
    const listeners = trackListeners()
    const first = await mountDesktopLyrics()
    const second = await mountDesktopLyrics()

    first.unmount()
    await settle()

    // 仍有组件在使用,监听不应被取消
    expect(listeners[0]).not.toHaveBeenCalled()

    second.unmount()
    await settle()
    expect(listeners[0]).toHaveBeenCalled()
  })

  it('unlistens when the last component unmounts', async () => {
    const listeners = trackListeners()
    const wrapper = await mountDesktopLyrics()

    wrapper.unmount()
    await settle()

    expect(listeners).toHaveLength(2)
    expect(listeners[0]).toHaveBeenCalled()
    expect(listeners[1]).toHaveBeenCalled()
  })

  it('stops sending updates after unmount', async () => {
    config().lyrics.desktopLyrics.enabled = true
    player().lyrics = karaokeLyrics()
    player().currentLyricIndex = 0
    player().currentTime = 2.5
    const wrapper = await mountDesktopLyrics()
    await tick()

    wrapper.unmount()
    await settle()
    const before = invokeCalls('update_desktop_lyric').length

    await vi.advanceTimersByTimeAsync(POLL_INTERVAL_MS * 5)
    await flushFrames()

    expect(invokeCalls('update_desktop_lyric')).toHaveLength(before)
  })

  it('re-initialises the listeners when a new component mounts later', async () => {
    const first = await mountDesktopLyrics()
    first.unmount()
    await settle()

    eventApi.listen.mockClear()
    await mountDesktopLyrics()

    expect(eventApi.listen).toHaveBeenCalledWith('desktop-lyrics-closed', expect.any(Function))
    expect(eventApi.listen).toHaveBeenCalledWith(
      'desktop-lyrics-lock-changed',
      expect.any(Function),
    )
  })

  it('immediately unlistens a listener that resolves after unmount', async () => {
    let resolveListen: (fn: () => void) => void = () => {}
    eventApi.listen.mockImplementation((name: string) => {
      if (name !== 'desktop-lyrics-closed') return Promise.resolve(vi.fn())
      return new Promise<() => void>((resolve) => {
        resolveListen = resolve
      })
    })

    const wrapper = await mountDesktopLyrics()
    wrapper.unmount()
    await settle()

    const lateUnlisten = vi.fn()
    resolveListen(lateUnlisten)
    await settle()

    // 卸载后才 resolve 的注册必须被立刻撤销
    expect(lateUnlisten).toHaveBeenCalled()
  })
})
