import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import type { Track } from '@/types'
import {
  recordCoverUpdate,
  takeCoverUpdates,
  cachePlaylistMetadata,
  loadPlaylistCovers,
} from '@/stores/playerMediaCache'

const track = (path: string, extra: Partial<Track> = {}): Track =>
  ({ path, name: path, ...extra }) as unknown as Track

/** 构造满足被测函数需求的最小 store 形状 */
interface MockStore {
  playlist: Track[]
  currentTrack: Track | null
  playlistCoverVersion: number
  _cacheAbortController: AbortController | null
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
    _cacheAbortController: null,
    _getMetadataCache: () => ({
      has: (k: string) => cache.has(k),
      get: (k: string) => cache.get(k) ?? null,
      set: (k: string, v: unknown) => void cache.set(k, v),
    }),
    __cache: cache,
  }
}

/** 被测函数签名要求完整 PlayerStore,测试只提供最小形状 */
const cacheMetadata = (store: MockStore, playlist: Track[]) =>
  cachePlaylistMetadata(store as never, playlist)
const loadCovers = (store: MockStore, playlist: Track[]) =>
  loadPlaylistCovers(store as never, playlist)

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
    // 取走后清空
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
    // 模拟第二次调用取消第一次
    store._cacheAbortController?.abort()
    await firstRun

    // 被取消后写入数量应少于完整列表
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

    // 25 首 => 3 批,批间让出 2 次
    expect(setTimeoutSpy.mock.calls.filter(([, delay]) => delay === 0).length).toBe(2)
    expect(store.playlistCoverVersion).toBe(3)
    expect(takeCoverUpdates().size).toBe(25)
    setTimeoutSpy.mockRestore()
  })
})
