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
const { mockFileUtils, mockLyricsParser, mockNeteaseApi } = vi.hoisted(() => ({
  // Mock fileUtils - 提供命名导出和默认导出
  mockFileUtils: {
    findLyricsFile: vi.fn(),
    readFile: vi.fn(),
    getFileExtension: vi.fn(),
    getFileNameWithoutExtension: vi.fn(),
    getDirectoryPath: vi.fn(),
  },
  // Mock lyricsParser - 提供命名导出和默认导出
  mockLyricsParser: {
    parseAsync: vi.fn(),
  },
  // Mock neteaseApi
  mockNeteaseApi: {
    searchAndGetLyrics: vi.fn(),
    mergeLyrics: vi.fn(),
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

vi.mock('@/utils/neteaseApi', () => ({
  neteaseApi: mockNeteaseApi,
}))

// Mock player store - 使用 reactive 使 watcher 能响应变化
const mockPlayerState = reactive({
  currentTrack: null as { path: string; title?: string; name?: string; artist?: string; duration?: number } | null,
  currentTime: 0,
  lyrics: null as unknown,
  currentLyricIndex: -1,
  lyricsOffset: 0,
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
import type { LyricLine, Track } from '@/types'

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
    mockNeteaseApi.searchAndGetLyrics.mockResolvedValue(null)
    mockNeteaseApi.mergeLyrics.mockReturnValue('')
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
      // 先设置一些状态
      const parsedLyrics: LyricLine[] = [{ time: 1, text: 'old', texts: ['old'] }]
      result.lyrics.value = parsedLyrics
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
      expect(mockLyricsParser.parseAsync).toHaveBeenCalledWith('[00:01.00]Hello\n[00:02.00]World', 'lrc')
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
      expect(mockNeteaseApi.searchAndGetLyrics).not.toHaveBeenCalled()
    })

    it('本地无歌词时尝试在线获取', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = false
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]Online lyrics',
        tlyric: '',
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

      expect(mockNeteaseApi.searchAndGetLyrics).toHaveBeenCalledWith('Test Song', 'Artist', 200000)
      expect(result.lyrics.value).toBe(parsedLyrics)
      expect(result.lyricsSource.value).toBe('online')
      expect(result.loading.value).toBe(false)
    })

    it('在线获取失败时设置错误信息', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockNeteaseApi.searchAndGetLyrics.mockRejectedValue(new Error('network error'))

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
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]Hello',
        tlyric: '[00:01.00]你好',
      })
      mockNeteaseApi.mergeLyrics.mockReturnValue('[00:01.00]Hello / 你好')
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

      expect(mockNeteaseApi.mergeLyrics).toHaveBeenCalledWith('[00:01.00]Hello', '[00:01.00]你好')
    })

    it('在线歌词缓存避免重复请求', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = false // 避免保存后删除缓存
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]Cached',
        tlyric: '',
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

      const initialCallCount = mockNeteaseApi.searchAndGetLyrics.mock.calls.length
      expect(initialCallCount).toBeGreaterThan(0)

      // 第二次调用 - 应使用缓存
      await result.loadLyrics('/music/cache-test.mp3')

      // searchAndGetLyrics 不应再次调用
      expect(mockNeteaseApi.searchAndGetLyrics.mock.calls.length).toBe(initialCallCount)
    })

    it('自动保存在线歌词到本地后来源变为 local', async () => {
      mockConfigState.lyrics.enableOnlineFetch = true
      mockConfigState.lyrics.autoSaveOnlineLyrics = true
      mockFileUtils.findLyricsFile.mockResolvedValue(null)
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]Save test',
        tlyric: '',
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

      const parsedLyrics: LyricLine[] = [
        { time: 1, text: 'Fetched', texts: ['Fetched'] },
      ]
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]Fetched',
        tlyric: '',
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
      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue({
        lrc: '[00:01.00]No save',
        tlyric: '',
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

      mockNeteaseApi.searchAndGetLyrics.mockResolvedValue(null)

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

      mockNeteaseApi.searchAndGetLyrics.mockRejectedValue(new Error('API error'))

      const success = await result.fetchAndSaveLyrics()

      expect(success).toBe(false)
      expect(result.onlineLyricsError.value).toContain('API error')
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
