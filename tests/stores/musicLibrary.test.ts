// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

// Mock logger
vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

// Mock config store - 返回可写的 mock 对象
const mockConfigStore = {
  musicDirectories: [] as string[],
  playlist: { sortOrder: 'asc' as 'asc' | 'desc' },
}
vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => mockConfigStore),
}))

// Mock player store
const mockPlayerStore = {
  playlist: [] as Array<{ path: string }>,
}
vi.mock('@/stores/player', () => ({
  usePlayerStore: vi.fn(() => mockPlayerStore),
}))

import { setActivePinia, createPinia } from 'pinia'
import { useMusicLibraryStore } from '@/stores/musicLibrary'
import { invoke } from '@tauri-apps/api/core'
import { mockStoreGet, mockStoreSet, mockStoreSave, resetTauriMocks } from '../mocks/tauri'
import type { Track, Playlist } from '@/types'

const invokeMock = vi.mocked(invoke)

/** 创建测试用 Track */
function makeTrack(path: string, title?: string): Track {
  return { path, name: path, title: title ?? path, duration: 100 }
}

/** 创建测试用 Playlist */
function makePlaylist(name: string, files: Track[]): Playlist {
  return { name, files, totalFiles: files.length }
}

describe('useMusicLibraryStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    resetTauriMocks()
    // 重置 mock store 状态
    mockConfigStore.musicDirectories = []
    mockConfigStore.playlist.sortOrder = 'asc'
    mockPlayerStore.playlist = []
    // 默认 invoke 和 store 方法返回值
    // get_data_dir 需返回数据目录,store 文件路径拼接(join)依赖它
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'resolve_data_file') {
        return Promise.resolve('C:\mock\data\store.json')
      }
      return Promise.resolve(undefined)
    })
    mockStoreGet.mockResolvedValue(null)
    mockStoreSet.mockResolvedValue(undefined)
    mockStoreSave.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  // ---------- 状态初始化 ----------

  describe('default state', () => {
    it('初始状态正确', () => {
      const store = useMusicLibraryStore()
      expect(store.musicFolders).toEqual([])
      expect(store.playlists).toEqual([])
      expect(store.currentPlaylist).toBeNull()
      expect(store.isLoading).toBe(false)
      expect(store.isBackgroundRefreshing).toBe(false)
      expect(store.loadingProgress).toBe(0)
      expect(store.error).toBeNull()
      expect(store.stats).toEqual({
        totalDirectories: 0,
        totalAudioFiles: 0,
        totalPlaylists: 0,
        maxDepth: 0,
      })
      expect(store._sortedPlaylists).toBeInstanceOf(Set)
      expect(store._sortedPlaylists.size).toBe(0)
      expect(store._loadedFromCache).toBe(false)
    })
  })

  // ---------- loadMusicFolders ----------

  describe('loadMusicFolders', () => {
    it('成功加载音乐文件夹列表', async () => {
      const folders = ['/music/folder1', '/music/folder2']
      invokeMock.mockResolvedValue(folders)
      const store = useMusicLibraryStore()
      const result = await store.loadMusicFolders()
      expect(result.success).toBe(true)
      expect(result.message).toContain('successfully')
      expect(store.musicFolders).toEqual(folders)
      expect(invokeMock).toHaveBeenCalledWith('get_music_directories')
    })

    it('invoke 失败时返回错误信息', async () => {
      invokeMock.mockRejectedValue(new Error('network error'))
      const store = useMusicLibraryStore()
      const result = await store.loadMusicFolders()
      expect(result.success).toBe(false)
      expect(result.message).toContain('network error')
    })
  })

  // ---------- addMusicFolder ----------

  describe('addMusicFolder', () => {
    it('成功添加文件夹并同步到 configStore', async () => {
      const updated = ['/music/folder1', '/music/new']
      invokeMock.mockResolvedValue(updated)
      const store = useMusicLibraryStore()
      const result = await store.addMusicFolder('/music/new')
      expect(result.success).toBe(true)
      expect(store.musicFolders).toEqual(updated)
      expect(mockConfigStore.musicDirectories).toEqual(updated)
      expect(invokeMock).toHaveBeenCalledWith('add_music_directory', { path: '/music/new' })
    })

    it('invoke 失败时返回错误信息', async () => {
      invokeMock.mockRejectedValue(new Error('permission denied'))
      const store = useMusicLibraryStore()
      const result = await store.addMusicFolder('/music/new')
      expect(result.success).toBe(false)
      expect(result.message).toContain('permission denied')
    })
  })

  // ---------- removeMusicFolder ----------

  describe('removeMusicFolder', () => {
    it('成功移除文件夹并同步到 configStore', async () => {
      const updated = ['/music/folder1']
      invokeMock.mockResolvedValue(updated)
      const store = useMusicLibraryStore()
      store.currentPlaylist = makePlaylist('test', [makeTrack('/music/folder1/song.mp3')])
      const result = await store.removeMusicFolder('/music/folder2')
      expect(result.success).toBe(true)
      expect(store.musicFolders).toEqual(updated)
      expect(mockConfigStore.musicDirectories).toEqual(updated)
      // folder2 不匹配任何文件，currentPlaylist 不受影响
      expect(store.currentPlaylist).not.toBeNull()
    })

    it('移除文件夹时清空受影响的 currentPlaylist', async () => {
      invokeMock.mockResolvedValue([])
      const store = useMusicLibraryStore()
      store.currentPlaylist = makePlaylist('test', [
        makeTrack('/music/folder1/song1.mp3'),
        makeTrack('/music/folder2/song2.mp3'),
      ])
      await store.removeMusicFolder('/music/folder1')
      expect(store.currentPlaylist).toBeNull()
    })

    it('normalizePath: 反斜杠文件夹路径与正斜杠文件路径匹配', async () => {
      invokeMock.mockResolvedValue([])
      const store = useMusicLibraryStore()
      // 文件路径用正斜杠，文件夹路径用反斜杠
      store.currentPlaylist = makePlaylist('test', [makeTrack('C:/Music/album/song.mp3')])
      await store.removeMusicFolder('C:\\Music\\album')
      expect(store.currentPlaylist).toBeNull()
    })

    it('normalizePath: 大小写不敏感匹配', async () => {
      invokeMock.mockResolvedValue([])
      const store = useMusicLibraryStore()
      store.currentPlaylist = makePlaylist('test', [makeTrack('/Music/Folder/song.mp3')])
      await store.removeMusicFolder('/music/folder')
      expect(store.currentPlaylist).toBeNull()
    })

    it('normalizePath: 不匹配的路径不清空 currentPlaylist', async () => {
      invokeMock.mockResolvedValue([])
      const store = useMusicLibraryStore()
      store.currentPlaylist = makePlaylist('test', [makeTrack('/other/path/song.mp3')])
      await store.removeMusicFolder('/music/folder')
      expect(store.currentPlaylist).not.toBeNull()
    })

    it('currentPlaylist 为 null 时正常执行', async () => {
      invokeMock.mockResolvedValue([])
      const store = useMusicLibraryStore()
      store.currentPlaylist = null
      const result = await store.removeMusicFolder('/music/folder')
      expect(result.success).toBe(true)
    })

    it('invoke 失败时返回错误信息', async () => {
      invokeMock.mockRejectedValue(new Error('remove failed'))
      const store = useMusicLibraryStore()
      const result = await store.removeMusicFolder('/music/folder1')
      expect(result.success).toBe(false)
      expect(result.message).toContain('remove failed')
    })
  })

  // ---------- selectPlaylist (惰性排序) ----------

  describe('selectPlaylist', () => {
    it('选择播放列表并设置为 currentPlaylist', () => {
      const store = useMusicLibraryStore()
      const playlist = makePlaylist('test', [makeTrack('/a.mp3', 'A')])
      store.selectPlaylist(playlist)
      expect(store.currentPlaylist).toStrictEqual(playlist)
    })

    it('首次选择时触发惰性排序 (asc 升序)', () => {
      const store = useMusicLibraryStore()
      mockConfigStore.playlist.sortOrder = 'asc'
      const playlist = makePlaylist('test', [
        makeTrack('/c.mp3', 'C'),
        makeTrack('/a.mp3', 'A'),
        makeTrack('/b.mp3', 'B'),
      ])
      store.selectPlaylist(playlist)
      expect(playlist.files[0].title).toBe('A')
      expect(playlist.files[1].title).toBe('B')
      expect(playlist.files[2].title).toBe('C')
      expect(store._sortedPlaylists.has('test')).toBe(true)
    })

    it('desc 降序排序', () => {
      const store = useMusicLibraryStore()
      mockConfigStore.playlist.sortOrder = 'desc'
      const playlist = makePlaylist('test', [
        makeTrack('/a.mp3', 'A'),
        makeTrack('/c.mp3', 'C'),
        makeTrack('/b.mp3', 'B'),
      ])
      store.selectPlaylist(playlist)
      expect(playlist.files[0].title).toBe('C')
      expect(playlist.files[1].title).toBe('B')
      expect(playlist.files[2].title).toBe('A')
    })

    it('重复选择同一播放列表不会重复排序', () => {
      const store = useMusicLibraryStore()
      mockConfigStore.playlist.sortOrder = 'asc'
      const playlist = makePlaylist('test', [makeTrack('/c.mp3', 'C'), makeTrack('/a.mp3', 'A')])
      store.selectPlaylist(playlist)
      expect(playlist.files[0].title).toBe('A')
      // 手动打乱顺序后再次选择
      playlist.files.reverse()
      store.selectPlaylist(playlist)
      // 不会重新排序，顺序保持打乱后的状态
      expect(playlist.files[0].title).toBe('C')
    })

    it('空文件列表的播放列表直接标记为已排序', () => {
      const store = useMusicLibraryStore()
      const playlist = makePlaylist('empty', [])
      store.selectPlaylist(playlist)
      expect(store._sortedPlaylists.has('empty')).toBe(true)
    })

    it('使用 title 回退到 name 进行排序', () => {
      const store = useMusicLibraryStore()
      mockConfigStore.playlist.sortOrder = 'asc'
      // title 为 undefined，回退到 name
      const playlist = makePlaylist('test', [
        { path: '/c.mp3', name: 'Charlie' },
        { path: '/a.mp3', name: 'Alice' },
      ])
      store.selectPlaylist(playlist)
      expect(playlist.files[0].name).toBe('Alice')
      expect(playlist.files[1].name).toBe('Charlie')
    })
  })

  // ---------- removeFileFromPlaylist ----------

  describe('removeFileFromPlaylist', () => {
    it('从 currentPlaylist 中移除指定文件', () => {
      const store = useMusicLibraryStore()
      const playlist = makePlaylist('test', [
        makeTrack('/a.mp3'),
        makeTrack('/b.mp3'),
        makeTrack('/c.mp3'),
      ])
      store.currentPlaylist = playlist
      store.removeFileFromPlaylist('/b.mp3')
      expect(playlist.files).toHaveLength(2)
      expect(playlist.files.map((f) => f.path)).toEqual(['/a.mp3', '/c.mp3'])
      expect(playlist.totalFiles).toBe(2)
    })

    it('currentPlaylist 为 null 时无操作', () => {
      const store = useMusicLibraryStore()
      store.currentPlaylist = null
      expect(() => store.removeFileFromPlaylist('/a.mp3')).not.toThrow()
    })

    it('文件不存在时无操作', () => {
      const store = useMusicLibraryStore()
      const playlist = makePlaylist('test', [makeTrack('/a.mp3')])
      store.currentPlaylist = playlist
      store.removeFileFromPlaylist('/nonexistent.mp3')
      expect(playlist.files).toHaveLength(1)
      expect(playlist.totalFiles).toBe(1)
    })
  })

  // ---------- 缓存逻辑 ----------

  describe('loadPlaylistsFromCache', () => {
    it('成功从缓存加载播放列表', async () => {
      const cachedPlaylists = [
        {
          name: 'playlist1',
          files: [{ path: '/a.mp3', name: 'a', title: 'A', duration: 100 }],
        },
      ]
      mockStoreGet.mockResolvedValue({
        playlists: cachedPlaylists,
        timestamp: Date.now(),
      })
      const store = useMusicLibraryStore()
      const result = await store.loadPlaylistsFromCache()
      expect(result).toBe(true)
      expect(store.playlists).toHaveLength(1)
      expect(store.playlists[0].name).toBe('playlist1')
      expect(store.playlists[0].files).toHaveLength(1)
      expect(store._loadedFromCache).toBe(true)
    })

    it('缓存为空时返回 false', async () => {
      mockStoreGet.mockResolvedValue(null)
      const store = useMusicLibraryStore()
      const result = await store.loadPlaylistsFromCache()
      expect(result).toBe(false)
      expect(store._loadedFromCache).toBe(false)
    })

    it('缓存中播放列表为空数组时返回 false', async () => {
      mockStoreGet.mockResolvedValue({
        playlists: [],
        timestamp: Date.now(),
      })
      const store = useMusicLibraryStore()
      const result = await store.loadPlaylistsFromCache()
      expect(result).toBe(false)
    })

    it('缓存过期(超过7天)时返回 false', async () => {
      mockStoreGet.mockResolvedValue({
        playlists: [{ name: 'old', files: [] }],
        timestamp: Date.now() - 8 * 24 * 60 * 60 * 1000, // 8天前
      })
      const store = useMusicLibraryStore()
      const result = await store.loadPlaylistsFromCache()
      expect(result).toBe(false)
      expect(store.playlists).toEqual([])
    })

    it('加载缓存后重置惰性排序追踪', async () => {
      const store = useMusicLibraryStore()
      store._sortedPlaylists.add('old_playlist')
      mockStoreGet.mockResolvedValue({
        playlists: [{ name: 'new', files: [{ path: '/a.mp3' }] }],
        timestamp: Date.now(),
      })
      await store.loadPlaylistsFromCache()
      expect(store._sortedPlaylists.size).toBe(0)
    })

    it('store.get 异常时返回 false', async () => {
      mockStoreGet.mockRejectedValue(new Error('store read error'))
      const store = useMusicLibraryStore()
      const result = await store.loadPlaylistsFromCache()
      expect(result).toBe(false)
    })
  })

  describe('_savePlaylistsToCache', () => {
    it('保存播放列表到缓存并去除 coverPath 字段', async () => {
      const store = useMusicLibraryStore()
      store.playlists = [
        {
          name: 'test',
          files: [
            {
              path: '/a.mp3',
              name: 'a',
              title: 'A',
              coverPath: '/cover/a.jpg',
            },
            {
              path: '/b.mp3',
              name: 'b',
              title: 'B',
              coverPath: '/cover/b.jpg',
            },
          ],
          totalFiles: 2,
        },
      ]
      await store._savePlaylistsToCache()
      expect(mockStoreSet).toHaveBeenCalledWith(
        'libraryCache',
        expect.objectContaining({
          playlists: expect.arrayContaining([
            expect.objectContaining({
              name: 'test',
            }),
          ]),
          timestamp: expect.any(Number),
        }),
      )
      // 验证 coverPath 被去除
      const savedArg = mockStoreSet.mock.calls[0][1] as {
        playlists: Array<{ files: Array<Record<string, unknown>> }>
      }
      for (const file of savedArg.playlists[0].files) {
        expect(file).not.toHaveProperty('coverPath')
      }
      expect(mockStoreSave).toHaveBeenCalled()
    })

    it('空播放列表也能正常保存', async () => {
      const store = useMusicLibraryStore()
      store.playlists = []
      await store._savePlaylistsToCache()
      expect(mockStoreSet).toHaveBeenCalledWith(
        'libraryCache',
        expect.objectContaining({
          playlists: [],
          timestamp: expect.any(Number),
        }),
      )
      expect(mockStoreSave).toHaveBeenCalled()
    })

    it('store.set 异常时不抛出', async () => {
      mockStoreSet.mockRejectedValue(new Error('write error'))
      const store = useMusicLibraryStore()
      store.playlists = [{ name: 'test', files: [] }]
      await expect(store._savePlaylistsToCache()).resolves.not.toThrow()
    })
  })

  // ---------- reset ----------

  describe('reset', () => {
    it('重置所有状态到初始值', () => {
      const store = useMusicLibraryStore()
      store.currentPlaylist = makePlaylist('test', [makeTrack('/a.mp3')])
      store.playlists = [makePlaylist('test', [makeTrack('/a.mp3')])]
      store._sortedPlaylists.add('test')
      store._loadedFromCache = true
      store.isBackgroundRefreshing = true
      store.loadingProgress = 50
      store.stats = {
        totalDirectories: 5,
        totalAudioFiles: 100,
        totalPlaylists: 5,
        maxDepth: 3,
      }

      store.reset()

      expect(store.currentPlaylist).toBeNull()
      expect(store.playlists).toEqual([])
      expect(store._sortedPlaylists.size).toBe(0)
      expect(store._loadedFromCache).toBe(false)
      expect(store.isBackgroundRefreshing).toBe(false)
      expect(store.loadingProgress).toBe(0)
      expect(store.stats).toEqual({
        totalDirectories: 0,
        totalAudioFiles: 0,
        totalPlaylists: 0,
        maxDepth: 0,
      })
    })
  })
})
