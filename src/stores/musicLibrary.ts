import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { load, type Store } from '@tauri-apps/plugin-store'
import { useConfigStore } from './config'
import logger from '../utils/logger'
import type { Track, Playlist, LibraryStats } from '@/types'

// plugin-store 实例（懒加载单例）
let _libraryStoreInstance: Store | null = null
async function getLibraryStore(): Promise<Store> {
  if (!_libraryStoreInstance) {
    _libraryStoreInstance = await load('library-cache.json', {
      defaults: {},
      autoSave: false,
    })
  }
  return _libraryStoreInstance
}

/** 缓存中存储的播放列表数据（不包含封面以减小体积） */
interface CachedPlaylist {
  name: string
  files: Omit<Track, 'coverPath'>[]
}

interface PlayHistoryItem extends Track {
  timestamp: string
}

interface SearchResult extends Track {
  folderPath?: string
  folderName?: string
}

interface MusicLibraryState {
  musicFolders: string[]
  playlists: Playlist[]
  currentPlaylist: Playlist | null
  currentFile: Track | null
  /** 缓存当前播放文件在 currentPlaylist 中的索引，避免每次 getter 都做 O(N) 查找 */
  _currentFileIndex: number
  searchResults: SearchResult[]
  searchTerm: string
  playHistory: PlayHistoryItem[]
  isLoading: boolean
  /** 是否正在后台刷新（区别于首次加载的阻塞加载） */
  isBackgroundRefreshing: boolean
  /** 加载进度（0-100） */
  loadingProgress: number
  error: string | null
  directoryTree: unknown | null
  stats: LibraryStats
  /** 记录已排序的播放列表名称，用于惰性排序 */
  _sortedPlaylists: Set<string>
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

    // 当前播放状态
    currentFile: null,
    _currentFileIndex: -1,

    // 搜索功能
    searchResults: [],
    searchTerm: '',

    // 播放历史
    playHistory: [],

    // 加载状态
    isLoading: false,
    isBackgroundRefreshing: false,
    loadingProgress: 0,
    error: null,

    // 目录结构
    directoryTree: null,

    // 统计信息
    stats: {
      totalDirectories: 0,
      totalAudioFiles: 0,
      totalPlaylists: 0,
      maxDepth: 0
    },

    // 惰性排序追踪
    _sortedPlaylists: new Set<string>(),

    // 缓存标记
    _loadedFromCache: false,
  }),

  getters: {
    /**
     * 获取所有音轨的列表
     * 使用 Array.prototype.concat 替代 push(...spread)，避免大数组展开的栈溢出和性能问题
     */
    allTracks: (state): Track[] => {
      const arrays = state.playlists.map(p => p.files)
      return ([] as Track[]).concat(...arrays)
    },

    /**
     * 获取当前播放列表的文件列表
     */
    currentFiles: (state): Track[] => {
      return state.currentPlaylist?.files || []
    },

    /**
     * 获取当前播放文件的索引
     * 优先使用缓存的 _currentFileIndex，仅在缓存失效时回退到线性查找
     */
    currentIndex: (state): number => {
      if (!state.currentPlaylist || !state.currentFile) return -1

      const files = state.currentPlaylist.files
      const cachedIdx = state._currentFileIndex

      // 缓存命中：O(1)
      if (cachedIdx >= 0 && cachedIdx < files.length && files[cachedIdx]?.path === state.currentFile.path) {
        return cachedIdx
      }

      // 缓存失效，回退到线性查找
      return files.findIndex(file => file.path === state.currentFile!.path)
    },

    /**
     * 获取上一首文件
     */
    previousFile(): Track | null {
      const index = this.currentIndex
      if (index > 0 && this.currentPlaylist) {
        return this.currentPlaylist.files[index - 1]
      }
      return null
    },

    /**
     * 获取下一首文件
     */
    nextFile(): Track | null {
      const index = this.currentIndex
      if (this.currentPlaylist && index >= 0 && index < this.currentPlaylist.files.length - 1) {
        return this.currentPlaylist.files[index + 1]
      }
      return null
    },

    /**
     * 检查是否在当前播放列表的开头
     */
    isAtStart(): boolean {
      return this.currentIndex <= 0
    },

    /**
     * 检查是否在当前播放列表的结尾
     */
    isAtEnd(): boolean {
      const index = this.currentIndex
      return this.currentPlaylist ? index >= this.currentPlaylist.files.length - 1 : true
    },

    /**
     * 获取总播放进度
     */
    totalProgress(): number {
      if (!this.currentPlaylist) return 0
      const total = this.currentPlaylist.files.length
      const current = this.currentIndex + 1
      return total > 0 ? (current / total) * 100 : 0
    }
  },

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
        const updatedFolders = await invoke<string[]>('remove_music_directory', { path: folderPath })
        this.musicFolders = updatedFolders
        // 同时更新配置存储中的音乐文件夹列表
        const configStore = useConfigStore()
        configStore.musicDirectories = updatedFolders
        
        // 如果当前播放列表受到影响，清空它
        if (this.currentPlaylist && this.currentPlaylist.files.some(f => f.path.startsWith(folderPath))) {
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
      try {
        // 先记录当前选中状态，刷新后重新绑定到新对象，避免封面/元数据显示不更新
        const currentPlaylistName = this.currentPlaylist?.name ?? null
        const currentFilePath = this.currentFile?.path ?? null

        // 获取新的播放列表数据
        const newPlaylists = await invoke<Playlist[]>('get_all_audio_files', { paths: this.musicFolders })

        // 分批更新，避免一次性替换导致响应式风暴
        const BATCH_SIZE = 10 // 每批处理 10 个播放列表
        this.playlists = [] // 先清空

        for (let i = 0; i < newPlaylists.length; i += BATCH_SIZE) {
          const batch = newPlaylists.slice(i, i + BATCH_SIZE)
          this.playlists.push(...batch)

          // 让出主线程，避免阻塞 UI
          if (i + BATCH_SIZE < newPlaylists.length) {
            await new Promise(resolve => setTimeout(resolve, 0))
          }
        }

        // 重置惰性排序追踪，让下次选择播放列表时重新排序
        this._sortedPlaylists = new Set<string>()

        // 重新绑定当前播放列表（指向刷新后的新对象）
        if (currentPlaylistName) {
          this.currentPlaylist = this.playlists.find(p => p.name === currentPlaylistName) ?? null
        }

        // 重新绑定当前文件（指向刷新后的新对象，确保 coverPath 等字段能更新）
        if (currentFilePath) {
          const playlistToSearch = this.currentPlaylist ? [this.currentPlaylist] : this.playlists
          let refreshedTrack: Track | null = null

          for (const p of playlistToSearch) {
            const found = p.files.find(f => f.path === currentFilePath)
            if (found) {
              refreshedTrack = found
              // 同步索引缓存
              if (this.currentPlaylist && this.currentPlaylist.name === p.name) {
                this._currentFileIndex = p.files.findIndex(f => f.path === currentFilePath)
              }
              break
            }
          }

          this.currentFile = refreshedTrack
        }

        // 异步缓存到 plugin-store（不阻塞当前流程）
        this._savePlaylistsToCache().catch(err =>
          logger.warn('Failed to save playlists cache:', err)
        )

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
        const cached = await store.get<{ playlists: CachedPlaylist[], timestamp: number }>('libraryCache')
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
        const lightPlaylists: CachedPlaylist[] = this.playlists.map(p => ({
          name: p.name,
          files: p.files.map(({ coverPath, ...rest }) => rest)
        }))
        await store.set('libraryCache', {
          playlists: lightPlaylists,
          timestamp: Date.now()
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
      if (this._sortedPlaylists.has(playlist.name)) return
      if (!playlist.files || playlist.files.length === 0) {
        this._sortedPlaylists.add(playlist.name)
        return
      }

      const configStore = useConfigStore()
      const isAscOrder = configStore.playlist.sortOrder === 'asc'

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
      this.currentFile = null
      this._currentFileIndex = -1
      this.addToPlayHistory(playlist as unknown as Track)
    },

    /**
     * 设置当前播放文件，同步维护索引缓存
     */
    setCurrentFile(file: Track): void {
      this.currentFile = file
      // 同步更新索引缓存，避免 currentIndex getter 做 O(N) 查找
      if (this.currentPlaylist && file) {
        const files = this.currentPlaylist.files
        // 先检查相邻位置（连续播放的高频场景）
        const cachedIdx = this._currentFileIndex
        if (cachedIdx >= 0 && cachedIdx < files.length) {
          // 检查下一首
          if (cachedIdx + 1 < files.length && files[cachedIdx + 1]?.path === file.path) {
            this._currentFileIndex = cachedIdx + 1
            this.addToPlayHistory(file)
            return
          }
          // 检查上一首
          if (cachedIdx - 1 >= 0 && files[cachedIdx - 1]?.path === file.path) {
            this._currentFileIndex = cachedIdx - 1
            this.addToPlayHistory(file)
            return
          }
          // 检查当前位置（重播）
          if (files[cachedIdx]?.path === file.path) {
            this.addToPlayHistory(file)
            return
          }
        }
        // 回退到线性查找
        this._currentFileIndex = files.findIndex(f => f.path === file.path)
      } else {
        this._currentFileIndex = -1
      }
      this.addToPlayHistory(file)
    },

    /**
     * 播放上一首
     */
    playPrevious(): Track | null {
      if (this.isAtStart) return null
      
      const previousFile = this.previousFile
      if (previousFile) {
        this.setCurrentFile(previousFile)
        return previousFile
      }
      
      return null
    },

    /**
     * 播放下一首
     */
    playNext(): Track | null {
      if (this.isAtEnd) return null
      
      const nextFile = this.nextFile
      if (nextFile) {
        this.setCurrentFile(nextFile)
        return nextFile
      }
      
      return null
    },

    /**
     * 随机播放
     */
    playRandom(): Track | null {
      if (!this.currentPlaylist || this.currentPlaylist.files.length === 0) return null
      
      const files = this.currentPlaylist.files
      const randomIndex = Math.floor(Math.random() * files.length)
      const randomFile = files[randomIndex]
      
      this.setCurrentFile(randomFile)
      return randomFile
    },

    /**
     * 清空当前播放列表
     */
    clearCurrentPlaylist(): void {
      this.currentPlaylist = null
      this.currentFile = null
    },

    // ========== 搜索功能 ==========

    /**
     * 搜索音频文件
     */
    async searchFiles(searchTerm: string): Promise<void> {
      this.isLoading = true
      this.searchTerm = searchTerm
      
      try {
        if (!searchTerm.trim()) {
          this.searchResults = []
          return
        }
        
        this.searchResults = []
        const lowerCaseSearchTerm = searchTerm.toLowerCase()

        for (const playlist of this.playlists) {
          if (playlist.files) {
            const results = playlist.files.filter(file => 
              (file.title && file.title.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.artist && file.artist.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.album && file.album.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.name && file.name.toLowerCase().includes(lowerCaseSearchTerm))
            )
            this.searchResults = this.searchResults.concat(results)
          }
        }
        
      } catch (error) {
        logger.error('Error searching files:', error)
        this.error = (error as Error).message
        throw error
      } finally {
        this.isLoading = false
      }
    },

    /**
     * 清空搜索
     */
    clearSearch(): void {
      this.searchResults = []
      this.searchTerm = ''
    },

    // ========== 播放历史管理 ==========

    /**
     * 添加到播放历史
     */
    addToPlayHistory(item: Track): void {
      // 限制历史记录长度
      if (this.playHistory.length >= 100) {
        this.playHistory.shift()
      }
      
      this.playHistory.push({
        ...item,
        timestamp: new Date().toISOString()
      })
    },

    /**
     * 获取播放历史
     */
    getPlayHistory(limit: number = 20): PlayHistoryItem[] {
      return this.playHistory.slice(-limit).reverse()
    },

    // ========== 文件操作 ==========

    /**
     * 从播放列表中移除文件
     */
    removeFileFromPlaylist(filePath: string): void {
      if (!this.currentPlaylist) return

      const index = this.currentPlaylist.files.findIndex(file => file.path === filePath)
      if (index > -1) {
        this.currentPlaylist.files.splice(index, 1)
        if (this.currentPlaylist.totalFiles) {
          this.currentPlaylist.totalFiles--
        }

        // 如果移除的是当前播放的文件，需要更新当前文件
        if (this.currentFile && this.currentFile.path === filePath) {
          this.currentFile = null
          this._currentFileIndex = -1
        } else if (index < this._currentFileIndex) {
          // 移除的在当前索引之前，索引需要前移
          this._currentFileIndex--
        }
      }
    },

    /**
     * 刷新播放列表
     */
    async refreshPlaylist(): Promise<void> {
      await this.refreshMusicFolders()
    },

    /**
     * 重置播放列表状态
     */
    reset(): void {
      this.currentPlaylist = null
      this.currentFile = null
      this._currentFileIndex = -1
      this.playlists = []
      this.playHistory = []
      this.searchResults = []
      this.searchTerm = ''
      this.directoryTree = null
      this._sortedPlaylists = new Set<string>()
      this._loadedFromCache = false
      this.isBackgroundRefreshing = false
      this.loadingProgress = 0
      this.stats = {
        totalDirectories: 0,
        totalAudioFiles: 0,
        totalPlaylists: 0,
        maxDepth: 0
      }
    }
  }
})
