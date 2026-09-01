import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import type { Playlist, Track } from '@/types'

const getTrackCoverPath = vi.fn()

vi.mock('@/services/mediaService', () => ({ getTrackCoverPath }))

const { useLibrarySearch } = await import('@/composables/useLibrarySearch')

const track = (path: string, extra: Partial<Track> = {}): Track =>
  ({ path, name: path, ...extra }) as Track

const playlist = (name: string, files: Track[]): Playlist =>
  ({ name, files }) as unknown as Playlist

function makeStores(sortOrder: 'asc' | 'desc' = 'asc') {
  return {
    configStore: { playlist: { sortOrder } },
    playerStore: { recordCoverUpdate: vi.fn() },
  }
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

beforeEach(() => {
  vi.clearAllMocks()
  getTrackCoverPath.mockResolvedValue(null)
})

describe('search filtering', () => {
  it('returns nothing for an empty search term', async () => {
    const playlists = ref([playlist('P', [track('/a.mp3')])])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = '   '
    await handleSearch()

    expect(searchResults.value).toEqual([])
  })

  it('clears the pending debounce timer on a repeat call', async () => {
    vi.useFakeTimers()
    const playlists = ref([playlist('P', [track('/a.mp3', { title: 'Hello' })])])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'Hello'
    void handleSearch()
    void handleSearch()
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)

    expect(searchResults.value).toHaveLength(1)
    vi.useRealTimers()
  })

  it('matches title, artist, album and file name case-insensitively', async () => {
    vi.useFakeTimers()
    const playlists = ref([
      playlist('P', [
        track('/1.mp3', { title: 'Bohemian Rhapsody' }),
        track('/2.mp3', { artist: 'Queen' }),
        track('/3.mp3', { album: 'A Night at the Opera' }),
        track('/4.mp3', { name: 'Killer Queen.mp3' }),
        track('/5.mp3', { title: 'Unrelated' }),
      ]),
    ])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'QUEEN'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)

    expect(searchResults.value.map((r) => r.path)).toEqual(
      expect.arrayContaining(['/2.mp3', '/4.mp3']),
    )
    expect(searchResults.value.map((r) => r.path)).not.toContain('/5.mp3')
    vi.useRealTimers()
  })

  it('deduplicates a track present in several playlists', async () => {
    vi.useFakeTimers()
    const shared = track('/a.mp3', { title: 'Shared' })
    const playlists = ref([playlist('A', [shared]), playlist('B', [shared])])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'shared'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)

    expect(searchResults.value).toHaveLength(1)
    vi.useRealTimers()
  })

  it('skips playlists without files', async () => {
    vi.useFakeTimers()
    const playlists = ref([{ name: 'Empty' } as unknown as Playlist])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'anything'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)

    expect(searchResults.value).toEqual([])
    vi.useRealTimers()
  })
})

describe('sorting', () => {
  /** 执行一次完整搜索并返回结果 */
  async function search(sortOrder: 'asc' | 'desc', files: Track[]) {
    vi.useFakeTimers()
    const playlists = ref([playlist('P', files)])
    const { searchTerm, searchResults, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores(sortOrder)) as [never, never]),
    )
    // '.' 命中所有形如 '/x.mp3' 的 name,便于只观察排序结果
    searchTerm.value = '.'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)
    const result = searchResults.value.map((r) => r.path)
    vi.useRealTimers()
    return result
  }

  const files = () => [
    track('/c.mp3', { title: 'Charlie' }),
    track('/a.mp3', { title: 'alpha' }),
    track('/b.mp3', { title: 'Bravo' }),
  ]

  it('sorts A-Z for the asc order', async () => {
    expect(await search('asc', files())).toEqual(['/a.mp3', '/b.mp3', '/c.mp3'])
  })

  it('sorts Z-A for the desc order', async () => {
    expect(await search('desc', files())).toEqual(['/c.mp3', '/b.mp3', '/a.mp3'])
  })

  it('falls back to the file name when no title is present', async () => {
    expect(await search('asc', [track('/z.mp3'), track('/a.mp3')])).toEqual(['/a.mp3', '/z.mp3'])
  })
})

describe('cover loading', () => {
  it('seeds the cover map with tracks that already have a cover', async () => {
    vi.useFakeTimers()
    const playlists = ref([
      playlist('P', [track('/a.mp3', { title: 'A', coverPath: '/cover/a.jpg' })]),
    ])
    const { searchTerm, searchCovers, handleSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'a'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)

    expect(searchCovers.value.get('/a.mp3')).toBe('/cover/a.jpg')
    expect(getTrackCoverPath).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  it('fetches missing covers in batches and notifies the player store', async () => {
    const playlists = ref([
      playlist(
        'P',
        Array.from({ length: 7 }, (_, i) => track(`/t${i}.mp3`, { title: `t${i}` })),
      ),
    ])
    const { configStore, playerStore } = makeStores()
    const { searchTerm, searchCovers, handleSearch } = useLibrarySearch(
      playlists,
      configStore as never,
      playerStore as never,
    )
    getTrackCoverPath.mockImplementation((path: string) =>
      Promise.resolve(`/cover${path.replace('/', '-')}`),
    )

    searchTerm.value = 't'
    await handleSearch()
    await vi.waitFor(() => expect(searchCovers.value.size).toBe(7))

    expect(getTrackCoverPath).toHaveBeenCalledTimes(7)
    expect(playerStore.recordCoverUpdate).toHaveBeenCalledTimes(7)
    expect(searchCovers.value.get('/t0.mp3')).toBe('/cover-t0.mp3')
  })

  it('writes the resolved cover back onto the track object', async () => {
    const file = track('/a.mp3', { title: 'A' })
    const playlists = ref([playlist('P', [file])])
    const { configStore, playerStore } = makeStores()
    const { searchTerm, handleSearch } = useLibrarySearch(
      playlists,
      configStore as never,
      playerStore as never,
    )
    getTrackCoverPath.mockResolvedValue('/cover/a.jpg')

    searchTerm.value = 'a'
    await handleSearch()
    await vi.waitFor(() => expect(file.coverPath).toBe('/cover/a.jpg'))
  })

  it('ignores a null cover result', async () => {
    const playlists = ref([playlist('P', [track('/a.mp3', { title: 'A' })])])
    const { configStore, playerStore } = makeStores()
    const { searchTerm, searchCovers, handleSearch } = useLibrarySearch(
      playlists,
      configStore as never,
      playerStore as never,
    )
    getTrackCoverPath.mockResolvedValue(null)

    searchTerm.value = 'a'
    await handleSearch()
    await flush()

    expect(searchCovers.value.size).toBe(0)
    expect(playerStore.recordCoverUpdate).not.toHaveBeenCalled()
  })

  it('swallows a per-track cover failure', async () => {
    const playlists = ref([
      playlist('P', [track('/a.mp3', { title: 'A' }), track('/b.mp3', { title: 'B' })]),
    ])
    const { configStore, playerStore } = makeStores()
    const { searchTerm, searchCovers, handleSearch } = useLibrarySearch(
      playlists,
      configStore as never,
      playerStore as never,
    )
    getTrackCoverPath.mockImplementation((path: string) =>
      path === '/a.mp3' ? Promise.reject(new Error('no cover')) : Promise.resolve('/cover/b.jpg'),
    )

    searchTerm.value = 'b'
    await handleSearch()
    await vi.waitFor(() => expect(searchCovers.value.size).toBe(1))

    expect(searchCovers.value.get('/b.mp3')).toBe('/cover/b.jpg')
  })

  it('cancels a superseded cover load', async () => {
    const playlists = ref([playlist('P', [track('/a.mp3', { title: 'A' })])])
    const { configStore, playerStore } = makeStores()
    const { searchTerm, searchCovers, handleSearch, clearSearch } = useLibrarySearch(
      playlists,
      configStore as never,
      playerStore as never,
    )
    let release: (value: string) => void = () => {}
    getTrackCoverPath.mockReturnValue(new Promise((resolve) => (release = resolve)))

    searchTerm.value = 'a'
    void handleSearch()
    // 真实定时器:等防抖结束并进入封面加载
    await new Promise((resolve) => setTimeout(resolve, 320))
    clearSearch()
    release('/cover/late.jpg')
    await flush()

    expect(searchCovers.value.size).toBe(0)
    expect(playerStore.recordCoverUpdate).not.toHaveBeenCalled()
  })
})

describe('clearSearch', () => {
  it('resets the term, results and covers', async () => {
    vi.useFakeTimers()
    const playlists = ref([playlist('P', [track('/a.mp3', { title: 'A' })])])
    const { searchTerm, searchResults, searchCovers, handleSearch, clearSearch } = useLibrarySearch(
      playlists,
      ...(Object.values(makeStores()) as [never, never]),
    )

    searchTerm.value = 'a'
    void handleSearch()
    await vi.advanceTimersByTimeAsync(300)
    expect(searchResults.value).toHaveLength(1)

    clearSearch()

    expect(searchTerm.value).toBe('')
    expect(searchResults.value).toEqual([])
    expect(searchCovers.value.size).toBe(0)
    vi.useRealTimers()
  })
})
