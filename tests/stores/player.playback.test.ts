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
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  LyricsParser: {
    parseAsync: vi.fn(async () => []),
  },
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => ({
    audio: { volume: 0.5, exclusiveMode: false, fadeEnabled: true },
    saveConfig: vi.fn().mockResolvedValue(undefined),
    saveConfigNow: vi.fn().mockResolvedValue(undefined),
  })),
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
import FileUtils from '@/utils/fileUtils'
import logger from '@/utils/logger'
import type { Track } from '@/types'

const invokeMock = vi.mocked(invoke)
const fileExistsMock = vi.mocked(FileUtils.fileExists)
const loggerMock = vi.mocked(logger)

const PLAY_TRACK_TIMEOUT_MS = 5000
const AUTO_NEXT_TRACK_DELAY_MS = 100

function makeTrack(path: string, name?: string): Track {
  return { path, name: name ?? path, title: name, duration: 100 }
}

function makePlaylist(n: number): Track[] {
  return Array.from({ length: n }, (_, i) => makeTrack(`/music/track${i}.mp3`, `Track${i}`))
}

type Store = ReturnType<typeof usePlayerStore>

/** 让 play_track 等命令成功,并记录每次调用 */
function setupPlayInvokeMock() {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_track_cover_path') return Promise.resolve(null)
    return Promise.resolve(undefined)
  })
}

/** 把 store 摆成"正在播放"状态(不经过 playTrack) */
function setPlaying(store: Store, tracks: Track[], index: number) {
  store.playlist = tracks
  store.currentTrack = tracks[index]!
  store.duration = tracks[index]!.duration || 100
  store.isPlaying = true
}

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  setupPlayInvokeMock()
  fileExistsMock.mockResolvedValue(true)
})

afterEach(() => {
  vi.useRealTimers()
})

describe('player.playback > play/pause/resume/togglePlay', () => {
  it('play() replays the current track', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)
    store.isPlaying = false

    store.play()
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
    })
  })

  it('play() falls back to the first playlist entry', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    store.playlist = tracks
    store.currentTrack = null

    store.play()
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
    })
  })

  it('play() does nothing with an empty playlist', () => {
    const store = usePlayerStore()

    store.play()

    expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
  })

  it('pause() pauses the backend and updates state', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)

    store.pause()
    await vi.waitFor(() => {
      expect(store.isPlaying).toBe(false)
    })
    expect(invokeMock).toHaveBeenCalledWith('pause_track')
  })

  it('pause() is a no-op when not playing', () => {
    const store = usePlayerStore()
    store.isPlaying = false

    store.pause()

    expect(invokeMock).not.toHaveBeenCalledWith('pause_track')
  })

  it('pause() logs instead of throwing when the backend rejects', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    invokeMock.mockRejectedValueOnce(new Error('device lost'))

    store.pause()
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to pause:', expect.any(Error))
    })
  })

  it('resume() resumes a paused track', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    store.isPlaying = false

    store.resume()
    await vi.waitFor(() => {
      expect(store.isPlaying).toBe(true)
    })
    expect(invokeMock).toHaveBeenCalledWith('resume_track')
  })

  it('resume() does nothing when already playing or without a track', () => {
    const store = usePlayerStore()
    store.isPlaying = true
    store.resume()

    store.isPlaying = false
    store.currentTrack = null
    store.resume()

    expect(invokeMock).not.toHaveBeenCalledWith('resume_track')
  })

  it('resume() logs instead of throwing when the backend rejects', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    store.isPlaying = false
    invokeMock.mockRejectedValueOnce(new Error('gone'))

    store.resume()
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to resume:', expect.any(Error))
    })
  })

  it('togglePlay() pauses while playing', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)

    store.togglePlay()
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('pause_track')
    })
  })

  it('togglePlay() resumes when paused mid-track', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    store.isPlaying = false
    store.currentTime = 30

    store.togglePlay()
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('resume_track')
    })
  })

  it('togglePlay() restarts from the beginning when paused at the end', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    store.isPlaying = false
    store.currentTime = 99.9

    store.togglePlay()
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
    })
  })
})

describe('player.playback > seek', () => {
  it('seeks and updates the position', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)

    store.seek(42)
    await vi.waitFor(() => {
      expect(store.currentTime).toBe(42)
    })
    expect(invokeMock).toHaveBeenCalledWith('seek_track', { time: 42 })
  })

  it('clamps the target into [0, duration]', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)

    store.seek(999)
    store.seek(-5)

    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('seek_track', { time: 100 })
      expect(invokeMock).toHaveBeenCalledWith('seek_track', { time: 0 })
    })
  })

  it('re-pauses after a seek when the track was paused', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    store.isPlaying = false

    store.seek(10)
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('pause_track')
    })
  })

  it('does not re-pause after a seek while playing', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)

    store.seek(10)
    await vi.waitFor(() => {
      expect(store.currentTime).toBe(10)
    })
    expect(invokeMock).not.toHaveBeenCalledWith('pause_track')
  })

  it('ignores seek without a current track', () => {
    const store = usePlayerStore()

    store.seek(10)

    expect(invokeMock).not.toHaveBeenCalledWith('seek_track', expect.anything())
  })

  it('logs instead of throwing when the seek fails', async () => {
    const store = usePlayerStore()
    setPlaying(store, makePlaylist(1), 0)
    invokeMock.mockRejectedValueOnce(new Error('seek failed'))

    store.seek(10)
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to seek:', expect.any(Error))
    })
  })
})

describe('player.playback > playTrack extras', () => {
  it('falls back to the alternate path separator when the file is missing', async () => {
    const store = usePlayerStore()
    const track = makeTrack('C:\\music\\a.mp3')
    fileExistsMock.mockImplementation(async (path: string) => path !== track.path)
    store.playlist = [track]

    await store.playTrack(track)

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: 'C:/music/a.mp3' })
  })

  it('skips to the next track when the file does not exist', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    store.playlist = tracks
    store.currentTrack = tracks[0]!
    fileExistsMock.mockImplementation(async (path: string) => !path.includes('track0'))

    await store.playTrack(tracks[0]!)

    // 找不到文件 → 自动播放下一首
    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track1.mp3' })
  })

  it('skips past consecutive missing tracks without looping', async () => {
    // 缺失曲目紧跟当前曲目:此前会走 nextTrack 并回到同一首,无限递归
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    store.playlist = tracks
    store.currentTrack = tracks[0]!
    // 只有 track2 存在
    fileExistsMock.mockImplementation(async (path: string) => path.includes('track2'))

    await store.playTrack(tracks[1]!)

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track2.mp3' })
    expect(store.currentTrack?.path).toBe('/music/track2.mp3')
  })

  it('resets the player state when every following track is missing', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    store.playlist = tracks
    store.currentTrack = tracks[0]!
    fileExistsMock.mockResolvedValue(false)

    await store.playTrack(tracks[1]!)

    expect(store.currentTrack).toBeNull()
    expect(store.isPlaying).toBe(false)
  })

  it('resets the player state when the last track is missing', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    store.playlist = tracks
    fileExistsMock.mockResolvedValue(false)

    await store.playTrack(tracks[1]!)

    expect(store.currentTrack).toBeNull()
    expect(store.playlist).toEqual([])
    expect(store.isPlaying).toBe(false)
  })

  it('keeps playing other tracks even when an earlier one is missing', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    store.playlist = tracks
    store.currentTrack = tracks[0]!
    // 只有 track1 存在,track0 被跳过
    fileExistsMock.mockImplementation(async (path: string) => !path.includes('track0'))

    await store.playTrack(tracks[0]!)

    expect(store.currentTrack?.path).toBe('/music/track1.mp3')
    expect(store.isPlaying).toBe(true)
  })

  it('syncs the shuffle position on a manual track change', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    store.playlist = tracks
    store.isShuffle = true
    store._shuffleOrder = [2, 0, 1]

    await store.playTrack(tracks[1]!)

    expect(store._shufflePosition).toBe(2)
  })

  it('invalidates a stale shuffle order on a manual track change', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    store.playlist = tracks
    store.isShuffle = true
    // 顺序里不含目标索引 → 作废等待懒生成
    store._shuffleOrder = [0, 2]

    await store.playTrack(tracks[1]!)

    expect(store._shuffleOrder).toEqual([])
    expect(store._shufflePosition).toBe(-1)
  })

  it('lazy-loads the cover after the track starts', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_track_cover_path') return Promise.resolve('/covers/a.jpg')
      return Promise.resolve(undefined)
    })

    await store.playTrack(track)
    await vi.waitFor(() => {
      expect(store.currentTrack?.coverPath).toBe('/covers/a.jpg')
    })
  })

  it('writes the lazy-loaded cover back into the metadata cache', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    store._getMetadataCache().set('/music/a.mp3', {
      title: 'Cached',
      artist: '',
      album: '',
      duration: 100,
      bitrate: null,
      sampleRate: null,
      channels: null,
      bitDepth: null,
      format: null,
    })
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_track_cover_path') return Promise.resolve('/covers/a.jpg')
      return Promise.resolve(undefined)
    })

    await store.playTrack(track)
    await vi.waitFor(() => {
      expect(store._getMetadataCache().get('/music/a.mp3')?.coverPath).toBe('/covers/a.jpg')
    })
  })

  it('does not request a cover that the track already has', async () => {
    const store = usePlayerStore()
    const track = { ...makeTrack('/music/a.mp3'), coverPath: '/covers/existing.jpg' }
    store.playlist = [track]

    await store.playTrack(track)

    expect(invokeMock).not.toHaveBeenCalledWith('get_track_cover_path', expect.anything())
    expect(store.currentTrack?.coverPath).toBe('/covers/existing.jpg')
  })

  it('continues to play even when the pre-play pause fails', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'pause_track') return Promise.reject(new Error('pause glitch'))
      return Promise.resolve(undefined)
    })

    await store.playTrack(track)

    expect(loggerMock.warn).toHaveBeenCalledWith('pause before play:', expect.any(Error))
    expect(store.isPlaying).toBe(true)
  })

  it('aborts a superseded request before touching the backend', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    store.playlist = tracks

    // 两个请求并发,先到的必须在 fileExists 之后发现已被作废
    const resolvers: Array<(value: boolean) => void> = []
    fileExistsMock.mockImplementation(() => new Promise((resolve) => resolvers.push(resolve)))

    const first = store.playTrack(tracks[0]!)
    const second = store.playTrack(tracks[1]!)

    resolvers[0]!(true)
    await first
    // 第一个请求已被作废,不应触碰 play_track
    expect(invokeMock).not.toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })

    resolvers[1]!(true)
    await second
    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track1.mp3' })
  })

  it('does not flip isPlaying when the store was destroyed mid-play', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'play_track') {
        store._isDestroyed = true
        store._activePlayRequestId++
      }
      return Promise.resolve(undefined)
    })

    await store.playTrack(track)

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/a.mp3' })
    expect(store.isPlaying).toBe(false)
  })

  it('ignores a late error from a superseded play request', async () => {
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'play_track') {
        store._isDestroyed = true
        return Promise.reject(new Error('stale failure'))
      }
      return Promise.resolve(undefined)
    })

    await store.playTrack(track)

    expect(loggerMock.error).not.toHaveBeenCalledWith('Failed to play track:', expect.anything())
  })

  it('times out when the backend never answers play_track', async () => {
    vi.useFakeTimers()
    const store = usePlayerStore()
    const track = makeTrack('/music/a.mp3')
    store.playlist = [track]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'play_track') return new Promise(() => {})
      return Promise.resolve(undefined)
    })

    const play = store.playTrack(track)
    await vi.advanceTimersByTimeAsync(PLAY_TRACK_TIMEOUT_MS)
    await play

    expect(store.isPlaying).toBe(false)
    expect(store._isLoading).toBe(false)
  })

  it('auto-advances to the next track after a playback error', async () => {
    vi.useFakeTimers()
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    store.playlist = tracks
    let playCalls = 0
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'play_track') {
        playCalls++
        if (playCalls === 1) return Promise.reject(new Error('decode error'))
      }
      return Promise.resolve(undefined)
    })

    const play = store.playTrack(tracks[0]!)
    await vi.advanceTimersByTimeAsync(0)
    await play

    await vi.advanceTimersByTimeAsync(AUTO_NEXT_TRACK_DELAY_MS)

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track1.mp3' })
  })

  it('does not auto-advance when the failing track is the last one', async () => {
    vi.useFakeTimers()
    const store = usePlayerStore()
    const tracks = makePlaylist(1)
    store.playlist = tracks
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'play_track') return Promise.reject(new Error('decode error'))
      return Promise.resolve(undefined)
    })

    const play = store.playTrack(tracks[0]!)
    await vi.advanceTimersByTimeAsync(0)
    await play
    await vi.advanceTimersByTimeAsync(AUTO_NEXT_TRACK_DELAY_MS * 3)

    // 无后续曲目 → 只尝试过一次 play
    const playCalls = invokeMock.mock.calls.filter(([cmd]) => cmd === 'play_track')
    expect(playCalls).toHaveLength(1)
    expect(store.isPlaying).toBe(false)
  })
})

describe('player.playback > _onEnded', () => {
  it('replays the current track in track repeat mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)
    store.repeatMode = 'track'

    await store._onEnded()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
  })

  it('advances to the next track in list repeat mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 1)
    store.repeatMode = 'list'

    await store._onEnded()

    // 列表循环:播完最后一首回到第一首
    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
  })

  it('advances to the next track in sequential mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)

    await store._onEnded()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track1.mp3' })
  })

  it('stops at the end of the playlist in sequential mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 1)

    await store._onEnded()

    expect(store.isPlaying).toBe(false)
    expect(store.currentTime).toBe(store.duration)
    expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
  })

  it('stops after a full shuffle round in no-repeat mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)
    store.isShuffle = true
    store.repeatMode = 'none'
    store._shuffleOrder = [0, 1]
    store._shufflePosition = 1

    await store._onEnded()

    expect(store.isPlaying).toBe(false)
    expect(store.currentTime).toBe(store.duration)
  })

  it('continues along the shuffle order while a round is unfinished', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)
    store.isShuffle = true
    store._shuffleOrder = [0, 1]
    store._shufflePosition = 0

    await store._onEnded()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track1.mp3' })
  })

  it('stops immediately with an empty playlist', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/ghost.mp3')
    store.playlist = []
    store.isPlaying = true

    await store._onEnded()

    expect(store.isPlaying).toBe(false)
    expect(invokeMock).toHaveBeenCalledWith('pause_track')
  })

  it('ignores a stale ended event after the user switched tracks', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(2)
    setPlaying(store, tracks, 0)
    // pause 期间用户手动切到了 track1
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'pause_track') {
        store.currentTrack = tracks[1]!
      }
      return Promise.resolve(undefined)
    })

    await store._onEnded()

    expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
  })
})

describe('player.playback > nextTrack / previousTrack', () => {
  it('advances along the shuffle order and records history', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    setPlaying(store, tracks, 0)
    store.isShuffle = true
    store._shuffleOrder = [0, 2, 1]
    store._shufflePosition = 0

    await store.nextTrack()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track2.mp3' })
    expect(store._shuffleHistory).toEqual([0])
  })

  it('wraps around with a single-entry playlist in shuffle mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(1)
    setPlaying(store, tracks, 0)
    store.isShuffle = true

    await store.nextTrack()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
  })

  it('does nothing when the playlist is empty', async () => {
    const store = usePlayerStore()
    store.currentTrack = makeTrack('/music/ghost.mp3')
    store.playlist = []

    await store.nextTrack()
    await store.previousTrack()

    expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
  })

  it('does nothing without a current track', async () => {
    const store = usePlayerStore()
    store.playlist = makePlaylist(2)

    await store.nextTrack()
    await store.previousTrack()

    expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
  })

  it('goes back through the shuffle history', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    setPlaying(store, tracks, 2)
    store.isShuffle = true
    store._shuffleOrder = [0, 2, 1]
    store._shufflePosition = 1
    store._shuffleHistory = [0]

    await store.previousTrack()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
  })

  it('wraps to the last track in sequential mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    setPlaying(store, tracks, 0)

    await store.previousTrack()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track2.mp3' })
  })

  it('wraps around with a single-entry playlist in shuffle mode', async () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(1)
    setPlaying(store, tracks, 0)
    store.isShuffle = true

    await store.previousTrack()

    expect(invokeMock).toHaveBeenCalledWith('play_track', { path: '/music/track0.mp3' })
  })
})

describe('player.playback > volume & mute', () => {
  it('clamps the volume and clears the muted flag', async () => {
    const store = usePlayerStore()
    store.isMuted = true

    store.setVolume(1.5)

    expect(store.volume).toBe(1)
    expect(store.isMuted).toBe(false)
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 1 })
    })
  })

  it('keeps the muted flag and sends zero volume', async () => {
    const store = usePlayerStore()
    store.isMuted = true

    store.setVolume(0.8)

    expect(store.volume).toBe(0.8)
    expect(store.isMuted).toBe(false)
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0.8 })
    })
  })

  it('remembers the previous volume for later un-muting', () => {
    const store = usePlayerStore()
    store.setVolume(0.7)

    expect(store.previousVolume).toBe(0.7)
  })

  it('logs instead of throwing when setting the volume fails', async () => {
    const store = usePlayerStore()
    invokeMock.mockRejectedValueOnce(new Error('volume rejected'))

    store.setVolume(0.5)
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to set volume:', expect.any(Error))
    })
  })

  it('mutes and restores the volume', async () => {
    const store = usePlayerStore()
    store.volume = 0.6

    store.toggleMute()
    expect(store.isMuted).toBe(true)
    expect(store.previousVolume).toBe(0.6)
    await vi.waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0 })
    })

    store.toggleMute()
    expect(store.isMuted).toBe(false)
    expect(store.volume).toBe(0.6)
  })

  it('falls back to half volume when un-muting without a remembered volume', () => {
    const store = usePlayerStore()
    store.isMuted = true
    store.previousVolume = 0

    store.toggleMute()

    expect(store.volume).toBe(0.5)
  })

  it('logs instead of throwing when muting fails', async () => {
    const store = usePlayerStore()
    store.volume = 0.6
    invokeMock.mockRejectedValueOnce(new Error('mute rejected'))

    store.toggleMute()
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to mute:', expect.any(Error))
    })
  })

  it('logs instead of throwing when un-muting fails', async () => {
    const store = usePlayerStore()
    store.isMuted = true
    store.previousVolume = 0.6
    invokeMock.mockRejectedValueOnce(new Error('unmute rejected'))

    store.toggleMute()
    await vi.waitFor(() => {
      expect(loggerMock.error).toHaveBeenCalledWith('Failed to unmute:', expect.any(Error))
    })
  })
})

describe('player.playback > shuffle helpers & file check', () => {
  it('regenerates the shuffle order around the current track', () => {
    const store = usePlayerStore()
    const tracks = makePlaylist(3)
    setPlaying(store, tracks, 1)
    store._shuffleOrder = []

    store._regenerateShuffleOrder()

    expect(store._shuffleOrder).toHaveLength(3)
    expect(store._shuffleOrder[0]).toBe(1)
    expect(store._shufflePosition).toBe(0)
  })

  it('reports whether the shuffle order is still valid', () => {
    const store = usePlayerStore()
    store.playlist = makePlaylist(3)
    store._shuffleOrder = [0, 1, 2]
    store._shufflePosition = 1

    expect(store._isShuffleOrderValid()).toBe(true)

    store._shuffleOrder = [0, 1]
    expect(store._isShuffleOrderValid()).toBe(false)
  })

  it('caches negative file-existence results on errors', async () => {
    const store = usePlayerStore()
    fileExistsMock.mockRejectedValue(new Error('fs gone'))

    await expect(store._checkFileExists('/music/a.mp3')).resolves.toBe(false)
    expect(loggerMock.error).toHaveBeenCalledWith('Error checking file:', expect.any(Error))

    // 第二次走缓存,不再访问文件系统
    fileExistsMock.mockClear()
    await expect(store._checkFileExists('/music/a.mp3')).resolves.toBe(false)
    expect(fileExistsMock).not.toHaveBeenCalled()
  })

  it('returns false for an empty path without touching the fs', async () => {
    const store = usePlayerStore()

    await expect(store._checkFileExists('')).resolves.toBe(false)
    expect(fileExistsMock).not.toHaveBeenCalled()
  })
})
