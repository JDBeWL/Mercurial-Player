import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import type { Track } from '@/types'
import {
  recordCoverUpdate,
  takeCoverUpdates,
  cachePlaylistMetadata,
  loadPlaylistCovers,
  loadTrackBpm,
} from '@/stores/player/mediaCache'

const track = (path: string, extra: Partial<Track> = {}): Track =>
  ({ path, name: path, ...extra }) as unknown as Track

interface MockStore {
  playlist: Track[]
  currentTrack: Track | null
  playlistCoverVersion: number
  audioInfo: Record<string, number | null>
  _cacheAbortController: AbortController | null
  _bpmAbortController: AbortController | null
  _getMetadataCache: () => {
    has: (k: string) => boolean
    get: (k: string) => unknown
    set: (k: string, v: unknown) => void
  }
  __cache: Map<string, unknown>
}

function makeStore(playlist: Track[] = [], currentTrack: Track | null = null): MockStore {
  const cache = new Map<string, unknown>()
  return {
    playlist,
    currentTrack,
    playlistCoverVersion: 0,
    audioInfo: {
      bitrate: null,
      sampleRate: null,
      channels: null,
      bitDepth: null,
      format: null,
      bpm: null,
    },
    _cacheAbortController: null,
    _bpmAbortController: null,
    _getMetadataCache: () => ({
      has: (k: string) => cache.has(k),
      get: (k: string) => cache.get(k) ?? null,
      set: (k: string, v: unknown) => void cache.set(k, v),
    }),
    __cache: cache,
  }
}

/** 被测函数签名要求完整 `PlayerStore`,测试只提供最小形状 */
const cacheMetadata = (store: MockStore, playlist: Track[]) =>
  cachePlaylistMetadata(store as never, playlist)
const loadCovers = (store: MockStore, playlist: Track[]) =>
  loadPlaylistCovers(store as never, playlist)
const loadBpm = (store: MockStore, track: Track) => loadTrackBpm(store as never, track)

beforeEach(() => {
  vi.clearAllMocks()
  // 清空模块级 pendingCoverUpdates,避免用例间互相污染
  takeCoverUpdates()
})

describe('recordCoverUpdate / takeCoverUpdates', () => {
  it('returns an empty map when nothing was recorded', () => {
    expect(takeCoverUpdates().size).toBe(0)
  })

  it('returns recorded updates and clears the buffer', () => {
    recordCoverUpdate('/a.mp3', '/cover/a.jpg')
    recordCoverUpdate('/b.mp3', '/cover/b.jpg')

    const taken = takeCoverUpdates()
    expect(taken.get('/a.mp3')).toBe('/cover/a.jpg')
    expect(taken.get('/b.mp3')).toBe('/cover/b.jpg')
    expect(takeCoverUpdates().size).toBe(0)
  })

  it('overwrites an earlier update for the same path', () => {
    recordCoverUpdate('/a.mp3', '/cover/old.jpg')
    recordCoverUpdate('/a.mp3', '/cover/new.jpg')
    expect(takeCoverUpdates().get('/a.mp3')).toBe('/cover/new.jpg')
  })

  it('returns a copy, so mutating the result does not affect the buffer', () => {
    recordCoverUpdate('/a.mp3', '/cover/a.jpg')
    const taken = takeCoverUpdates()
    taken.clear()
    recordCoverUpdate('/b.mp3', '/cover/b.jpg')
    expect([...takeCoverUpdates().keys()]).toEqual(['/b.mp3'])
  })
})

describe('cachePlaylistMetadata', () => {
  it('does nothing for an empty playlist', async () => {
    const store = makeStore([])
    await cacheMetadata(store, [])
    expect(store._cacheAbortController).toBeNull()
  })

  it('caches metadata for every track, preferring display fields', async () => {
    const store = makeStore()
    const playlist = [
      track('/a.mp3', { displayTitle: '显示标题', title: '原标题' }),
      track('/b.flac', { title: '原标题', artist: '艺术家', album: '专辑', duration: 200 }),
    ]

    await cacheMetadata(store, playlist)

    const a = store.__cache.get('/a.mp3') as Record<string, unknown>
    expect(a.title).toBe('显示标题')
    const b = store.__cache.get('/b.flac') as Record<string, unknown>
    expect(b).toMatchObject({ title: '原标题', artist: '艺术家', album: '专辑', duration: 200 })
  })

  it('falls back to the file name when no title is available', async () => {
    const store = makeStore()
    await cacheMetadata(store, [track('/dir/song name.mp3')])
    expect((store.__cache.get('/dir/song name.mp3') as Record<string, unknown>).title).toBe(
      'song name',
    )
  })

  it('skips tracks that are already cached or have no path', async () => {
    const store = makeStore()
    store.__cache.set('/a.mp3', { title: '已有' })
    const noPath = { path: '', name: 'x' } as unknown as Track

    await cacheMetadata(store, [track('/a.mp3', { title: '新' }), noPath])

    expect((store.__cache.get('/a.mp3') as Record<string, unknown>).title).toBe('已有')
    expect(store.__cache.size).toBe(1)
  })

  it('stores an AbortController so a later run can cancel the previous one', async () => {
    const store = makeStore()
    const first = await cacheMetadata(store, [track('/a.mp3')]).then(
      () => store._cacheAbortController,
    )
    expect(first).toBeInstanceOf(AbortController)
  })

  it('aborts mid-way when a newer run replaces the controller', async () => {
    const store = makeStore()
    // 超过 200 首才会触发让出主线程,给第二次调用提供取消窗口
    const playlist = Array.from({ length: 250 }, (_, i) => track(`/t${i}.mp3`))

    const firstRun = cacheMetadata(store, playlist)
    await Promise.resolve()
    store._cacheAbortController?.abort()
    await firstRun

    expect(store.__cache.size).toBe(200)
    expect(store.__cache.has('/t249.mp3')).toBe(false)
  })

  it('yields to the event loop every 200 cached tracks', async () => {
    const store = makeStore()
    const playlist = Array.from({ length: 450 }, (_, i) => track(`/t${i}.mp3`))
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    await cacheMetadata(store, playlist)

    expect(setTimeoutSpy.mock.calls.filter(([, delay]) => delay === 0).length).toBe(2)
    expect(store.__cache.size).toBe(450)
    setTimeoutSpy.mockRestore()
  })
})

describe('loadPlaylistCovers', () => {
  it('does nothing for an empty playlist', async () => {
    await loadCovers(makeStore(), [])
    expect(invoke).not.toHaveBeenCalled()
  })

  it('assigns coverPath, records the update and bumps the version once per batch', async () => {
    vi.mocked(invoke).mockResolvedValue('/cover/a.jpg')
    const playlist = [track('/a.mp3'), track('/b.mp3')]
    const store = makeStore(playlist)

    await loadCovers(store, playlist)

    expect(playlist[0]!.coverPath).toBe('/cover/a.jpg')
    expect(playlist[1]!.coverPath).toBe('/cover/a.jpg')
    expect(store.playlistCoverVersion).toBe(1)
    expect(takeCoverUpdates().size).toBe(2)
  })

  it('syncs the cover onto currentTrack when paths match', async () => {
    vi.mocked(invoke).mockResolvedValue('/cover/cur.jpg')
    const current = track('/cur.mp3')
    const store = makeStore([current], current)

    await loadCovers(store, [current])

    expect(store.currentTrack!.coverPath).toBe('/cover/cur.jpg')
  })

  it('leaves currentTrack untouched when paths differ', async () => {
    vi.mocked(invoke).mockResolvedValue('/cover/other.jpg')
    const current = track('/cur.mp3')
    const store = makeStore([track('/other.mp3')], current)

    await loadCovers(store, [track('/other.mp3')])

    expect(store.currentTrack!.coverPath).toBeUndefined()
  })

  it('backfills coverPath into the metadata cache', async () => {
    vi.mocked(invoke).mockResolvedValue('/cover/a.jpg')
    const store = makeStore()
    store.__cache.set('/a.mp3', { title: 'A', coverPath: undefined })

    await loadCovers(store, [track('/a.mp3')])

    expect((store.__cache.get('/a.mp3') as Record<string, unknown>).coverPath).toBe('/cover/a.jpg')
  })

  it('skips tracks that already have a coverPath', async () => {
    const store = makeStore()
    const already = track('/a.mp3', { coverPath: '/cover/existing.jpg' })

    await loadCovers(store, [already])

    expect(invoke).not.toHaveBeenCalled()
    expect(store.playlistCoverVersion).toBe(0)
  })

  it('does not bump the version when the backend returns null', async () => {
    vi.mocked(invoke).mockResolvedValue(null)
    const store = makeStore()

    await loadCovers(store, [track('/a.mp3')])

    expect(store.playlistCoverVersion).toBe(0)
    expect(takeCoverUpdates().size).toBe(0)
  })

  it('swallows per-track failures and keeps processing the batch', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('backend down'))
    vi.mocked(invoke).mockResolvedValueOnce('/cover/b.jpg')
    const playlist = [track('/a.mp3'), track('/b.mp3')]
    const store = makeStore(playlist)

    await expect(loadCovers(store, playlist)).resolves.toBeUndefined()

    expect(playlist[0]!.coverPath).toBeUndefined()
    expect(playlist[1]!.coverPath).toBe('/cover/b.jpg')
    expect(store.playlistCoverVersion).toBe(1)
  })

  it('processes large playlists in batches of 10, yielding between batches', async () => {
    vi.mocked(invoke).mockResolvedValue('/cover/x.jpg')
    const playlist = Array.from({ length: 25 }, (_, i) => track(`/t${i}.mp3`))
    const store = makeStore(playlist)
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

    await loadCovers(store, playlist)

    expect(setTimeoutSpy.mock.calls.filter(([, delay]) => delay === 0).length).toBe(2)
    expect(store.playlistCoverVersion).toBe(3)
    expect(takeCoverUpdates().size).toBe(25)
    setTimeoutSpy.mockRestore()
  })
})

describe('loadTrackBpm', () => {
  it('does nothing for a track with an empty path', async () => {
    await loadBpm(makeStore(), track(''))
    expect(invoke).not.toHaveBeenCalled()
  })

  it('does nothing when the track already has a bpm', async () => {
    await loadBpm(makeStore(), track('/a.mp3', { bpm: 120 }))
    expect(invoke).not.toHaveBeenCalled()
  })

  it('asks the backend for exactly one path and writes the result to the track and metadata cache', async () => {
    vi.mocked(invoke).mockResolvedValue([{ path: '/a.mp3', bpm: 128 }])
    const current = track('/a.mp3')
    const store = makeStore([current], current)
    store.__cache.set('/a.mp3', { title: 'A', bpm: null })

    await loadBpm(store, current)

    expect(invoke).toHaveBeenCalledWith('analyze_tracks_bpm', { paths: ['/a.mp3'] })
    expect(current.bpm).toBe(128)
    // 元数据缓存同步,切回来时 prepareTrack 才不会把 BPM 丢掉
    expect((store.__cache.get('/a.mp3') as Record<string, unknown>).bpm).toBe(128)
  })

  it('backfills the currently playing track and its audioInfo snapshot', async () => {
    vi.mocked(invoke).mockResolvedValue([{ path: '/cur.mp3', bpm: 100 }])
    const current = track('/cur.mp3')
    const store = makeStore([current], current)

    await loadBpm(store, current)

    expect(store.currentTrack!.bpm).toBe(100)
    expect(store.audioInfo.bpm).toBe(100)
    expect(store.playlistCoverVersion).toBe(0)
  })

  it('leaves the current track alone when the backend reports null', async () => {
    vi.mocked(invoke).mockResolvedValue([{ path: '/cur.mp3', bpm: null }])
    const current = track('/cur.mp3')
    const store = makeStore([current], current)

    await loadBpm(store, current)

    expect(store.currentTrack!.bpm).toBeUndefined()
    expect(store.audioInfo.bpm).toBeNull()
  })

  it('does not touch the store when the analyzed track is no longer playing', async () => {
    // 识别是异步的，返回时用户可能已经切歌：结果已进后端缓存，但界面不该再动
    vi.mocked(invoke).mockResolvedValue([{ path: '/old.mp3', bpm: 90 }])
    const analyzed = track('/old.mp3')
    const nowPlaying = track('/new.mp3')
    const store = makeStore([analyzed, nowPlaying], nowPlaying)
    await loadBpm(store, analyzed)

    expect(store.currentTrack!.path).toBe('/new.mp3')
    expect(store.currentTrack!.bpm).toBeUndefined()
    expect(store.audioInfo.bpm).toBeNull()
  })

  it('keeps the BPM result when the batch-cache signal is aborted', async () => {
    // BPM 与批量缓存不能共用取消信号,否则普通切歌会把结果一起丢掉
    const current = track('/cur.mp3')
    const store = makeStore([current], current)
    const batch = new AbortController()
    store._cacheAbortController = batch
    vi.mocked(invoke).mockImplementation(() => {
      batch.abort()
      return Promise.resolve([{ path: '/cur.mp3', bpm: 100 }])
    })

    await loadBpm(store, current)

    expect(store.currentTrack!.bpm).toBe(100)
  })

  it('does not touch the store when its own signal was aborted', async () => {
    const current = track('/cur.mp3')
    const store = makeStore([current], current)
    vi.mocked(invoke).mockImplementation(() => {
      // 模拟分析期间被新的 BPM 请求取代:`loadTrackBpm` 开始时会挂上新 controller
      store._bpmAbortController?.abort()
      return Promise.resolve([{ path: '/cur.mp3', bpm: 100 }])
    })

    await loadBpm(store, current)

    expect(store.currentTrack!.bpm).toBeUndefined()
    expect(store.audioInfo.bpm).toBeNull()
  })

  // `loadTrack` 必须传 resolvedPath:未解析路径与 `currentTrack.path` 不等,结果会被丢弃
  it('matches the resolved path that currentTrack actually holds', async () => {
    const rawPath = 'E:/Music/Album/track.flac'
    const resolvedPath = 'E:\\Music\\Album\\track.flac'
    vi.mocked(invoke).mockResolvedValue([{ path: resolvedPath, bpm: 128 }])
    const current = track(resolvedPath)
    const store = makeStore([current], current)

    await loadBpm(store, track(resolvedPath))

    expect(store.currentTrack!.bpm).toBe(128)
    expect(store.audioInfo.bpm).toBe(128)

    const store2 = makeStore([current], current)
    await loadBpm(store2, track(rawPath))
    expect(store2.currentTrack!.bpm).toBeUndefined()
    expect(store2.audioInfo.bpm).toBeNull()
  })
})
