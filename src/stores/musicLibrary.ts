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
  /** 是否已从缓存加载 */
  _loadedFromCache: boolean
}

export const useMusicLibraryStore = defineStore('musicLibrary', {
  state: (): MusicLibraryState => ({
    // 音乐文件夹管理
    musicFolders: [],

    // 播放列表管理
    playlists: [],
    currentPlaylist: null,

    // 加载状态
    isLoading: false,
    error: null,

    // 惰性排序追踪
    _sortedPlaylists: new Set<string>(),
    _sortedSortOrder: '',
    _refreshEpoch: 0,

    // 缓存标记
    _loadedFromCache: false,
  }),

  getters: {},

  actions: {
    // ========== 音乐文件夹管理 ==========

    /**
     * 加载音乐文件夹
     */
    async loadMusicFolders(): Promise<{ success: boolean; message: string }> {
      try {
        this.musicFolders = await invoke<string[]>('get_music_directories')
        return { success: true, message: 'Music directories loaded successfully' }
      } catch (error) {
        logger.error('Error loading music directories:', error)
        return { success: false, message: String(error) }
      }
    },

    /**
     * 添加音乐文件夹
     */
    async addMusicFolder(folderPath: string): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('add_music_directory', { path: folderPath })
        this.musicFolders = updatedFolders
        // 同时更新配置存储中的音乐文件夹列表
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders
        return { success: true, message: 'Folder added successfully' }
      } catch (error) {
        logger.error('Error adding music folder:', error)
        return { success: false, message: String(error) }
      }
    },

    /**
     * 移除音乐文件夹
     */
    async removeMusicFolder(folderPath: string): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('remove_music_directory', {
          path: folderPath,
        })
        this.musicFolders = updatedFolders
        // 同时更新配置存储中的音乐文件夹列表
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders

        // 如果当前播放列表受到影响，清空它
        // 注意：folderPath 与 f.path 可能来自不同数据源，Windows 下斜杠方向可能不一致，
        // 需要先规范化再比较，避免漏判
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

    /**
     * 设置音乐文件夹
     */
    async setMusicFolders(folders: string[]): Promise<{ success: boolean; message: string }> {
      try {
        const updatedFolders = await invoke<string[]>('set_music_directories', { paths: folders })
        this.musicFolders = updatedFolders
        // 同时更新配置存储中的音乐文件夹列表
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders
        return { success: true, message: 'Music directories updated successfully' }
      } catch (error) {
        logger.error('Error setting music directories:', error)
        return { success: false, message: String(error) }
      }
    },

    /**
     * 刷新音乐文件夹
     * 使用分批更新策略，避免一次性替换大数组导致前端卡顿
     * 扫描完成后自动将播放列表缓存到 plugin-store 以加速下次启动
     */
    async refreshMusicFolders(): Promise<{ success: boolean; message: string }> {
      // 并发刷新会让「清空 → 分批 push」交错叠加,用代际只让最后一次生效
      const epoch = ++this._refreshEpoch
      const superseded = { success: false, message: 'Superseded by a newer refresh' }
      try {
        // 先记录当前选中状态，刷新后重新绑定到新对象，避免封面/元数据显示不更新
        const currentPlaylistName = this.currentPlaylist?.name ?? null

        // 获取新的播放列表数据
        const newPlaylists = await invoke<Playlist[]>('get_all_audio_files', {
          paths: this.musicFolders,
        })
        if (epoch !== this._refreshEpoch) return superseded

        // 分批更新，避免一次性替换导致响应式风暴
        const BATCH_SIZE = 10 // 每批处理 10 个播放列表
        this.playlists = [] // 先清空

        for (let i = 0; i < newPlaylists.length; i += BATCH_SIZE) {
          if (epoch !== this._refreshEpoch) return superseded
          const batch = newPlaylists.slice(i, i + BATCH_SIZE)
          this.playlists.push(...batch)

          // 让出主线程，避免阻塞 UI
          if (i + BATCH_SIZE < newPlaylists.length) {
            await new Promise((resolve) => setTimeout(resolve, 0))
          }
        }

        // 重置惰性排序追踪，让下次选择播放列表时重新排序
        this._sortedPlaylists = new Set<string>()
        this._sortedSortOrder = ''

        // 重新绑定当前播放列表（指向刷新后的新对象）
        if (currentPlaylistName) {
          this.currentPlaylist = this.playlists.find((p) => p.name === currentPlaylistName) ?? null
        }

        // 刷新后同步更新 player.playlist 中的曲目引用
        // playlists 已重建为新对象，但 player.playlist 仍持有旧对象引用，元数据不会更新
        // 注意：扫描走轻量模式，新对象不含 coverPath，直接整体替换会导致封面丢失，
        // 且结构 watch 只比较 path，重新打开播放列表才会暴露，故沿用旧对象已加载的封面路径
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

        // 异步缓存到 plugin-store（不阻塞当前流程）
        // 失败已在 _savePlaylistsToCache 内部记录告警，这里不再重复 catch
        void this._savePlaylistsToCache()

        return { success: true, message: 'Library refreshed successfully' }
      } catch (error) {
        logger.error('Error refreshing music folders:', error)
        return { success: false, message: String(error) }
      }
    },

    /**
     * 从缓存中加载播放列表（启动时快速恢复）
     * 返回是否成功加载了缓存
     */
    async loadPlaylistsFromCache(): Promise<boolean> {
      try {
        const store = await getLibraryStore()
        const cached = await store.get<{ playlists: CachedPlaylist[]; timestamp: number }>(
          'libraryCache',
        )
        if (!cached || !cached.playlists || cached.playlists.length === 0) {
          return false
        }

        // 检查缓存是否过期（超过 7 天视为过期）
        const CACHE_MAX_AGE = 7 * 24 * 60 * 60 * 1000
        if (Date.now() - cached.timestamp > CACHE_MAX_AGE) {
          logger.info('Library cache is too old, will refresh')
          return false
        }

        // 将缓存的播放列表恢复到 state（coverPath 字段为 undefined，后续按需加载）
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

    /**
     * 将当前播放列表保存到缓存
     * 去除 cover 数据以减小缓存体积
     */
    async _savePlaylistsToCache(): Promise<void> {
      try {
        const store = await getLibraryStore()
        // 去除 coverPath 字段以减小体积
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

    /**
     * 对播放列表进行惰性排序（只在首次访问时排序一次）
     */
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

    // ========== 播放列表管理 ==========

    /**
     * 选择播放列表（触发惰性排序）
     */
    selectPlaylist(playlist: Playlist): void {
      this._ensureSorted(playlist)
      this.currentPlaylist = playlist
    },

    // ========== 文件操作 ==========

    /**
     * 从播放列表中移除文件
     */
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

    /**
     * 重置播放列表状态
     */
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
