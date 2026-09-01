import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import type { Track } from '@/types'

const configState = vi.hoisted(() => ({
  titleExtraction: {
    preferMetadata: true,
    hideFileExtension: true,
    parseArtistTitle: true,
    separator: '-',
    customSeparators: ['-', '_', '.'],
  } as Record<string, unknown> | undefined,
}))

vi.mock('@/stores/config', () => ({
  // 必须用 getter:useTrackInfo 会缓存 store 实例,直接取值会冻结首次快照
  useConfigStore: () => ({
    get titleExtraction() {
      return configState.titleExtraction
    },
  }),
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: {
    getFileName: (p: string) => p.split(/[\\/]/).pop() ?? p,
    getFileNameWithoutExtension: (p: string) =>
      (p.split(/[\\/]/).pop() ?? p).replace(/\.[^.]+$/, ''),
    getFileExtension: (p: string) => p.split('.').pop() ?? '',
  },
}))

const extractTitle = vi.fn()

vi.mock('@/utils/titleExtractor', () => ({
  TitleExtractor: { extractTitle: (...args: unknown[]) => extractTitle(...(args as [])) },
}))

const { useTrackInfo } = await import('@/composables/useTrackInfo')
const logger = (await import('@/utils/logger')).default

const track = (path: string, extra: Partial<Track> = {}): Track => ({ path, ...extra }) as Track

beforeEach(() => {
  vi.clearAllMocks()
  configState.titleExtraction = {
    preferMetadata: true,
    hideFileExtension: true,
    parseArtistTitle: true,
    separator: '-',
    customSeparators: ['-', '_', '.'],
  }
  extractTitle.mockResolvedValue({ title: 'Extracted', artist: 'Artist', isFromMetadata: true })
  useTrackInfo().clearAllCache()
})

describe('getTrackTitle', () => {
  it('returns the fallback for a missing track', () => {
    expect(useTrackInfo().getTrackTitle(null, '未知')).toBe('未知')
    expect(useTrackInfo().getTrackTitle(undefined)).toBe('')
    expect(useTrackInfo().getTrackTitle({} as Track, 'x')).toBe('x')
  })

  it('falls back to the file name while processing', () => {
    extractTitle.mockReturnValue(new Promise(() => {})) // 永不 resolve,保持 processing

    const result = useTrackInfo().getTrackTitle(track('/dir/song.mp3'))

    expect(result).toBe('song')
  })

  it('keeps the file extension when the config asks for it', () => {
    configState.titleExtraction!.hideFileExtension = false
    extractTitle.mockReturnValue(new Promise(() => {}))

    expect(useTrackInfo().getTrackTitle(track('/dir/song.mp3'))).toBe('song.mp3')
  })

  it('prefers the store title until extraction finishes', () => {
    extractTitle.mockReturnValue(new Promise(() => {}))

    expect(useTrackInfo().getTrackTitle(track('/dir/raw.mp3', { title: 'Store Title' }))).toBe(
      'Store Title',
    )
  })

  it('returns the extracted title once processing completes', async () => {
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/dir/song.mp3')) // 触发处理

    await vi.waitFor(() => expect(getTrackTitle(track('/dir/song.mp3'))).toBe('Extracted'))
  })

  it('falls back when the extracted title is empty', async () => {
    extractTitle.mockResolvedValue({ title: '', artist: '' })
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/dir/song.mp3'))

    await vi.waitFor(() => expect(getTrackTitle(track('/dir/song.mp3'))).toBe('song'))
  })

  it('strips a trailing extension from the cached title', async () => {
    extractTitle.mockResolvedValue({ title: 'Song.mp3', artist: '' })
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/dir/song.mp3'))

    await vi.waitFor(() => expect(getTrackTitle(track('/dir/song.mp3'))).toBe('Song'))
  })

  it('keeps the title extension when hiding is disabled', async () => {
    configState.titleExtraction!.hideFileExtension = false
    extractTitle.mockResolvedValue({ title: 'Song.mp3', artist: '' })
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/dir/song.mp3'))

    await vi.waitFor(() => expect(getTrackTitle(track('/dir/song.mp3'))).toBe('Song.mp3'))
  })
})

describe('getTrackArtist', () => {
  it('returns the fallback for a missing track', () => {
    expect(useTrackInfo().getTrackArtist(null, '未知艺术家')).toBe('未知艺术家')
    expect(useTrackInfo().getTrackArtist({} as Track)).toBe('')
  })

  it('returns the store artist while processing', () => {
    extractTitle.mockReturnValue(new Promise(() => {}))

    expect(useTrackInfo().getTrackArtist(track('/a.mp3', { artist: 'Store Artist' }))).toBe(
      'Store Artist',
    )
  })

  it('returns the fallback while processing without a store artist', () => {
    extractTitle.mockReturnValue(new Promise(() => {}))

    expect(useTrackInfo().getTrackArtist(track('/a.mp3'), '未知')).toBe('未知')
  })

  it('returns the extracted artist once processing completes', async () => {
    extractTitle.mockResolvedValue({ title: 'T', artist: 'Extracted Artist' })
    const { getTrackArtist } = useTrackInfo()
    getTrackArtist(track('/a.mp3'))

    await vi.waitFor(() => expect(getTrackArtist(track('/a.mp3'))).toBe('Extracted Artist'))
  })
})

describe('processing failures', () => {
  it('falls back to the file name and logs the error', async () => {
    extractTitle.mockRejectedValue(new Error('metadata read failed'))
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/dir/song.mp3'))

    await vi.waitFor(() => expect(getTrackTitle(track('/dir/song.mp3'))).toBe('song'))
    expect(logger.error).toHaveBeenCalled()
  })

  it('does not start a second run while one is in flight', () => {
    extractTitle.mockReturnValue(new Promise(() => {}))
    const { getTrackTitle, getTrackArtist } = useTrackInfo()

    getTrackTitle(track('/a.mp3'))
    getTrackArtist(track('/a.mp3'))
    getTrackTitle(track('/a.mp3'))

    expect(extractTitle).toHaveBeenCalledTimes(1)
  })

  it('passes the title extraction config through with defaults', async () => {
    configState.titleExtraction = undefined
    const { getTrackTitle } = useTrackInfo()
    getTrackTitle(track('/a.mp3'))
    await Promise.resolve()

    expect(extractTitle).toHaveBeenCalledWith(
      '/a.mp3',
      expect.objectContaining({
        preferMetadata: true,
        hideFileExtension: true,
        parseArtistTitle: true,
        separator: '-',
        customSeparators: ['-', '_', '.'],
      }),
    )
  })
})

describe('watchTrack', () => {
  it('prefills the cache from the track metadata on change', async () => {
    const { watchTrack, getTrackTitle } = useTrackInfo()
    const current = ref<Track | null>(null)

    watchTrack(() => current.value)
    current.value = track('/dir/song.mp3', { title: 'Prefilled' })
    await nextTick()

    // 预填项处于 processing,getTrackTitle 直接返回缓存值
    expect(getTrackTitle(current.value)).toBe('Prefilled')
  })

  // ⚠️ 已确认的缺陷(本测试如实记录当前行为,非期望行为):
  // watchTrack 预填时把缓存标记为 processing:true,紧接着调用的 processTrackInfo
  // 因 `getCached(path)?.processing` 为真而 early-return,TitleExtractor.extractTitle
  // 在主流程(App.vue / MiniPlayer.vue 均只走 watchTrack)中永远不会被调用,
  // 标题只能停留在 store 元数据或文件名。修复方式:预填后应立即执行提取。
  it('BUG: prefill marks processing=true, so processTrackInfo skips extraction', async () => {
    const { watchTrack } = useTrackInfo()
    const current = ref<Track | null>(null)

    watchTrack(() => current.value)
    current.value = track('/dir/song.mp3', { title: 'Prefilled' })
    await nextTick()

    expect(extractTitle).not.toHaveBeenCalled()
  })

  it('ignores a null track', async () => {
    const { watchTrack } = useTrackInfo()
    const current = ref<Track | null>(null)

    watchTrack(() => current.value)
    current.value = null
    await nextTick()

    expect(extractTitle).not.toHaveBeenCalled()
  })

  it('skips the prefill and re-runs extraction when a completed entry exists', async () => {
    extractTitle.mockResolvedValue({ title: 'Done', artist: 'A' })
    const { watchTrack, getTrackTitle, processTrackInfo } = useTrackInfo()
    await processTrackInfo('/a.mp3')
    await vi.waitFor(() => expect(getTrackTitle(track('/a.mp3'))).toBe('Done'))

    // 已存在完成态缓存 => 不预填,直接重新提取
    extractTitle.mockResolvedValue({ title: 'Refreshed', artist: 'A' })
    const current = ref<Track | null>(null)
    watchTrack(() => current.value)
    current.value = track('/a.mp3', { title: 'IGNORED' })
    await nextTick()

    await vi.waitFor(() => expect(getTrackTitle(current.value)).toBe('Refreshed'))
  })

  it('can be stopped', async () => {
    const { watchTrack } = useTrackInfo()
    const current = ref<Track | null>(null)

    const stop = watchTrack(() => current.value)
    stop()
    current.value = track('/a.mp3')
    await nextTick()

    expect(extractTitle).not.toHaveBeenCalled()
  })
})

describe('cache management', () => {
  it('shares one cache across instances', async () => {
    extractTitle.mockResolvedValue({ title: 'Shared', artist: 'A' })
    const first = useTrackInfo()
    first.getTrackTitle(track('/a.mp3'))
    await vi.waitFor(() => expect(first.getTrackTitle(track('/a.mp3'))).toBe('Shared'))

    expect(useTrackInfo().getTrackTitle(track('/a.mp3'))).toBe('Shared')
    expect(extractTitle).toHaveBeenCalledTimes(1)
  })

  it('clears a single entry', async () => {
    extractTitle.mockResolvedValue({ title: 'A', artist: 'A' })
    const { getTrackTitle, clearCache, processedTracks } = useTrackInfo()
    getTrackTitle(track('/a.mp3'))
    await vi.waitFor(() => expect(processedTracks.value.has('/a.mp3')).toBe(true))

    clearCache('/a.mp3')

    expect(processedTracks.value.has('/a.mp3')).toBe(false)
    expect(clearCache.bind(null, '')).not.toThrow()
  })

  it('clears every entry', async () => {
    extractTitle.mockResolvedValue({ title: 'A', artist: 'A' })
    const { getTrackTitle, clearAllCache, processedTracks } = useTrackInfo()
    getTrackTitle(track('/a.mp3'))
    getTrackTitle(track('/b.mp3'))
    await vi.waitFor(() => expect(processedTracks.value.size).toBeGreaterThan(0))

    clearAllCache()

    expect(processedTracks.value.size).toBe(0)
  })

  it('exposes processTrackInfo for manual refresh', async () => {
    const { processTrackInfo, processedTracks } = useTrackInfo()
    await processTrackInfo('/manual.mp3')
    expect(processedTracks.value.get('/manual.mp3')).toMatchObject({ processing: false })
  })

  it('evicts the least recently used entry once 200 keys are stored', async () => {
    extractTitle.mockResolvedValue({ title: 'T', artist: 'A' })
    const { processTrackInfo, processedTracks, getTrackTitle } = useTrackInfo()

    // 先触及 /keeper.mp3,使其成为最久未访问项
    await processTrackInfo('/keeper.mp3')
    for (let i = 0; i < 205; i++) {
      await processTrackInfo(`/filler${i}.mp3`)
    }

    expect(processedTracks.value.has('/keeper.mp3')).toBe(false)
    // 缓存规模受 LRU 上限约束
    expect(processedTracks.value.size).toBeLessThanOrEqual(200)
    expect(getTrackTitle(track('/filler204.mp3'))).toBeDefined()
  })
})
