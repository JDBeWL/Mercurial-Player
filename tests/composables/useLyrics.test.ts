// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'
import { reactive, nextTick } from 'vue'

// Mock logger
vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

// 使用 vi.hoisted 避免 TDZ：vi.mock 会被提升到文件顶部，直接引用 const 变量会报错
const { mockFileUtils, mockLyricsParser, mockLyricProviders } = vi.hoisted(() => ({
  // Mock fileUtils - 提供命名导出和默认导出
  mockFileUtils: {
    findLyricsFile: vi.fn(),
    readFile: vi.fn(),
    getFileExtension: vi.fn(),
    getFileNameWithoutExtension: vi.fn(),
    getDirectoryPath: vi.fn(),
    joinPath: vi.fn((...segments: string[]) => segments.filter(Boolean).join('/')),
  },
  // Mock lyricsParser - 提供命名导出和默认导出
  mockLyricsParser: {
    parseAsync: vi.fn(),
  },
  // Mock 多来源歌词获取
  mockLyricProviders: {
    fetchBestLyrics: vi.fn(),
    collectCandidates: vi.fn(),
    buildFinalLyric: vi.fn(),
    LYRIC_PROVIDERS: [],
  },
}))

vi.mock('@/utils/fileUtils', () => ({
  FileUtils: mockFileUtils,
  default: mockFileUtils,
}))

vi.mock('@/utils/lyricsParser', () => ({
  LyricsParser: mockLyricsParser,
  default: mockLyricsParser,
}))

vi.mock('@/utils/lyricProviders', () => ({
  fetchBestLyrics: mockLyricProviders.fetchBestLyrics,
  collectCandidates: mockLyricProviders.collectCandidates,
  buildFinalLyric: mockLyricProviders.buildFinalLyric,
  LYRIC_PROVIDERS: mockLyricProviders.LYRIC_PROVIDERS,
}))

// Mock player store - 使用 reactive 使 watcher 能响应变化
const mockPlayerState = reactive({
  currentTrack: null as {
    path: string
    title?: string
    name?: string
    artist?: string
    duration?: number
  } | null,
  currentTime: 0,
  lyrics: null as unknown,
  currentLyricIndex: -1,
  lyricsOffset: 0,
  // 统一歌词请求守卫 (与真实 player store 的 beginLyricsRequest / isLyricsRequestCurrent 语义一致)
  _lyricsRequestId: 0,
  _activeLyricsRequestId: 0,
  _isDestroyed: false,
  beginLyricsRequest: (): number => {
    mockPlayerState._activeLyricsRequestId = ++mockPlayerState._lyricsRequestId
    return mockPlayerState._activeLyricsRequestId
  },
  isLyricsRequestCurrent: (id: number): boolean =>
    !mockPlayerState._isDestroyed && mockPlayerState._activeLyricsRequestId === id,
})
vi.mock('@/stores/player', () => ({
  usePlayerStore: vi.fn(() => mockPlayerState),
}))

// Mock config store - 使用 reactive 使 watcher 能响应变化
const mockConfigState = reactive({
  lyrics: {
    enableOnlineFetch: false,
    autoSaveOnlineLyrics: true,
    preferTranslation: true,
  },
})
vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => mockConfigState),
}))

import { useLyrics } from '@/composables/useLyrics'
import { mockInvoke, resetTauriMocks } from '../mocks/tauri'
import type { LyricLine } from '@/types'

/** 等待 watcher 的 loadLyrics 完成 */
async function waitForLoadComplete() {
  await nextTick()
  // 刷新微任务队列：mock 异步操作（findLyricsFile/searchAndGetLyrics/parseAsync）
  // 都在微任务中完成，setTimeout(0) 是宏任务，确保所有微任务执行完毕
  await new Promise((resolve) => setTimeout(resolve, 0))
  await nextTick()
}

describe('useLyrics', () => {
  let result: ReturnType<typeof useLyrics>

  beforeEach(() => {
    vi.clearAllMocks()
    resetTauriMocks()
    // 重置 mock store 状态
    mockPlayerState.currentTrack = null
    mockPlayerState.currentTime = 0
    mockPlayerState.lyrics = null
    mockPlayerState.currentLyricIndex = -1
    mockPlayerState.lyricsOffset = 0
    mockPlayerState._lyricsRequestId = 0
    mockPlayerState._activeLyricsRequestId = 0
    mockPlayerState._isDestroyed = false
    mockConfigState.lyrics.enableOnlineFetch = false
    mockConfigState.lyrics.autoSaveOnlineLyrics = true
    mockConfigState.lyrics.preferTranslation = true
    // 重置 mock 实现为默认值
    mockFileUtils.findLyricsFile.mockResolvedValue(null)
    mockFileUtils.readFile.mockResolvedValue('')
    mockFileUtils.getFileExtension.mockReturnValue('lrc')
    mockFileUtils.getFileNameWithoutExtension.mockReturnValue('song')
    mockFileUtils.getDirectoryPath.mockReturnValue('/music')
    mockLyricsParser.parseAsync.mockResolvedValue([])
    mockLyricProviders.fetchBestLyrics.mockResolvedValue(null)
    mockLyricProviders.collectCandidates.mockResolvedValue([])
    mockLyricProviders.buildFinalLyric.mockImplementation(
      (bundle: { lrc: string }, _kind: string, _pref: boolean) => ({
        content: bundle?.lrc || '',
        format: 'lrc' as const,
      }),
    )
    mockInvoke.mockResolvedValue(undefined)
    // 初始化 useLyrics（currentTrack 为 null，immediate watcher 调用 loadLyrics(undefined) 清空状态）
    result = useLyrics()
  })

  afterEach(() => {
    result.cleanup()
    vi.clearAllMocks()
  })

  // ---------- 共享状态 ----------

  describe('共享状态', () => {
    it('多次调用返回相同的共享 ref', () => {
      const result2 = useLyrics()
      expect(result.lyrics).toBe(result2.lyrics)
      expect(result.loading).toBe(result2.loading)
      expect(result.activeIndex).toBe(result2.activeIndex)
      expect(result.lyricsSource).toBe(result2.lyricsSource)
      expect(result.onlineLyricsError).toBe(result2.onlineLyricsError)
    })

    it('初始状态正确', () => {
      expect(result.lyrics.value).toEqual([])
      expect(result.loading.value).toBe(false)
      expect(result.activeIndex.value).toBe(-1)
      expect(result.lyricsSource.value).toBe('local')
      expect(result.onlineLyricsError.value).toBeNull()
    })
  })

  // ---------- loadLyrics ----------

  describe('loadLyrics', () => {
    it('无路径时清空歌词状态', async () => {
      // 先设置一些状态 (store.lyrics 是唯一事实源,sharedLyrics 由同步 watcher 跟随)
      const parsedLyrics: LyricLine[] = [{ time: 1, text: 'old', texts: ['old'] }]
      mockPlayerState.lyrics = parsedLyrics
      await nextTick()
      result.lyricsSource.value = 'online'
      result.onlineLyricsError.value = 'some error'

      await result.loadLyrics(undefined)

      expect(result.lyrics.value).toEqual([])
      expect(result.lyricsSource.value).toBe('local')
      expect(result.onlineLyricsError.value).toBeNull()
    })

    it('本地优先加载歌词', async () => {
      const parsedLyrics: LyricLine[] = [
        { time: 1, text: 'Hello', texts: ['Hello'] },
        { time: 2, text: 'World', texts: ['World'] },
      ]
      mockFileUtils.findLyricsFile.mockResolvedValue('/path/to/lyrics.lrc')
      mockFileUtils.readFile.mockResolvedValue('[00:01.00]Hello\n[00:02.00]World')
      mockFileUtils.getFileExtension.mockReturnValue('lrc')
      mockLyricsParser.parseAsync.mockResolvedValue(parsedLyrics)

      await result.loadLyrics('/music/song.mp3')

      expect(mockFileUtils.findLyricsFile).toHaveBeenCalledWith('/music/song.mp3')
      expect(mockFileUtils.readFile).toHaveBeenCalledWith('/path/to/lyrics.lrc')
      expect(mockLyricsParser.parseAsync).toHaveBeenCalledWith(
        '[00:01.00]Hello\n[00:02.00]World',
        'lrc',
      )
      expect(result.lyrics.value).toBe(parsedLyrics)
      expect(result.lyricsSource.value).toBe('local')
      expect(result.loading.value).toBe(false)
    })

    it('本地无歌词且未启用在线获取时不加载', async () => {
      mockConfigState.lyrics.enableOnlineFetch = false
      mockFileUtils.findLyricsFile.mockResolvedValue(null)

      await result.loadLyrics('/music/no-lyrics.mp3')

      expect(result.lyrics.value).toEqual([])
      expect(result.lyricsSource.value).toBe('local')
      expect(result.loading.value).toBe(false)
      expect(mockLyricProviders.fetchBestLyrics).not.toHaveBeenCalled()
    })

    it('本地无歌词时尝试在线获取', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = false
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]Online lyrics',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      const parsedLyrics: LyricLine[] = [
        { time: 1, text: 'Online lyrics', texts: ['Online lyrics'] },
      ]
      mockLyricsParser.parseAsync.mockResolvedValue(parsedLyrics)

      // 设置 currentTrack（fetchOnlineLyrics 需要）
      mockPlayerState.currentTrack = {
        path: '/music/online-test.mp3',
        title: 'Test Song',
        name: 'test.mp3',
        artist: 'Artist',
        duration: 200,
      }
      // 等待 watcher 的 loadLyrics 完成
      await waitForLoadComplete()

      // 直接调用 loadLyrics
      await result.loadLyrics('/music/online-test.mp3')

      expect(mockLyricProviders.fetchBestLyrics).toHaveBeenCalled()
      expect(result.lyrics.value).toBe(parsedLyrics)
      expect(result.lyricsSource.value).toBe('online')
      expect(result.loading.value).toBe(false)
    })

    it('在线获取失败时设置错误信息', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockLyricProviders.fetchBestLyrics.mockRejectedValue(new Error('network error'))

      mockPlayerState.currentTrack = {
        path: '/music/error-test.mp3',
        title: 'Error Song',
        name: 'error.mp3',
      }
      await waitForLoadComplete()

      await result.loadLyrics('/music/error-test.mp3')

      expect(result.onlineLyricsError.value).toContain('network error')
      expect(result.loading.value).toBe(false)
    })

    it('启用翻译时合并翻译歌词', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.preferTranslation = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = false
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]Hello / 你好',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      mockLyricsParser.parseAsync.mockResolvedValue([
        { time: 1, text: 'Hello / 你好', texts: ['Hello / 你好'] },
      ])

      mockPlayerState.currentTrack = {
        path: '/music/translation-test.mp3',
        title: 'Translation',
        name: 'translation.mp3',
      }
      await waitForLoadComplete()

      await result.loadLyrics('/music/translation-test.mp3')

      expect(mockLyricProviders.fetchBestLyrics).toHaveBeenCalled()
      expect(result.lyrics.value.length).toBeGreaterThan(0)
    })

    it('在线歌词缓存避免重复请求', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = false // 避免保存后删除缓存
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]Cached',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      mockLyricsParser.parseAsync.mockResolvedValue([
        { time: 1, text: 'Cached', texts: ['Cached'] },
      ])

      mockPlayerState.currentTrack = {
        path: '/music/cache-test.mp3',
        title: 'Cache',
        name: 'cache.mp3',
      }

      // 等待 watcher 的 loadLyrics 完成（第一次获取）
      await waitForLoadComplete()

      const initialCallCount = mockLyricProviders.fetchBestLyrics.mock.calls.length
      expect(initialCallCount).toBeGreaterThan(0)

      // 第二次调用 - 应使用缓存
      await result.loadLyrics('/music/cache-test.mp3')

      // fetchBestLyrics 不应再次调用
      expect(mockLyricProviders.fetchBestLyrics.mock.calls.length).toBe(initialCallCount)
    })

    it('自动保存在线歌词到本地后来源变为 local', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = true
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]Save test',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      mockLyricsParser.parseAsync.mockResolvedValue([
        { time: 1, text: 'Save test', texts: ['Save test'] },
      ])
      mockFileUtils.getFileNameWithoutExtension.mockReturnValue('save-test')
      mockFileUtils.getDirectoryPath.mockReturnValue('/music')

      mockPlayerState.currentTrack = {
        path: '/music/save-test.mp3',
        title: 'Save Test',
        name: 'save-test.mp3',
      }
      await waitForLoadComplete()

      await result.loadLyrics('/music/save-test.mp3')

      expect(mockInvoke).toHaveBeenCalledWith('write_lyrics_file', {
        path: '/music/save-test.lrc',
        content: '[00:01.00]Save test',
      })
      expect(result.lyricsSource.value).toBe('local')
    })
  })

  // ---------- fetchAndSaveLyrics ----------

  describe('fetchAndSaveLyrics', () => {
    it('无当前曲目时返回 false', async () => {
      mockPlayerState.currentTrack = null
      const success = await result.fetchAndSaveLyrics()
      expect(success).toBe(false)
    })

    it('成功获取并保存在线歌词', async () => {
      mockConfigState.lyrics.enableOnlineFetch = false // 防止 watcher 的 loadLyrics 干扰
      mockPlayerState.currentTrack = {
        path: '/music/fetch-save.mp3',
        title: 'Fetch Save',
        name: 'fetch-save.mp3',
        artist: 'Artist',
        duration: 180,
      }
      await waitForLoadComplete()

      const parsedLyrics: LyricLine[] = [{ time: 1, text: 'Fetched', texts: ['Fetched'] }]
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]Fetched',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      mockLyricsParser.parseAsync.mockResolvedValue(parsedLyrics)
      mockConfigState.lyrics.autoSaveOnlineLyrics = true
      mockFileUtils.getFileNameWithoutExtension.mockReturnValue('fetch-save')
      mockFileUtils.getDirectoryPath.mockReturnValue('/music')

      const success = await result.fetchAndSaveLyrics()

      expect(success).toBe(true)
      expect(result.lyrics.value).toBe(parsedLyrics)
      expect(mockInvoke).toHaveBeenCalledWith('write_lyrics_file', {
        path: '/music/fetch-save.lrc',
        content: '[00:01.00]Fetched',
      })
    })

    it('不自动保存时歌词来源为 online', async () => {
      mockConfigState.lyrics.enableOnlineFetch = false
      mockPlayerState.currentTrack = {
        path: '/music/no-save.mp3',
        title: 'No Save',
        name: 'no-save.mp3',
      }
      await waitForLoadComplete()

      mockConfigState.lyrics.autoSaveOnlineLyrics = false
      mockLyricProviders.fetchBestLyrics.mockResolvedValue({
        content: '[00:01.00]No save',
        format: 'lrc' as const,
        source: 'netease',
        candidates: [],
      })
      mockLyricsParser.parseAsync.mockResolvedValue([
        { time: 1, text: 'No save', texts: ['No save'] },
      ])

      const success = await result.fetchAndSaveLyrics()

      expect(success).toBe(true)
      expect(result.lyricsSource.value).toBe('online')
      expect(mockInvoke).not.toHaveBeenCalledWith('write_lyrics_file', expect.anything())
    })

    it('在线获取失败时返回 false', async () => {
      mockConfigState.lyrics.enableOnlineFetch = false
      mockPlayerState.currentTrack = {
        path: '/music/fail.mp3',
        title: 'Fail',
        name: 'fail.mp3',
      }
      await waitForLoadComplete()

      mockLyricProviders.fetchBestLyrics.mockResolvedValue(null)

      const success = await result.fetchAndSaveLyrics()

      expect(success).toBe(false)
      expect(result.loading.value).toBe(false)
    })

    it('获取异常时返回 false 并设置错误信息', async () => {
      mockConfigState.lyrics.enableOnlineFetch = false
      mockPlayerState.currentTrack = {
        path: '/music/exception.mp3',
        title: 'Exception',
        name: 'exception.mp3',
      }
      await waitForLoadComplete()

      mockLyricProviders.fetchBestLyrics.mockRejectedValue(new Error('API error'))

      const success = await result.fetchAndSaveLyrics()

      expect(success).toBe(false)
      expect(result.onlineLyricsError.value).toContain('API error')
    })
  })

  // ---------- fetchCandidates / applyCandidate ----------

  describe('fetchCandidates / applyCandidate', () => {
    it('无当前曲目时 fetchCandidates 返回空数组', async () => {
      mockPlayerState.currentTrack = null
      const candidates = await result.fetchCandidates()
      expect(candidates).toEqual([])
      // 无曲目时不进入 collectCandidates
      expect(mockLyricProviders.collectCandidates).not.toHaveBeenCalled()
    })

    it('fetchCandidates 聚合各来源候选', async () => {
      mockPlayerState.currentTrack = {
        path: '/music/pick.mp3',
        title: 'Pick',
        name: 'pick.mp3',
      }
      await waitForLoadComplete()
      const cands = [
        {
          id: '1',
          title: 'Pick',
          artist: '',
          album: '',
          duration_ms: 0,
          bundle: { lrc: '[00:01.00]A', tlyric: '', romalrc: '' },
          provider: 'netease' as const,
          method: 'webapi',
        },
      ]
      mockLyricProviders.collectCandidates.mockResolvedValue(cands)
      const resultList = await result.fetchCandidates()
      expect(resultList).toEqual(cands)
    })

    it('applyCandidate 显示并写入本地文件', async () => {
      mockPlayerState.currentTrack = {
        path: '/music/apply.mp3',
        title: 'Apply',
        name: 'apply.mp3',
      }
      await waitForLoadComplete()
      mockConfigState.lyrics.autoSaveOnlineLyrics = true
      mockLyricsParser.parseAsync.mockResolvedValue([
        { time: 1, text: 'Applied', texts: ['Applied'] },
      ])
      mockFileUtils.getFileNameWithoutExtension.mockReturnValue('apply')
      mockFileUtils.getDirectoryPath.mockReturnValue('/music')
      const candidate = {
        id: '1',
        title: 'Apply',
        artist: '',
        album: '',
        duration_ms: 0,
        bundle: { lrc: '[00:01.00]Applied' },
        provider: 'netease' as const,
        method: 'webapi',
      }
      const ok = await result.applyCandidate(candidate, 'auto')
      expect(ok).toBe(true)
      expect(result.lyricsSource.value).toBe('local')
      expect(mockInvoke).toHaveBeenCalledWith('write_lyrics_file', {
        path: '/music/apply.lrc',
        content: '[00:01.00]Applied',
      })
    })

    it('无当前曲目时 applyCandidate 返回 false', async () => {
      mockPlayerState.currentTrack = null
      const candidate = {
        id: '1',
        title: 'x',
        artist: '',
        album: '',
        duration_ms: 0,
        bundle: { lrc: '[00:01.00]x' },
        provider: 'netease' as const,
        method: 'webapi',
      }
      const ok = await result.applyCandidate(candidate, 'auto')
      expect(ok).toBe(false)
    })
  })

  // ---------- cleanup ----------

  describe('cleanup', () => {
    it('重置所有共享状态', () => {
      result.lyrics.value = [{ time: 1, text: 'test', texts: ['test'] }]
      result.loading.value = true
      result.activeIndex.value = 5
      result.lyricsSource.value = 'online'
      result.onlineLyricsError.value = 'error'

      result.cleanup()

      expect(result.lyrics.value).toEqual([])
      expect(result.loading.value).toBe(false)
      expect(result.activeIndex.value).toBe(-1)
      expect(result.lyricsSource.value).toBe('local')
      expect(result.onlineLyricsError.value).toBeNull()
    })

    it('cleanup 后重新调用 useLyrics 可以重新初始化', () => {
      result.cleanup()
      // 重新初始化
      const result2 = useLyrics()
      expect(result2.lyrics.value).toEqual([])
      expect(result2.loading.value).toBe(false)
      expect(result2.activeIndex.value).toBe(-1)
      // result2 和 result 共享同一个 ref（模块级共享状态）
      expect(result2.lyrics).toBe(result.lyrics)
    })
  })
})
