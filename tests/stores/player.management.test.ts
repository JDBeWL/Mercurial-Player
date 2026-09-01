// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: vi.fn(),
  unregisterAll: vi.fn(),
  isRegistered: vi.fn(async () => false),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: {
    fileExists: vi.fn(async () => true),
    findLyricsFile: vi.fn(async () => null),
    readFile: vi.fn(async () => ''),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileNameWithoutExtension: vi.fn((p: string) =>
      (p.split(/[\\/]/).pop() || p).replace(/\.[^.]+$/, ''),
    ),
    getFileExtension: vi.fn(() => 'lrc'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  LyricsParser: {
    parseAsync: vi.fn(async () => [{ time: 1, text: 'hello' }]),
  },
}))

const configState = vi.hoisted(() => ({
  audio: { volume: 0.5, exclusiveMode: false, fadeEnabled: true },
  saveConfig: vi.fn(),
  saveConfigNow: vi.fn(),
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => configState),
}))

vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: vi.fn(() => ({
    currentPlaylist: null,
    playlists: [],
  })),
}))

import { setActivePinia, createPinia } from 'pinia'
import { usePlayerStore } from '@/stores/player'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import FileUtils from '@/utils/fileUtils'
import logger from '@/utils/logger'
import errorHandler, { ErrorSeverity } from '@/utils/errorHandler'
import type { Track } from '@/types'

const invokeMock = vi.mocked(invoke)
const listenMock = vi.mocked(listen)
const loggerMock = vi.mocked(logger)
const findLyricsFileMock = vi.mocked(FileUtils.findLyricsFile)

function makeTrack(path: string, name?: string): Track {
  return { path, name: name ?? path, title: name, duration: 100 }
}

function makePlaylist(n: number): Track[] {
  return Array.from({ length: n }, (_, i) => makeTrack(`/music/track${i}.mp3`, `Track${i}`))
}

type Store = ReturnType<typeof usePlayerStore>

let unlistenSpies: Array<ReturnType<typeof vi.fn>>

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  invokeMock.mockResolvedValue(undefined)
  configState.audio = { volume: 0.5, exclusiveMode: false, fadeEnabled: true }
  configState.saveConfig.mockResolvedValue(undefined)
  configState.saveConfigNow.mockResolvedValue(undefined)

  // 每个 listen 注册都返回一个可断言的 unlisten
  unlistenSpies = []
  listenMock.mockImplementation(() => {
    const unlisten = vi.fn()
    unlistenSpies.push(unlisten)
    return Promise.resolve(unlisten)
  })
})

afterEach(() => {
  vi.useRealTimers()
})

/** 初始化 store(注册监听器),返回已初始化的实例 */
async function initStore(): Promise<Store> {
  const store = usePlayerStore()
  await store.initAudio()
  return store
}

describe('player.management > initAudio', () => {
  it('applies the saved volume from the config', async () => {
    const store = await initStore()

    expect(store.volume).toBe(0.5)
    expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0.5 })
  })

  it('skips an out-of-range saved volume', async () => {
    configState.audio = { volume: 1.5, exclusiveMode: false, fadeEnabled: true }
    const store = await initStore()

    expect(store.volume).toBe(1)
    expect(invokeMock).not.toHaveBeenCalledWith('set_volume', expect.anything())
  })

  it('registers every event listener', async () => {
    const store = await initStore()

    expect(store._trackEndedUnlisten).not.toBeNull()
    expect(store._positionUnlisten).not.toBeNull()
    expect(store._taskbarPreviousUnlisten).not.toBeNull()
    expect(store._taskbarPlayPauseUnlisten).not.toBeNull()
    expect(store._taskbarNextUnlisten).not.toBeNull()
    expect(store._deviceRemovedUnlisten).not.toBeNull()
    expect(store._deviceSwitchRequiredUnlisten).not.toBeNull()
    expect(store._noDeviceAvailableUnlisten).not.toBeNull()
    expect(store._deviceDefaultChangedUnlisten).not.toBeNull()
  })

  it('warns and skips a duplicate initAudio call', async () => {
    const store = await initStore()
    const volumeCalls = invokeMock.mock.calls.filter(([cmd]) => cmd === 'set_volume').length

    await store.initAudio()

    expect(loggerMock.warn).toHaveBeenCalledWith(
      'Player store already initialized, skipping duplicate initAudio call',
    )
    // 不重复设置音量/监听器
    expect(invokeMock.mock.calls.filter(([cmd]) => cmd === 'set_volume')).toHaveLength(volumeCalls)
  })

  it('coalesces concurrent initAudio calls into a single setup', async () => {
    const store = usePlayerStore()

    const first = store.initAudio()
    const second = store.initAudio()
    await Promise.all([first, second])

    expect(invokeMock.mock.calls.filter(([cmd]) => cmd === 'set_volume')).toHaveLength(1)
    expect(listenMock.mock.calls.length).toBeGreaterThan(0)
    // 重复的注册数量应与单次一致
    const listenCount = listenMock.mock.calls.length
    await store.initAudio()
    expect(listenMock.mock.calls.length).toBe(listenCount)
  })

  it('continues when the saved volume cannot be read', async () => {
    const store = usePlayerStore()
    const brokenAudio = {
      get audio(): unknown {
        throw new Error('config corrupted')
      },
    }
    vi.mocked((await import('@/stores/config')).useConfigStore).mockReturnValueOnce(
      brokenAudio as never,
    )

    await store.initAudio()

    expect(loggerMock.error).toHaveBeenCalledWith(
      'Failed to load volume from config:',
      expect.any(Error),
    )
    // 监听器仍被注册
    expect(store._trackEndedUnlisten).not.toBeNull()
  })
})

describe('player.management > _switchAudioDevice', () => {
  it('does nothing when the store is destroyed', async () => {
    const store = usePlayerStore()
    store._isDestroyed = true

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).not.toHaveBeenCalledWith('set_audio_device', expect.anything())
  })

  it('does nothing for an empty device name', async () => {
    const store = usePlayerStore()

    await store._switchAudioDevice(
      '',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).not.toHaveBeenCalledWith('set_audio_device', expect.anything())
  })

  it('switches the device and reports success', async () => {
    const handle = vi.spyOn(errorHandler, 'handle')
    const store = usePlayerStore()

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      '已切换到 Speakers',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).toHaveBeenCalledWith('set_audio_device', {
      deviceName: 'Speakers',
      currentTime: 0,
    })
    expect(handle).toHaveBeenCalledWith(
      expect.any(Error),
      expect.objectContaining({
        showToUser: true,
        userMessage: '已切换到 Speakers',
        context: expect.objectContaining({ deviceName: 'Speakers' }),
      }),
    )
    handle.mockRestore()
  })

  it('re-pauses after switching while the track is paused', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    store.isPlaying = false

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).toHaveBeenCalledWith('pause_track')
    expect(store.isPlaying).toBe(false)
  })

  it('does not re-pause after switching while playing', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    store.isPlaying = true

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).not.toHaveBeenCalledWith('pause_track')
  })

  it('reports a friendly message when the switch fails', async () => {
    const handle = vi.spyOn(errorHandler, 'handle')
    const store = usePlayerStore()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'set_audio_device') return Promise.reject(new Error('device busy'))
      return Promise.resolve(undefined)
    })

    await store._switchAudioDevice(
      'Speakers',
      'switch-fallback-success',
      'ok',
      'switch-fallback',
      ErrorSeverity.HIGH,
    )

    expect(handle).toHaveBeenCalledWith(
      expect.any(Error),
      expect.objectContaining({
        showToUser: true,
        severity: ErrorSeverity.HIGH,
        context: expect.objectContaining({ action: 'switch-fallback' }),
      }),
    )
    handle.mockRestore()
  })

  it('ignores a duplicated switch for the same target', async () => {
    const store = usePlayerStore()
    store._isSwitchingDevice = true
    store._lastDeviceSwitchTarget = 'Speakers'

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).not.toHaveBeenCalledWith('set_audio_device', expect.anything())
    expect(loggerMock.debug).toHaveBeenCalledWith(
      expect.stringContaining('Skip duplicated device switch'),
    )
  })

  it('warns when another switch is already running', async () => {
    const store = usePlayerStore()
    store._isSwitchingDevice = true
    store._lastDeviceSwitchTarget = 'Headphones'

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(invokeMock).not.toHaveBeenCalledWith('set_audio_device', expect.anything())
    expect(loggerMock.warn).toHaveBeenCalledWith(
      expect.stringContaining('Device switch ignored while another switch is running'),
    )
  })

  it('resets the switching flags after completion', async () => {
    const store = usePlayerStore()

    await store._switchAudioDevice(
      'Speakers',
      'switch-default-success',
      'ok',
      'switch-default',
      ErrorSeverity.HIGH,
    )

    expect(store._isSwitchingDevice).toBe(false)
    expect(store._lastDeviceSwitchTarget).toBeNull()
  })
})

describe('player.management > cleanup', () => {
  it('saves the last session before shutting down', async () => {
    const store = await initStore()
    store.currentTrack = makeTrack('/music/a.mp3')

    await store.cleanup()

    expect(invokeMock).toHaveBeenCalledWith('save_last_session', expect.anything())
  })

  it('continues the cleanup when saving the session fails', async () => {
    const store = await initStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'save_last_session') return Promise.reject(new Error('disk full'))
      return Promise.resolve(undefined)
    })

    await store.cleanup()

    // saveLastSessionNow 内部自行捕获错误并记录日志
    expect(loggerMock.debug).toHaveBeenCalledWith('Failed to save last session:', expect.any(Error))
    expect(store._isDestroyed).toBe(true)
  })

  it('does not save the session when nothing was playing', async () => {
    const store = await initStore()

    await store.cleanup()

    expect(invokeMock).not.toHaveBeenCalledWith('save_last_session', expect.anything())
  })

  it('unlistens every registered listener', async () => {
    const store = await initStore()
    expect(unlistenSpies.length).toBeGreaterThan(0)

    await store.cleanup()

    for (const unlisten of unlistenSpies) {
      expect(unlisten).toHaveBeenCalled()
    }
    expect(store._trackEndedUnlisten).toBeNull()
    expect(store._positionUnlisten).toBeNull()
    expect(store._deviceDefaultChangedUnlisten).toBeNull()
  })

  it('stops playback and updates the taskbar', async () => {
    const store = await initStore()

    await store.cleanup()

    expect(invokeMock).toHaveBeenCalledWith('pause_track')
    expect(invokeMock).toHaveBeenCalledWith('set_taskbar_stopped')
  })

  it('clears a pending auto-next-track timer', async () => {
    vi.useFakeTimers()
    const store = await initStore()
    store._nextTrackTimeoutId = setTimeout(() => {}, 10_000)
    expect(vi.getTimerCount()).toBeGreaterThan(0)

    await store.cleanup()

    expect(store._nextTrackTimeoutId).toBeNull()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('aborts an in-flight metadata caching task', async () => {
    const store = await initStore()
    const controller = new AbortController()
    store._cacheAbortController = controller

    await store.cleanup()

    expect(controller.signal.aborted).toBe(true)
    expect(store._cacheAbortController).toBeNull()
  })

  it('tolerates a taskbar update failure', async () => {
    const store = await initStore()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'set_taskbar_stopped') return Promise.reject(new Error('not windows'))
      return Promise.resolve(undefined)
    })

    await expect(store.cleanup()).resolves.toBeUndefined()
    expect(store._isDestroyed).toBe(true)
  })

  it('invalidates in-flight play and lyrics requests', async () => {
    const store = await initStore()
    const lyricsId = store.beginLyricsRequest()
    expect(store.isLyricsRequestCurrent(lyricsId)).toBe(true)

    await store.cleanup()

    expect(store.isLyricsRequestCurrent(lyricsId)).toBe(false)
  })
})

describe('player.management > resetPlayerState', () => {
  it('clears every piece of playback state', async () => {
    const store = usePlayerStore()
    store.playlist = makePlaylist(3)
    store.currentTrack = store.playlist[1]!
    store.isPlaying = true
    store.currentTime = 42
    store.duration = 100
    store.lyrics = [{ time: 0, text: 'x' }] as never
    store.currentLyricIndex = 0

    await store.resetPlayerState()

    expect(store.isPlaying).toBe(false)
    expect(store.currentTrack).toBeNull()
    expect(store.playlist).toEqual([])
    expect(store.currentTime).toBe(0)
    expect(store.duration).toBe(0)
    expect(store.lyrics).toBeNull()
    expect(store.currentLyricIndex).toBe(-1)
  })

  it('keeps the playlist when clearPlaylist is false', async () => {
    const store = usePlayerStore()
    store.playlist = makePlaylist(3)
    store.currentTrack = store.playlist[1]!

    await store.resetPlayerState(false)

    expect(store.playlist).toHaveLength(3)
    expect(store.currentTrack).toBeNull()
  })

  it('destroys the cache manager and aborts pending work', async () => {
    const store = usePlayerStore()
    store._getCacheManager() // 触发缓存管理器创建
    const controller = new AbortController()
    store._cacheAbortController = controller

    await store.resetPlayerState()

    expect(store._cacheManager).toBeNull()
    expect(controller.signal.aborted).toBe(true)
    expect(store._cacheAbortController).toBeNull()
  })

  it('stops the backend and logs a stop failure', async () => {
    const store = usePlayerStore()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'pause_track') return Promise.reject(new Error('backend gone'))
      return Promise.resolve(undefined)
    })

    await store.resetPlayerState()

    expect(loggerMock.error).toHaveBeenCalledWith(
      'Error stopping backend playback:',
      expect.any(Error),
    )
  })
})

describe('player.management > lyrics loading', () => {
  it('loads and parses the sidecar lyrics file', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.currentTrack = track
    findLyricsFileMock.mockResolvedValue('/music/a.lrc')

    await store.loadLyrics('/music/a.mp3')

    expect(findLyricsFileMock).toHaveBeenCalledWith('/music/a.mp3')
    expect(FileUtils.readFile).toHaveBeenCalledWith('/music/a.lrc')
    expect(store.lyrics).toEqual([{ time: 1, text: 'hello' }])
  })

  it('clears the lyrics when no sidecar file exists', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.currentTrack = track
    store.lyrics = [{ time: 0, text: 'stale' }] as never
    findLyricsFileMock.mockResolvedValue(null)

    await store.loadLyrics('/music/a.mp3')

    expect(store.lyrics).toBeNull()
  })

  it('ignores the request for a different track', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    findLyricsFileMock.mockResolvedValue('/music/b.lrc')

    await store.loadLyrics('/music/b.mp3')

    expect(store.lyrics).toBeNull()
    expect(FileUtils.readFile).not.toHaveBeenCalled()
  })

  it('discards a stale request superseded by a newer one', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')

    const resolvers: Array<(value: string | null) => void> = []
    findLyricsFileMock.mockImplementation(() => new Promise((resolve) => resolvers.push(resolve)))

    const first = store.loadLyrics('/music/a.mp3')
    const second = store.loadLyrics('/music/a.mp3')

    // 后一个请求先返回,先到的必须被作废
    resolvers[1]!('/music/a.lrc')
    await second
    const lyricsAfterSecond = store.lyrics

    resolvers[0]!('/music/a.lrc')
    await first

    expect(store.lyrics).toBe(lyricsAfterSecond)
    // 解析只应为有效的那个请求执行一次
    expect((FileUtils.readFile as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1)
  })

  it('clears the lyrics when parsing fails', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    store.lyrics = [{ time: 0, text: 'stale' }] as never
    findLyricsFileMock.mockRejectedValue(new Error('fs error'))

    await store.loadLyrics('/music/a.mp3')

    expect(store.lyrics).toBeNull()
    expect(loggerMock.debug).toHaveBeenCalledWith(
      'No lyrics found or failed to load:',
      expect.any(Error),
    )
  })

  it('keeps existing lyrics when the failure belongs to another track', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/a.mp3')
    store.lyrics = [{ time: 0, text: 'current' }] as never
    const before = store.lyrics
    findLyricsFileMock.mockRejectedValue(new Error('fs error'))

    await store.loadLyrics('/music/b.mp3')

    expect(store.lyrics).toBe(before)
    expect(store.lyrics).toEqual([{ time: 0, text: 'current' }])
  })
})

describe('player.management > lyrics request guard', () => {
  it('issues increasing ids and validates only the latest', () => {
    const store = usePlayerStore()

    const first = store.beginLyricsRequest()
    expect(store.isLyricsRequestCurrent(first)).toBe(true)

    const second = store.beginLyricsRequest()
    expect(second).toBe(first + 1)
    expect(store.isLyricsRequestCurrent(first)).toBe(false)
    expect(store.isLyricsRequestCurrent(second)).toBe(true)
  })

  it('rejects every id after the store is destroyed', () => {
    const store = usePlayerStore()
    const id = store.beginLyricsRequest()

    store._isDestroyed = true

    expect(store.isLyricsRequestCurrent(id)).toBe(false)
  })
})

describe('player.management > cover updates', () => {
  it('records a cover update and bumps the version', () => {
    const store = usePlayerStore()
    const versionBefore = store.playlistCoverVersion

    store.recordCoverUpdate('/music/a.mp3', '/covers/a.jpg')

    expect(store.playlistCoverVersion).toBe(versionBefore + 1)
  })

  it('hands out accumulated updates and clears the queue', () => {
    const store = usePlayerStore()

    store.recordCoverUpdate('/music/a.mp3', '/covers/a.jpg')
    store.recordCoverUpdate('/music/b.mp3', '/covers/b.jpg')

    const updates = store.takeCoverUpdates()
    expect(updates.get('/music/a.mp3')).toBe('/covers/a.jpg')
    expect(updates.get('/music/b.mp3')).toBe('/covers/b.jpg')

    expect(store.takeCoverUpdates().size).toBe(0)
  })
})

describe('player.management > loadPlaylist', () => {
  it('aborts the previous caching task when a new playlist arrives', () => {
    const store = usePlayerStore()

    store.loadPlaylist(makePlaylist(5))
    const first = store._cacheAbortController
    expect(first).not.toBeNull()

    store.loadPlaylist(makePlaylist(3))

    expect(first!.signal.aborted).toBe(true)
  })

  it('sets the first track as current', () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)

    store.loadPlaylist(tracks)

    expect(store.currentTrack?.path).toBe('/music/track0.mp3')
    expect(store.duration).toBe(100)
    expect(store.audioInfo.format).toBeNull()
  })

  it('clears the current track for an empty playlist', () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/old.mp3')
    store.isPlaying = true

    store.loadPlaylist([])

    expect(store.currentTrack).toBeNull()
    expect(store.isPlaying).toBe(false)
    expect(store.duration).toBe(0)
  })

  it('invalidates the shuffle order and history', () => {
    const store = usePlayerStore()
    store._shuffleOrder = [0, 1]
    store._shufflePosition = 1
    store._shuffleHistory = [0]

    store.loadPlaylist(makePlaylist(3))

    expect(store._shuffleOrder).toEqual([])
    expect(store._shufflePosition).toBe(-1)
    expect(store._shuffleHistory).toEqual([])
  })
})
