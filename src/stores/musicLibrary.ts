import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useConfigStore } from './config'
import logger from '../utils/logger'
import type { Track, Playlist, LibraryStats } from '@/types'

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
  searchResults: SearchResult[]
  searchTerm: string
  playHistory: PlayHistoryItem[]
  isLoading: boolean
  error: string | null
  directoryTree: unknown | null
  stats: LibraryStats
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
    
    // 搜索功能
    searchResults: [],
    searchTerm: '',
    
    // 播放历史
    playHistory: [],
    
    // 加载状态
    isLoading: false,
    error: null,
    
    // 目录结构
    directoryTree: null,
    
    // 统计信息
    stats: {
      totalDirectories: 0,
      totalAudioFiles: 0,
      totalPlaylists: 0,
      maxDepth: 0
    }
  }),

  getters: {
    /**
     * 获取所有音轨的列表
     */
    allTracks: (state): Track[] => {
      const tracks: Track[] = []
      for (const playlist of state.playlists) {
        tracks.push(...playlist.files)
      }
      return tracks
    },

    /**
     * 获取当前播放列表的文件列表
     */
    currentFiles: (state): Track[] => {
      return state.currentPlaylist?.files || []
    },

    /**
     * 获取当前播放文件的索引
     */
    currentIndex: (state): number => {
      if (!state.currentPlaylist || !state.currentFile) return -1
      return state.currentPlaylist.files.findIndex(file => 
        file.path === state.currentFile!.path
      )
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
     */
    async refreshMusicFolders(): Promise<{ success: boolean; message: string }> {
      try {
        this.playlists = await invoke<Playlist[]>('get_all_audio_files', { paths: this.musicFolders })
        
        // 获取配置中的排序顺序
        const configStore = useConfigStore()
        const isAscOrder = configStore.playlist.sortOrder === 'asc'
        
        // 对所有播放列表中的文件按A-Z或Z-A排序
        this.playlists.forEach(playlist => {
          if (playlist.files && playlist.files.length > 0) {
            playlist.files.sort((a, b) => {
              const titleA = (a.title || a.name || '').toLowerCase()
              const titleB = (b.title || b.name || '').toLowerCase()
              
              if (isAscOrder) {
                if (titleA < titleB) return -1
                if (titleA > titleB) return 1
              } else {
                if (titleA > titleB) return -1
                if (titleA < titleB) return 1
              }
              
              return 0
            })
          }
        })
        
        return { success: true, message: 'Library refreshed successfully' }
      } catch (error) {
        logger.error('Error refreshing music folders:', error)
        return { success: false, message: String(error) }
      }
    },

    // ========== 播放列表管理 ==========

    /**
     * 选择播放列表
     */
    selectPlaylist(playlist: Playlist): void {
      this.currentPlaylist = playlist
      this.currentFile = null
      this.addToPlayHistory(playlist as unknown as Track)
    },

    /**
     * 设置当前播放文件
     */
    setCurrentFile(file: Track): void {
      this.currentFile = file
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
      this.playlists = []
      this.playHistory = []
      this.searchResults = []
      this.searchTerm = ''
      this.directoryTree = null
      this.stats = {
        totalDirectories: 0,
        totalAudioFiles: 0,
        totalPlaylists: 0,
        maxDepth: 0
      }
    }
  }
})
