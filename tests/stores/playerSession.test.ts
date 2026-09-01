import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import type { Track, ResumeResult, TrackSnapshot } from '@/types'

const loadPlaylistsFromCache = vi.fn().mockResolvedValue(undefined)
const selectPlaylist = vi.fn()
const musicLibraryState = {
  playlists: [] as { name: string; files: Track[] }[],
  currentPlaylist: null as { name: string; files: Track[] } | null,
}

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: () => ({
    get playlists() {
      return musicLibraryState.playlists
    },
    get currentPlaylist() {
      return musicLibraryState.currentPlaylist
    },
    loadPlaylistsFromCache,
    selectPlaylist,
  }),
}))

// mock 必须在导入被测模块之前声明
const { saveLastSessionNow, resumeLastSession } = await import('@/stores/playerSession')

const invokeMock = vi.mocked(invoke)

const snapshot = (path: string, extra: Partial<TrackSnapshot> = {}): TrackSnapshot => ({
  path,
  title: `Title ${path}`,
  artist: 'Artist',
  album: 'Album',
  duration: 200,
  bitrate: 320,
  sampleRate: 44100,
  channels: 2,
  bitDepth: 16,
  format: 'mp3',
  ...extra,
})

const track = (path: string, extra: Partial<Track> = {}): Track =>
  ({ path, name: path, displayTitle: path, ...extra }) as unknown as Track

type FakeStore = Parameters<typeof saveLastSessionNow>[0]

function makeStore(
  overrides: Partial<Record<string, unknown>> = {},
  playlist: Track[] = [],
  currentTrack: Track | null = null,
): FakeStore & Record<string, unknown> {
  return {
    playlist,
    currentTrack,
    duration: 0,
    currentTime: 0,
    isPlaying: true,
    audioInfo: null,
    _isDestroyed: false,
    _cacheAbortController: null as AbortController | null,
    _cachePlaylistMetadata: vi.fn(),
    _loadPlaylistCovers: vi.fn(),
    _updateTaskbarState: vi.fn(),
    ...overrides,
  } as unknown as FakeStore & Record<string, unknown>
}

beforeEach(() => {
  vi.clearAllMocks()
  invokeMock.mockResolvedValue(undefined)
  musicLibraryState.playlists = []
  musicLibraryState.currentPlaylist = null
})

describe('saveLastSessionNow', () => {
  it('does nothing when there is no current track', async () => {
    await saveLastSessionNow(makeStore())
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('does nothing after the store has been destroyed', async () => {
    const store = makeStore({ _isDestroyed: true }, [], track('/a.mp3'))
    store.currentTrack = track('/a.mp3')

    await saveLastSessionNow(store)

    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('saves position, duration and a playlist snapshot', async () => {
    const store = makeStore(
      { duration: 180, currentTime: 42.5 },
      [track('/a.mp3'), track('/b.mp3')],
      track('/a.mp3', { title: 'A', artist: 'Artist A' }),
    )

    await saveLastSessionNow(store)

    expect(invokeMock).toHaveBeenCalledWith('save_last_session', {
      trackPath: '/a.mp3',
      trackTitle: 'A',
      trackArtist: 'Artist A',
      durationSecs: 180,
      positionSecs: 42.5,
      playlistName: null,
      trackIndexInPlaylist: null,
      playlistTracks: [
        expect.objectContaining({ path: '/a.mp3' }),
        expect.objectContaining({ path: '/b.mp3' }),
      ],
    })
  })

  it('falls back to display fields when title/artist are missing', async () => {
    const store = makeStore(
      { duration: 10, currentTime: 1 },
      [track('/a.mp3', { displayTitle: '显示', displayArtist: '显示作者' })],
      track('/a.mp3', { displayTitle: '显示', displayArtist: '显示作者' }),
    )

    await saveLastSessionNow(store)

    expect(invokeMock).toHaveBeenCalledWith(
      'save_last_session',
      expect.objectContaining({ trackTitle: '显示', trackArtist: '显示作者' }),
    )
  })

  it('resolves the playlist name and track index from the music library', async () => {
    musicLibraryState.currentPlaylist = {
      name: 'Favorites',
      files: [track('/x.mp3'), track('/a.mp3'), track('/y.mp3')],
    }
    const store = makeStore({ duration: 1, currentTime: 2 }, [track('/a.mp3')], track('/a.mp3'))

    await saveLastSessionNow(store)

    expect(invokeMock).toHaveBeenCalledWith(
      'save_last_session',
      expect.objectContaining({ playlistName: 'Favorites', trackIndexInPlaylist: 1 }),
    )
  })

  it('leaves the playlist name null when the track is not in the current playlist', async () => {
    musicLibraryState.currentPlaylist = { name: 'Favorites', files: [track('/x.mp3')] }
    const store = makeStore({ duration: 1, currentTime: 2 }, [track('/a.mp3')], track('/a.mp3'))

    await saveLastSessionNow(store)

    expect(invokeMock).toHaveBeenCalledWith(
      'save_last_session',
      expect.objectContaining({ playlistName: 'Favorites', trackIndexInPlaylist: null }),
    )
  })

  it('drops optional metadata as null when the track lacks it', async () => {
    const store = makeStore({ duration: 0, currentTime: 0 }, [track('/a.mp3')], track('/a.mp3'))

    await saveLastSessionNow(store)

    expect(invokeMock).toHaveBeenCalledWith(
      'save_last_session',
      expect.objectContaining({
        playlistTracks: [
          {
            path: '/a.mp3',
            title: null,
            artist: null,
            album: null,
            duration: null,
            bitrate: null,
            sampleRate: null,
            channels: null,
            bitDepth: null,
            format: null,
          },
        ],
      }),
    )
  })

  it('swallows backend failures instead of throwing', async () => {
    invokeMock.mockRejectedValueOnce(new Error('disk full'))
    const store = makeStore({ duration: 1, currentTime: 2 }, [track('/a.mp3')], track('/a.mp3'))

    await expect(saveLastSessionNow(store)).resolves.toBeUndefined()
  })
})

describe('resumeLastSession', () => {
  it('returns null and logs when the backend call fails', async () => {
    invokeMock.mockRejectedValueOnce(new Error('backend down'))
    const store = makeStore()

    await expect(resumeLastSession(store)).resolves.toBeNull()
  })

  it('restores the playlist from playlistTracks and stays paused', async () => {
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/b.mp3',
      trackTitle: 'B',
      trackArtist: 'Artist B',
      durationSecs: 123,
      positionSecs: 45,
      playlistTracks: [snapshot('/a.mp3'), snapshot('/b.mp3')],
    } as ResumeResult)
    const store = makeStore()

    await resumeLastSession(store)

    expect(store.playlist.map((t: Track) => t.path)).toEqual(['/a.mp3', '/b.mp3'])
    expect(store.currentTrack!.path).toBe('/b.mp3')
    expect(store.duration).toBe(200)
    expect(store.currentTime).toBe(45)
    expect(store.isPlaying).toBe(false)
    expect(store.audioInfo).toEqual({
      bitrate: 320,
      sampleRate: 44100,
      channels: 2,
      bitDepth: 16,
      format: 'mp3',
    })
    expect(store._updateTaskbarState).toHaveBeenCalled()
  })

  it('appends a minimal track when it is absent from playlistTracks', async () => {
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/missing.mp3',
      trackTitle: 'Missing',
      trackArtist: 'Unknown',
      durationSecs: 99,
      positionSecs: 3,
      playlistTracks: [snapshot('/a.mp3')],
    } as ResumeResult)
    const store = makeStore()

    await resumeLastSession(store)

    expect(store.playlist).toHaveLength(2)
    expect(store.currentTrack).toMatchObject({
      path: '/missing.mp3',
      displayTitle: 'Missing',
      displayArtist: 'Unknown',
      duration: 99,
    })
    expect(store.duration).toBe(99)
  })

  it('uses the last-session snapshot duration when the track has none', async () => {
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/a.mp3',
      trackTitle: 'A',
      trackArtist: '',
      durationSecs: 321,
      positionSecs: 0,
      playlistTracks: [snapshot('/a.mp3', { duration: null })],
    } as ResumeResult)
    const store = makeStore()

    await resumeLastSession(store)

    expect(store.duration).toBe(321)
  })

  it('aborts a previous metadata caching run and kicks off new ones', async () => {
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/a.mp3',
      durationSecs: 1,
      positionSecs: 0,
      playlistTracks: [snapshot('/a.mp3')],
    } as ResumeResult)
    const abort = vi.fn()
    const store = makeStore({ _cacheAbortController: { abort } })

    await resumeLastSession(store)

    expect(abort).toHaveBeenCalled()
    expect(store._cachePlaylistMetadata).toHaveBeenCalledWith(store.playlist)
    expect(store._loadPlaylistCovers).toHaveBeenCalledWith(store.playlist)
  })

  it('syncs the music library playlist and prefers its richer metadata', async () => {
    const richTrack = track('/a.mp3', { bitrate: 999, displayTitle: '库内标题' })
    // 播放列表为空 => 触发 loadPlaylistsFromCache,由它填入待匹配的数据
    musicLibraryState.playlists = []
    loadPlaylistsFromCache.mockImplementation(async () => {
      musicLibraryState.playlists = [{ name: 'Favorites', files: [richTrack] }]
    })
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resume_last_session') {
        return Promise.resolve({
          resumed: true,
          status: 'ok',
          trackPath: '/a.mp3',
          trackTitle: '快照标题',
          trackArtist: '',
          durationSecs: 10,
          positionSecs: 1,
          playlistName: 'Favorites',
          playlistTracks: [snapshot('/a.mp3', { bitrate: null })],
        } as ResumeResult)
      }
      return Promise.resolve(null)
    })
    const store = makeStore()

    await resumeLastSession(store)

    expect(loadPlaylistsFromCache).toHaveBeenCalled()
    expect(selectPlaylist).toHaveBeenCalledWith(musicLibraryState.playlists[0])
    expect(store.currentTrack).toBe(richTrack)
    expect(store.audioInfo.bitrate).toBe(999)
  })

  it('does not reload playlists that are already present', async () => {
    musicLibraryState.playlists = [{ name: 'Favorites', files: [] }]
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/a.mp3',
      durationSecs: 1,
      positionSecs: 0,
      playlistName: 'Favorites',
      playlistTracks: [snapshot('/a.mp3')],
    } as ResumeResult)
    const store = makeStore()

    await resumeLastSession(store)

    expect(loadPlaylistsFromCache).not.toHaveBeenCalled()
  })

  it('survives a music library sync failure', async () => {
    loadPlaylistsFromCache.mockRejectedValueOnce(new Error('cache unavailable'))
    invokeMock.mockResolvedValueOnce({
      resumed: true,
      status: 'ok',
      trackPath: '/a.mp3',
      durationSecs: 10,
      positionSecs: 1,
      playlistName: 'Favorites',
      playlistTracks: [snapshot('/a.mp3')],
    } as ResumeResult)
    const store = makeStore()

    await expect(resumeLastSession(store)).resolves.toMatchObject({ resumed: true })
    expect(store.currentTrack!.path).toBe('/a.mp3')
  })

  it('removes a missing track from the playlist when the file is gone', async () => {
    invokeMock.mockResolvedValueOnce({
      resumed: false,
      status: 'not_found',
      trackPath: '/gone.mp3',
    } as ResumeResult)
    const store = makeStore({}, [track('/a.mp3'), track('/gone.mp3')])

    const result = await resumeLastSession(store)

    expect(result).toMatchObject({ status: 'not_found' })
    expect(store.playlist.map((t: Track) => t.path)).toEqual(['/a.mp3'])
    expect(store.currentTrack).toBeNull()
  })

  it('ignores a not_found status without a track path', async () => {
    invokeMock.mockResolvedValueOnce({ resumed: false, status: 'not_found' } as ResumeResult)
    const store = makeStore({}, [track('/a.mp3')])

    await resumeLastSession(store)

    expect(store.playlist).toHaveLength(1)
  })

  it('leaves state untouched for other non-resumed statuses', async () => {
    invokeMock.mockResolvedValueOnce({ resumed: false, status: 'empty' } as ResumeResult)
    const store = makeStore()

    await resumeLastSession(store)

    expect(store.currentTrack).toBeNull()
    expect(store.playlist).toHaveLength(0)
  })

  it('applies the cover once the backend resolves it', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resume_last_session') {
        return Promise.resolve({
          resumed: true,
          status: 'ok',
          trackPath: '/a.mp3',
          durationSecs: 10,
          positionSecs: 1,
          playlistTracks: [snapshot('/a.mp3')],
        } as ResumeResult)
      }
      if (cmd === 'get_track_cover_path') return Promise.resolve('/cover/a.jpg')
      return Promise.resolve(null)
    })
    const store = makeStore()

    await resumeLastSession(store)
    await Promise.resolve()

    expect(store.currentTrack!.coverPath).toBe('/cover/a.jpg')
  })

  it('skips the cover update when the store was destroyed meanwhile', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resume_last_session') {
        return Promise.resolve({
          resumed: true,
          status: 'ok',
          trackPath: '/a.mp3',
          durationSecs: 10,
          positionSecs: 1,
          playlistTracks: [snapshot('/a.mp3')],
        } as ResumeResult)
      }
      if (cmd === 'get_track_cover_path') return Promise.resolve('/cover/a.jpg')
      return Promise.resolve(null)
    })
    const store = makeStore()
    store._isDestroyed = true

    await resumeLastSession(store)
    await Promise.resolve()

    expect(store.currentTrack!.coverPath).toBeUndefined()
  })

  it('skips the cover update when the current track changed meanwhile', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resume_last_session') {
        return Promise.resolve({
          resumed: true,
          status: 'ok',
          trackPath: '/a.mp3',
          durationSecs: 10,
          positionSecs: 1,
          playlistTracks: [snapshot('/a.mp3')],
        } as ResumeResult)
      }
      if (cmd === 'get_track_cover_path') return Promise.resolve('/cover/a.jpg')
      return Promise.resolve(null)
    })
    const store = makeStore()

    await resumeLastSession(store)
    store.currentTrack = track('/other.mp3')
    store.currentTrack.path = '/other.mp3'
    await Promise.resolve()

    expect(store.currentTrack!.coverPath).toBeUndefined()
  })

  it('swallows a failed cover lookup', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resume_last_session') {
        return Promise.resolve({
          resumed: true,
          status: 'ok',
          trackPath: '/a.mp3',
          durationSecs: 10,
          positionSecs: 1,
          playlistTracks: [snapshot('/a.mp3')],
        } as ResumeResult)
      }
      if (cmd === 'get_track_cover_path') return Promise.reject(new Error('no cover'))
      return Promise.resolve(null)
    })
    const store = makeStore()

    await expect(resumeLastSession(store)).resolves.toMatchObject({ resumed: true })
    await Promise.resolve()
    expect(store.currentTrack!.coverPath).toBeUndefined()
  })
})
