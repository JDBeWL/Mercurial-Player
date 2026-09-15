// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Track } from '@/types'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: vi.fn(),
  unregisterAll: vi.fn(),
  isRegistered: vi.fn(async () => false),
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

// player.ts 用默认导入,useLyrics 用具名导入,两侧必须指向同一份 mock
const { findLyricsFile, fileUtilsMock } = vi.hoisted(() => {
  const findLyricsFile = vi.fn(async (): Promise<string | null> => '/music/a.lrc')
  const readFile = vi.fn(async () => '[00:00.00]line one\n[00:10.00]line two')
  const fileUtilsMock = {
    fileExists: vi.fn(async () => true),
    findLyricsFile,
    readFile,
    getFileExtension: vi.fn(() => 'lrc'),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileNameWithoutExtension: vi.fn((p: string) =>
      (p.split(/[\\/]/).pop() || p).replace(/\.[^.]+$/, ''),
    ),
    getDirectoryPath: vi.fn((p: string) => p.slice(0, p.lastIndexOf('/'))),
    joinPath: vi.fn((dir: string, name: string) => `${dir}/${name}`),
  }
  return { findLyricsFile, fileUtilsMock }
})

vi.mock('@/utils/fileUtils', () => ({ FileUtils: fileUtilsMock, default: fileUtilsMock }))

const PARSED_LYRICS = [
  { time: 0, text: 'line one' },
  { time: 10, text: 'line two' },
]
vi.mock('@/utils/lyricsParser', () => ({
  LyricsParser: { parseAsync: vi.fn(async () => PARSED_LYRICS) },
  findLyricIndex: vi.fn(() => 0),
}))

vi.mock('@/utils/neteaseApi', () => ({
  neteaseApi: { searchAndGetLyrics: vi.fn(async () => null) },
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => ({
    audio: { volume: 0.5, exclusiveMode: false, fadeEnabled: true },
    lyrics: { enableOnlineFetch: false, autoSaveOnlineLyrics: false },
    saveConfigNow: vi.fn().mockResolvedValue(undefined),
    saveConfig: vi.fn(),
  })),
}))

vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: vi.fn(() => ({
    currentPlaylist: null,
    playlists: [],
    loadPlaylistsFromCache: vi.fn().mockResolvedValue(false),
    selectPlaylist: vi.fn(),
  })),
}))

import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { usePlayerStore } from '@/stores/player'
import { useLyrics } from '@/composables/useLyrics'

const invokeMock = vi.mocked(invoke)
const findLyricsFileMock = vi.mocked(findLyricsFile)

const trackA: Track = { path: '/music/a.mp3', name: 'a', title: 'A', duration: 100 } as Track
const trackB: Track = { path: '/music/b.mp3', name: 'b', title: 'B', duration: 100 } as Track

interface LyricsApi {
  lyrics: { value: Array<{ time: number; text: string }> }
  cleanup: () => void
}

describe('歌词在「重新播放同一首歌」时不应丢失', () => {
  let lyricsApi: LyricsApi

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined as never)
    findLyricsFileMock.mockResolvedValue('/music/a.lrc')
    // 初始化共享 watcher(与真实组件一致:唯一加载入口是 currentTrack.path 的 watcher)
    lyricsApi = useLyrics() as unknown as LyricsApi
  })

  afterEach(() => {
    // 重置模块级 isInitialized,让下个用例重新绑定新的 pinia store
    lyricsApi.cleanup()
  })

  it('单曲循环/重复点击当前曲目:path 未变,歌词必须保持而不是被清空', async () => {
    const store = usePlayerStore()

    await store.playTrack(trackA)
    await vi.waitFor(() => expect(store.lyrics).not.toBeNull())
    expect(lyricsApi.lyrics.value.length).toBeGreaterThan(0)

    // 重新播放同一首(path 不变 → loadLyrics 的 watcher 不会重新触发)
    await store.playTrack(trackA)

    expect(store.lyrics).not.toBeNull()
    expect(lyricsApi.lyrics.value.length).toBeGreaterThan(0)
  })

  it('切到另一首时仍要清空上一首的歌词', async () => {
    const store = usePlayerStore()

    await store.playTrack(trackA)
    await vi.waitFor(() => expect(store.lyrics).not.toBeNull())

    // B 没有本地歌词且未开在线获取 → 应清空而不是继续显示 A 的歌词
    findLyricsFileMock.mockResolvedValue(null)
    await store.playTrack(trackB)

    await vi.waitFor(() => expect(store.lyrics).toBeNull())
    expect(lyricsApi.lyrics.value.length).toBe(0)
  })
})
