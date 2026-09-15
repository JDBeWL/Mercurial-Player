// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, watch } from 'vue'

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

vi.mock('@/utils/fileUtils', () => ({
  default: {
    fileExists: vi.fn(async () => true),
    findLyricsFile: vi.fn(async () => null),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  default: { parseAsync: vi.fn(async () => []) },
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => ({
    audio: { volume: 0.5, exclusiveMode: false, fadeEnabled: true },
    saveConfigNow: vi.fn().mockResolvedValue(undefined),
    saveConfig: vi.fn(),
  })),
}))

const loadPlaylistsFromCache = vi.fn().mockResolvedValue(false)
vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: vi.fn(() => ({
    currentPlaylist: null,
    playlists: [],
    loadPlaylistsFromCache,
    selectPlaylist: vi.fn(),
  })),
}))

import { setActivePinia, createPinia, storeToRefs } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { usePlayerStore } from '@/stores/player'
import { resumeLastSession } from '@/stores/playerSession'

const invokeMock = vi.mocked(invoke)

const COVER = '/cache/covers/a.jpg'

function resolveCoverCommand(command: string): unknown {
  if (command === 'resume_last_session') {
    return {
      resumed: true,
      status: 'resumed',
      trackPath: '/music/a.mp3',
      trackTitle: 'A',
      trackArtist: 'Artist A',
      durationSecs: 100,
      positionSecs: 10,
      playlistName: null,
      // 会话快照不含 coverPath(见 saveLastSessionNow)
      playlistTracks: [
        {
          path: '/music/a.mp3',
          title: 'A',
          artist: 'Artist A',
          album: null,
          duration: 100,
          bitrate: 320,
          sampleRate: 44100,
          channels: 2,
          bitDepth: 16,
          format: 'mp3',
        },
      ],
    }
  }
  if (command === 'get_track_cover_path') return COVER
  return null
}

describe('恢复会话后的当前曲目封面', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    invokeMock.mockImplementation(async (command: string) => resolveCoverCommand(command))
    loadPlaylistsFromCache.mockResolvedValue(false)
  })

  it('恢复后在 currentTrack 上写入 coverPath,且响应式可观察到(主界面/MiniPlayer 用)', async () => {
    const store = usePlayerStore()

    // 模拟 App.vue 的 currentTrackCover 计算属性:先读一次建立依赖
    const cover = computed(() => store.currentTrack?.coverPath)
    expect(cover.value).toBeUndefined()

    await resumeLastSession(store)

    await vi.waitFor(() => expect(store.currentTrack?.coverPath).toBe(COVER))
    expect(cover.value).toBe(COVER)
  })

  it('经 storeToRefs(与 App.vue 一致)也能观察到封面变化', async () => {
    const store = usePlayerStore()
    const { currentTrack } = storeToRefs(store)

    // App.vue: computed(() => currentTrack.value?.coverPath)
    const cover = computed(() => currentTrack.value?.coverPath)
    expect(cover.value).toBeUndefined()

    await resumeLastSession(store)

    await vi.waitFor(() => expect(cover.value).toBe(COVER))
  })

  it('封面写入必须真正触发响应式(而非只改了对象字段)', async () => {
    const store = usePlayerStore()
    // watch 只在依赖真正触发时回调;computed 的惰性重算是掩盖不了漏触发的
    const seen: Array<string | undefined> = []
    const stop = watch(
      () => store.currentTrack?.coverPath,
      (v) => seen.push(v),
      { flush: 'sync' },
    )

    await resumeLastSession(store)
    await vi.waitFor(() => expect(store.currentTrack?.coverPath).toBe(COVER))
    stop()

    expect(seen.at(-1)).toBe(COVER)
  })

  it('恢复后的播放列表可被重新赋值并触发响应式(playlist 已 markRaw)', async () => {
    const store = usePlayerStore()
    const length = computed(() => store.playlist.length)
    expect(length.value).toBe(0)

    await resumeLastSession(store)

    await vi.waitFor(() => expect(store.playlist).toHaveLength(1))
    expect(length.value).toBe(1)
  })
})
