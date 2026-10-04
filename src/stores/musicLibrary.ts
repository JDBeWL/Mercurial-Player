import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { load, type Store } from '@tauri-apps/plugin-store'
import { resolveDataFile } from '../services/appService'
import { useConfigStore } from './config'
import { usePlayerStore } from './player'
import logger from '../utils/logger'
import type { Track, Playlist, SortOrder } from '@/types'

// plugin-store 实例（懒加载单例）
let _libraryStoreInstance: Store | null = null
async function getLibraryStore(): Promise<Store> {
  if (!_libraryStoreInstance) {
    // 传绝对路径:plugin-store 遇绝对路径会原样使用,
    // 借此把 library-cache.json 放到主程序同级 data/ 而非系统 Roaming 目录
    _libraryStoreInstance = await load(await resolveDataFile('library-cache.json'), {
      defaults: {},
      autoSave: false,
    })
  }
  return _libraryStoreInstance
}

/** 规范化路径用于比较：统一为正斜杠 + 小写，避免 Windows 下斜杠方向不一致导致匹配失败 */
function normalizePath(p: string): string {
  return p.replace(/\\/g, '/').toLowerCase()
}

/** 缓存中存储的播放列表数据（不包含封面以减小体积） */
interface CachedPlaylist {
  name: string
  files: Omit<Track, 'coverPath'>[]
  isAllSongsPlaylist?: boolean
}

interface MusicLibraryState {
  musicFolders: string[]
  playlists: Playlist[]
  currentPlaylist: Playlist | null
  isLoading: boolean
  error: string | null
  /** 记录已排序的播放列表名称，用于惰性排序 */
  _sortedPlaylists: Set<string>
  /** 上次排序使用的顺序，与配置不一致时作废惰性排序结果 */
  _sortedSortOrder: SortOrder | ''
  /** 刷新代际，并发调用时只让最后一次的结果生效 */
  _refreshEpoch: number
  _loadedFromCache: boolean
}

export const useMusicLibraryStore = defineStore('musicLibrary', {
  state: (): MusicLibraryState => ({
    musicFolders: [],

    playlists: [],
    currentPlaylist: null,

    isLoading: false,
    error: null,

    _sortedPlaylists: new Set<string>(),
    _sortedSortOrder: '',
    _refreshEpoch: 0,

    _loadedFromCache: false,
  }),

  getters: {},

  actions: {
    // --- 音乐文件夹管理 ---

    /** 加载音乐文件夹 */
    async loadMusicFolders(): Promise<{ success: boolean; message: string }> {
      try {
        this.musicFolders = await invoke<string[]>('get_music_directories')
        return { success: true, message: 'Music directories loaded successfully' }
      } catch (error) {
        logger.error('Error loading music directories:', error)
        return { success: false, message: String(error) }
      }
    },

    /** 添加音乐文件夹 */
    async addMusicFolder(folderPath: string): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('add_music_directory', { path: folderPath })
        this.musicFolders = updatedFolders
        // 后端是唯一数据源,变更须同步镜像到 config.musicDirectories,否则设置页与持久化副本不一致
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders
        return { success: true, message: 'Folder added successfully' }
      } catch (error) {
        logger.error('Error adding music folder:', error)
        return { success: false, message: String(error) }
      }
    },

    /** 移除音乐文件夹 */
    async removeMusicFolder(folderPath: string): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('remove_music_directory', {
          path: folderPath,
        })
        this.musicFolders = updatedFolders
        // 镜像到 config.musicDirectories(理由见 addMusicFolder)
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders

        // 路径来自不同数据源,须先 normalizePath 再比较,否则 Windows 下会漏判
        const normalizedFolder = normalizePath(folderPath)
        if (
          this.currentPlaylist &&
          this.currentPlaylist.files.some((f) => normalizePath(f.path).startsWith(normalizedFolder))
        ) {
          this.currentPlaylist = null
        }

        return { success: true, message: 'Folder removed successfully' }
      } catch (error) {
        logger.error('Error removing music folder:', error)
        return { success: false, message: String(error) }
      }
    },

    /** 设置音乐文件夹 */
    async setMusicFolders(folders: string[]): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('set_music_directories', { paths: folders })
        this.musicFolders = updatedFolders
        // 镜像到 config.musicDirectories(理由见 addMusicFolder)
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders
        return { success: true, message: 'Music directories updated successfully' }
      } catch (error) {
        logger.error('Error setting music directories:', error)
        return { success: false, message: String(error) }
      }
    },

    /** 刷新音乐文件夹，分批重建播放列表；完成后写入 plugin-store 缓存以加速下次启动 */
    async refreshMusicFolders(): Promise<{ success: boolean; message: string }> {
      // 并发刷新会让「清空 -> 分批 push」交错叠加,用代际只让最后一次生效
      const epoch = ++this._refreshEpoch
      const superseded = { success: false, message: 'Superseded by a newer refresh' }
      try {
        // 先记录当前选中状态，刷新后重新绑定到新对象，避免封面/元数据显示不更新
        const currentPlaylistName = this.currentPlaylist?.name ?? null

        const newPlaylists = await invoke<Playlist[]>('get_all_audio_files', {
          paths: this.musicFolders,
        })
        if (epoch !== this._refreshEpoch) return superseded

        // 分批 push 而非整体替换:大列表一次性替换会触发响应式风暴卡住 UI
        const BATCH_SIZE = 10
        this.playlists = []

        for (let i = 0; i < newPlaylists.length; i += BATCH_SIZE) {
          if (epoch !== this._refreshEpoch) return superseded
          const batch = newPlaylists.slice(i, i + BATCH_SIZE)
          this.playlists.push(...batch)

          // 让出主线程，避免阻塞 UI
          if (i + BATCH_SIZE < newPlaylists.length) {
            await new Promise((resolve) => setTimeout(resolve, 0))
          }
        }

        // 作废惰性排序记录，下次选择播放列表时重新排序
        this._sortedPlaylists = new Set<string>()
        this._sortedSortOrder = ''

        if (currentPlaylistName) {
          this.currentPlaylist = this.playlists.find((p) => p.name === currentPlaylistName) ?? null
        }

        // playlists 重建为新对象后 player.playlist 仍持旧引用;扫描是轻量模式不含 coverPath,
        // 直接整体替换会丢封面,故沿用旧条目已加载的封面路径
        const playerStore = usePlayerStore()
        if (playerStore.playlist.length > 0) {
          const trackMap = new Map<string, Track>()
          for (const p of this.playlists) {
            for (const f of p.files) {
              trackMap.set(f.path, f)
            }
          }
          playerStore._setPlaylist(
            playerStore.playlist.map((t) => {
              const fresh = trackMap.get(t.path)
              if (!fresh) return t
              return t.coverPath ? { ...fresh, coverPath: t.coverPath } : fresh
            }),
          )

          // 替换前同样未加载出封面的曲目，补一次加载，避免封面永久缺失
          const missingCovers = playerStore.playlist.filter((t) => !t.coverPath)
          if (missingCovers.length > 0) {
            void playerStore._loadPlaylistCovers(missingCovers)
          }
        }

        // 异步写 plugin-store 缓存,不阻塞当前流程;失败已在 _savePlaylistsToCache 内部告警
        void this._savePlaylistsToCache()

        return { success: true, message: 'Library refreshed successfully' }
      } catch (error) {
        logger.error('Error refreshing music folders:', error)
        return { success: false, message: String(error) }
      }
    },

    /** 启动时从缓存恢复播放列表，返回是否命中缓存 */
    async loadPlaylistsFromCache(): Promise<boolean> {
      try {
        const store = await getLibraryStore()
        const cached = await store.get<{ playlists: CachedPlaylist[]; timestamp: number }>(
          'libraryCache',
        )
        if (!cached || !cached.playlists || cached.playlists.length === 0) {
          return false
        }

        const CACHE_MAX_AGE = 7 * 24 * 60 * 60 * 1000
        if (Date.now() - cached.timestamp > CACHE_MAX_AGE) {
          logger.info('Library cache is too old, will refresh')
          return false
        }

        // 缓存条目不含 coverPath，恢复到 state 后按需重新加载封面
        this.playlists = cached.playlists as Playlist[]
        this._loadedFromCache = true
        this._sortedPlaylists = new Set<string>()
        this._sortedSortOrder = ''
        // 缓存是权威内容,让仍在进行中的刷新作废
        this._refreshEpoch++
        logger.info(`Loaded ${cached.playlists.length} playlists from cache`)
        return true
      } catch (err) {
        logger.warn('Failed to load playlists from cache:', err)
        return false
      }
    },

    /** 将当前播放列表写入缓存，剥离 coverPath 以减小体积 */
    async _savePlaylistsToCache(): Promise<void> {
      try {
        const store = await getLibraryStore()
        const lightPlaylists: CachedPlaylist[] = this.playlists.map((p) => ({
          name: p.name,
          files: p.files.map(({ coverPath: _coverPath, ...rest }) => rest),
          isAllSongsPlaylist: p.isAllSongsPlaylist,
        }))
        await store.set('libraryCache', {
          playlists: lightPlaylists,
          timestamp: Date.now(),
        })
        await store.save()
        logger.debug('Playlists cache saved')
      } catch (err) {
        logger.warn('Failed to save playlists cache:', err)
      }
    },

    /** 惰性排序：只在首次访问某播放列表时排序一次 */
    _ensureSorted(playlist: Playlist): void {
      const configStore = useConfigStore()
      const sortOrder = configStore.playlist.sortOrder

      // 排序顺序变了就作废旧结果:改顺序的入口不止 refresh(设置页直接改配置也算)
      if (this._sortedSortOrder !== sortOrder) {
        this._sortedPlaylists.clear()
        this._sortedSortOrder = sortOrder
      }

      if (this._sortedPlaylists.has(playlist.name)) return
      if (!playlist.files || playlist.files.length === 0) {
        this._sortedPlaylists.add(playlist.name)
        return
      }

      const isAscOrder = sortOrder === 'asc'

      playlist.files.sort((a, b) => {
        const titleA = (a.title || a.name || '').toLowerCase()
        const titleB = (b.title || b.name || '').toLowerCase()
        if (titleA < titleB) return isAscOrder ? -1 : 1
        if (titleA > titleB) return isAscOrder ? 1 : -1
        return 0
      })

      this._sortedPlaylists.add(playlist.name)
    },

    // --- 播放列表管理 ---

    /** 选择播放列表（触发惰性排序） */
    selectPlaylist(playlist: Playlist): void {
      this._ensureSorted(playlist)
      this.currentPlaylist = playlist
    },

    // --- 文件操作 ---

    /** 从播放列表中移除文件 */
    removeFileFromPlaylist(filePath: string): void {
      if (!this.currentPlaylist) return

      const index = this.currentPlaylist.files.findIndex((file) => file.path === filePath)
      if (index > -1) {
        this.currentPlaylist.files.splice(index, 1)
        if (this.currentPlaylist.totalFiles) {
          this.currentPlaylist.totalFiles--
        }
      }
    },

    /** 重置播放列表状态 */
    reset(): void {
      this.currentPlaylist = null
      this.playlists = []
      this._sortedPlaylists = new Set<string>()
      this._sortedSortOrder = ''
      this._loadedFromCache = false
      this._refreshEpoch++
    },
  },
})
