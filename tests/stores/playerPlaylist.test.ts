import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Track } from '@/types'
import {
  addTrackNextInPlaylist,
  addTracksNextInPlaylist,
  removeTrackFromPlaylist,
} from '@/stores/playerPlaylist'
import { adjustShuffleAfterRemove } from '@/stores/shuffle'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))
vi.mock('@/stores/shuffle', () => ({
  adjustShuffleAfterRemove: vi.fn((_order, pos, history, _index) => ({
    order: [9],
    position: pos,
    history,
  })),
}))

const track = (path: string): Track => ({ path, name: path } as unknown as Track)

/** 被测函数接收的 store 参数类型(完整 Pinia PlayerStore,测试只需结构子集) */
type PlaylistStoreParam = Parameters<typeof removeTrackFromPlaylist>[0]

/** 构造满足被测函数需求的最小 store 形状 */
function makeStore(tracks: Track[], currentIndex = 0): PlaylistStoreParam {
  return {
    playlist: tracks,
    currentTrackIndex: currentIndex,
    currentTrack: tracks[currentIndex] ?? null,
    isPlaying: false,
    resetPlayerState: vi.fn(),
    playTrack: vi.fn().mockResolvedValue(undefined),
    pause: vi.fn(),
    _shuffleOrder: [] as number[],
    _shufflePosition: 0,
    _shuffleHistory: [] as number[],
  } as unknown as PlaylistStoreParam
}

describe('removeTrackFromPlaylist', () => {
  let store: ReturnType<typeof makeStore>

  beforeEach(() => {
    store = makeStore([track('a'), track('b'), track('c')])
  })

  it('removes a non-current track and keeps state', () => {
    removeTrackFromPlaylist(store, 'c')
    expect(store.playlist.map((t) => t.path)).toEqual(['a', 'b'])
    expect(store.currentTrack?.path).toBe('a')
    expect(store.resetPlayerState).not.toHaveBeenCalled()
  })

  it('is a no-op when the path is not in the playlist', () => {
    removeTrackFromPlaylist(store, 'missing')
    expect(store.playlist).toHaveLength(3)
  })

  it('resets player state when the last track is removed', () => {
    store = makeStore([track('a')])
    removeTrackFromPlaylist(store, 'a')
    expect(store.resetPlayerState).toHaveBeenCalledWith(false)
  })

  it('plays the next track when removing the current track', async () => {
    store.isPlaying = true
    removeTrackFromPlaylist(store, 'a')
    expect(store.playTrack).toHaveBeenCalledWith(store.playlist[0]) // 原 'b' 前移
  })

  it('adjusts shuffle order when removing a track before current', () => {
    // current 是 'b'(索引 1),删除其前面的 'a' 走 shuffle 同步分支
    store = makeStore([track('a'), track('b'), track('c')], 1)
    store._shuffleOrder = [0, 1, 2]
    removeTrackFromPlaylist(store, 'a')
    expect(adjustShuffleAfterRemove).toHaveBeenCalled()
    expect(store._shuffleOrder).toEqual([9])
  })
})

describe('addTrackNextInPlaylist', () => {
  it('unshifts when playlist is empty or no current track', () => {
    const empty = makeStore([], -1)
    addTrackNextInPlaylist(empty, track('x'))
    expect(empty.playlist[0].path).toBe('x')
  })

  it('inserts after the current track', () => {
    const store = makeStore([track('a'), track('b')], 0)
    addTrackNextInPlaylist(store, track('new'))
    expect(store.playlist.map((t) => t.path)).toEqual(['a', 'new', 'b'])
  })

  it('moves an existing track to next position', () => {
    const store = makeStore([track('a'), track('b'), track('c')], 0)
    addTrackNextInPlaylist(store, track('c'))
    expect(store.playlist.map((t) => t.path)).toEqual(['a', 'c', 'b'])
  })

  it('ignores falsy track', () => {
    const store = makeStore([track('a')], 0)
    addTrackNextInPlaylist(store, undefined as unknown as Track)
    expect(store.playlist).toHaveLength(1)
  })
})

describe('addTracksNextInPlaylist', () => {
  it('unshifts all tracks when playlist is empty', () => {
    const store = makeStore([], -1)
    addTracksNextInPlaylist(store, [track('x'), track('y')])
    expect(store.playlist.map((t) => t.path)).toEqual(['x', 'y'])
  })

  it('filters existing tracks and inserts the rest after current', () => {
    const store = makeStore([track('a'), track('b')], 0)
    addTracksNextInPlaylist(store, [track('b'), track('c'), track('d')])
    expect(store.playlist.map((t) => t.path)).toEqual(['a', 'c', 'd', 'b'])
  })

  it('ignores empty or falsy input', () => {
    const store = makeStore([track('a')], 0)
    addTracksNextInPlaylist(store, [])
    addTracksNextInPlaylist(store, undefined as unknown as Track[])
    expect(store.playlist).toHaveLength(1)
  })
})
