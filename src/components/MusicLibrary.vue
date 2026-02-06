<template>
  <div class="music-library" :class="{ 'slide-out': isClosing }">
    <div class="library-header">
      <h2 class="library-title">{{ $t('library.title') }}</h2>
      <div class="header-actions">
        <button class="icon-button" @click="refreshDirectoryTrees" title="刷新音乐库">
          <span class="material-symbols-rounded">refresh</span>
        </button>
        <button class="icon-button" @click="handleClose">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
    </div>
    
    
    <div class="library-content">
      <!-- 搜索栏 -->
      <div class="search-bar" v-if="musicFolders.length > 0">
        <div class="search-input-wrapper">
          <span class="material-symbols-rounded">search</span>
          <input 
            type="text" 
            v-model="searchTerm" 
            :placeholder="$t('library.searchPlaceholder')"
            @input="handleSearch"
          />
          <button class="icon-button" @click="clearSearch" v-if="searchTerm">
            <span class="material-symbols-rounded">close</span>
          </button>
        </div>
      </div>
      
      <!-- 搜索结果 -->
      <div class="search-results" v-if="searchResults.length > 0">
        <h3 class="section-title">{{ $t('library.searchResults') }} ({{ searchResults.length }})</h3>
        <div class="list">
          <div 
            v-for="(file, index) in searchResults" 
            :key="`search-${index}`"
            class="list-item"
            @click="playFile(file)"
          >
            <div class="list-item-leading">
              <span class="material-symbols-rounded">music_note</span>
            </div>
            <div class="list-item-content">
              <div class="list-item-headline" :title="file.displayTitle || file.name">{{ file.displayTitle || file.name }}</div>
              <div class="list-item-supporting" :title="(file.displayArtist || file.artist || '') + (file.displayArtist ? ' • ' : '') + file.folderName">
                {{ file.displayArtist || file.artist || '' }} 
                {{ file.displayArtist ? '•' : '' }} 
                {{ file.folderName }}
              </div>
            </div>
            <div class="list-item-trailing">
              <button class="icon-button" @click.stop="addFileNext(file)" :title="$t('library.playNext') || '播放下一首'">
                <span class="material-symbols-rounded">playlist_add</span>
              </button>
            </div>
          </div>
        </div>
      </div>
      
      <!-- 播放列表和目录结构 -->
      <div class="library-structure" v-if="!searchTerm && musicFolders.length > 0">
        
        <!-- 播放列表 -->
        <div class="library-playlists" v-if="playlists.length > 0">
          <div class="playlists-header">
            <h3 class="section-title">
              {{ $t('library.playlists') }}
              <button class="text-button" @click="playAll" v-if="playlists.length > 0" title="播放全部歌曲">
                <span class="material-symbols-rounded">play_arrow</span>
                播放全部歌曲
              </button>
            </h3>
            <button class="text-button sort-button" @click="toggleSortOrder" :title="$t('library.toggleSortOrder')">
              {{ configStore.playlist.sortOrder === 'asc' ? 'A-Z' : 'Z-A' }}
            </button>
          </div>
          <div class="list">
            <div 
              v-for="(playlist, index) in enhancedPlaylists" 
              :key="`playlist-${index}`"
              class="list-item"
              @click="loadPlaylist(playlist)"
            >
              <div class="list-item-leading">
                <span class="material-symbols-rounded">
                  {{ playlist.isAllSongsPlaylist ? 'library_music' : 'queue_music' }}
                </span>
              </div>
              <div class="list-item-content">
                <div class="list-item-headline">{{ playlist.name }}</div>
                <div class="list-item-supporting">
                  {{ $t('playlist.songs', playlist.totalFiles) }}
                  <span v-if="playlist.subdirectoryCount > 0">
                    • {{ playlist.subdirectoryCount }} {{ $t('library.subdirectories') }}
                  </span>
                </div>
              </div>
              <div class="list-item-trailing">
                <button class="icon-button" @click.stop="playPlaylist(playlist)">
                  <span class="material-symbols-rounded">play_arrow</span>
                </button>
              </div>
            </div>
          </div>
        </div>
        
        <!-- 目录结构 -->
        <!-- <div class="library-directories" v-if="directoryStats.totalDirectories > 0">
          <h3 class="section-title">
            {{ $t('library.directories') }}
            <span class="stats">
              {{ directoryStats.totalDirectories }} {{ $t('library.directories') }}, 
              {{ directoryStats.totalAudioFiles }} {{ $t('library.songs') }}
            </span>
          </h3>
        </div> -->
      </div>
      
      <!-- 空状态 -->
      <div class="library-empty" v-if="musicFolders.length === 0">
        <div class="empty-state">
          <span class="material-symbols-rounded">folder_open</span>
          <h3>{{ $t('library.emptyTitle') }}</h3>
          <p>{{ $t('library.emptyDescription') }}</p>
          <button class="filled-button" @click="openFolderDialog">
            {{ $t('library.selectFirstDirectory') }}
          </button>
        </div>
      </div>
      
      <!-- 加载状态 -->
      <div class="loading-overlay" v-if="isLoading">
        <div class="loading-spinner">
          <span class="material-symbols-rounded">progress_activity</span>
          <span>{{ $t('library.loading') }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, reactive, onMounted, onUnmounted, computed, shallowRef, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useMusicLibraryStore } from '../stores/musicLibrary'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import { useI18n } from 'vue-i18n'

import FileUtils from '../utils/fileUtils'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity } from '../utils/errorHandler'

const emit = defineEmits(['close'])
const { t } = useI18n()

const musicLibraryStore = useMusicLibraryStore()
const playerStore = usePlayerStore()
const configStore = useConfigStore()


// 控制动画状态
const isClosing = ref(false)

// 关闭动画处理
let closeTimeout = null

const handleClose = () => {
  isClosing.value = true
  if (closeTimeout) clearTimeout(closeTimeout)
  closeTimeout = setTimeout(() => {
    emit('close')
    closeTimeout = null
  }, 300) // 与CSS动画时间一致
}

// 组件卸载时清理
onUnmounted(() => {
  if (closeTimeout) {
    clearTimeout(closeTimeout)
    closeTimeout = null
  }
})

const { musicFolders, playlists } = storeToRefs(musicLibraryStore)
const searchTerm = ref('')
const searchResults = ref([])
const isLoading = ref(false)
const directoryStats = reactive({
  totalDirectories: 0,
  totalAudioFiles: 0,
  totalPlaylists: 0,
  maxDepth: 0
})

// 使用 shallowRef 缓存 enhancedPlaylists 结果，减少响应式开销
const cachedEnhancedPlaylists = shallowRef([])
const lastPlaylistsHash = ref('')
const lastSortOrder = ref('')

// 计算播放列表的哈希值用于检测变化
const getPlaylistsHash = () => {
  if (!playlists.value.length) return ''
  return playlists.value.map(p => `${p.name}:${p.files?.length || 0}`).join('|')
}

// 实际的计算逻辑
const computeEnhancedPlaylists = () => {
  if (!playlists.value.length) return []

  let allPlaylists = []
  let allSongsFiles = []
  const uniqueFiles = new Set()
  
  // 检查是否有全部歌曲播放列表
  const hasAllSongsPlaylist = playlists.value.some(p => p.name === '全部歌曲')

  // 处理全部歌曲播放列表
  for (const playlist of playlists.value) {
    if (playlist.files && playlist.name !== '全部歌曲') {
      for (const file of playlist.files) {
        if (!uniqueFiles.has(file.path)) {
          uniqueFiles.add(file.path)
          allSongsFiles.push(file)
        }
      }
    }
  }

  // 如果有全部歌曲，则添加到播放列表中
  if (allSongsFiles.length > 0 && !hasAllSongsPlaylist) {
    allPlaylists.push({
      name: `全部歌曲 (${allSongsFiles.length} 首)`,
      path: 'all-songs',
      files: allSongsFiles,
      subdirectoryCount: 0,
      totalFiles: allSongsFiles.length,
      isAllSongsPlaylist: true
    })
  }

  // 处理其他播放列表
  for (const playlist of playlists.value) {
    if (playlist.files && playlist.files.length > 0) {
      // 处理播放列表名称，如果为"全部歌曲"，则加上文件数量
      let playlistName = playlist.name
      if (playlist.name === '全部歌曲') {
        playlistName = `全部歌曲 (${playlist.files.length} 首)`
      } else {
        playlistName = `${playlist.name} (${playlist.files.length} 首)`
      }
      
      allPlaylists.push({
        ...playlist,
        totalFiles: playlist.files.length,
        subdirectoryCount: 0,
        name: playlistName,
        isAllSongsPlaylist: playlist.name === '全部歌曲'
      })
    }
  }

  // 根据配置排序
  const isAscOrder = configStore.playlist.sortOrder === 'asc'
  return allPlaylists.sort((a, b) => {
    // 如果两个都是"全部歌曲"，则保持原始顺序
    if (a.isAllSongsPlaylist && b.isAllSongsPlaylist) return 0
    
    // 如果一个是"全部歌曲"，则排在前面
    if (a.isAllSongsPlaylist) return -1
    if (b.isAllSongsPlaylist) return 1
    
    // 如果两个都不是"全部歌曲"，则按名称排序
    const nameA = a.name.toLowerCase()
    const nameB = b.name.toLowerCase()
    
    if (isAscOrder) {
      // A-Z order
      if (nameA < nameB) return -1
      if (nameA > nameB) return 1
    } else {
      // Z-A order
      if (nameA > nameB) return -1
      if (nameA < nameB) return 1
    }
    
    return 0
  })
}

// 监听播放列表和排序变化，只在必要时重新计算
watch(
  [playlists, () => configStore.playlist.sortOrder],
  () => {
    const currentHash = getPlaylistsHash()
    const currentSortOrder = configStore.playlist.sortOrder
    
    // 只有当播放列表或排序顺序真正变化时才重新计算
    if (currentHash !== lastPlaylistsHash.value || currentSortOrder !== lastSortOrder.value) {
      lastPlaylistsHash.value = currentHash
      lastSortOrder.value = currentSortOrder
      cachedEnhancedPlaylists.value = computeEnhancedPlaylists()
    }
  },
  { immediate: true }
)

// 使用缓存的结果
const enhancedPlaylists = computed(() => cachedEnhancedPlaylists.value)

// 生命周期
onMounted(async () => {
  // 只在音乐库为空时加载，避免频繁刷新
  if (musicLibraryStore.musicFolders.length === 0) {
    await musicLibraryStore.loadMusicFolders()
  }
  
  // 只在播放列表为空时刷新，避免频繁扫描
  if (musicLibraryStore.playlists.length === 0) {
    await musicLibraryStore.refreshMusicFolders()
  }
  
  await calculateDirectoryStats()
})

// 目录树管理
const refreshDirectoryTrees = async () => {
  isLoading.value = true
  try {
    await musicLibraryStore.refreshMusicFolders()
    await calculateDirectoryStats()
  } catch (error) {
    logger.error('Error refreshing directory trees:', error)
  } finally {
    isLoading.value = false
  }
}



const calculateDirectoryStats = async () => {
  if (!playlists.value.length) {
    Object.assign(directoryStats, {
      totalDirectories: 0,
      totalAudioFiles: 0,
      totalPlaylists: 0,
      maxDepth: 0
    })
    return
  }
  
  let totalDirs = 0
  let totalFiles = 0
  let allAudioFiles = new Set() // 使用Set来去重
  let allDirectories = new Set() // 使用Set来去重目录
  
  // 统计所有播放列表的实际文件和目录
  for (const playlist of playlists.value) {
    if (playlist.files) {
      playlist.files.forEach(file => allAudioFiles.add(file.path))
    }
    if (playlist.name !== '全部歌曲' && playlist.files && playlist.files.length > 0) {
      // 如果不是"全部歌曲"，则添加到目录中
      allDirectories.add(playlist.name); 
    }
  }
  
  totalFiles = allAudioFiles.size
  totalDirs = allDirectories.size
  
  Object.assign(directoryStats, {
    totalDirectories: totalDirs,
    totalAudioFiles: totalFiles,
    totalPlaylists: enhancedPlaylists.value.length,
    maxDepth: 3
  })
}

// 搜索功能 - 添加防抖
let searchTimeout = null
const handleSearch = async () => {
  // 清除之前的定时器
  if (searchTimeout) {
    clearTimeout(searchTimeout)
  }
  
  if (!searchTerm.value.trim()) {
    searchResults.value = []
    return
  }
  
  // 防抖：300ms 后执行搜索
  searchTimeout = setTimeout(() => {
    const lowerCaseSearchTerm = searchTerm.value.toLowerCase()
    const uniqueResults = new Map() // 使用Map来去重

    for (const playlist of playlists.value) {
      if (playlist.files) {
        const results = playlist.files.filter(file => 
          (file.title && file.title.toLowerCase().includes(lowerCaseSearchTerm)) ||
          (file.artist && file.artist.toLowerCase().includes(lowerCaseSearchTerm)) ||
          (file.album && file.album.toLowerCase().includes(lowerCaseSearchTerm)) ||
          (file.name && file.name.toLowerCase().includes(lowerCaseSearchTerm))
        )
        
        // 去重
        for (const file of results) {
          if (!uniqueResults.has(file.path)) {
            uniqueResults.set(file.path, file)
          }
        }
      }
    }
    
    // 根据配置排序
    const isAscOrder = configStore.playlist.sortOrder === 'asc'
    searchResults.value = Array.from(uniqueResults.values()).sort((a, b) => {
      const titleA = (a.title || a.name || '').toLowerCase()
      const titleB = (b.title || b.name || '').toLowerCase()
      
      if (isAscOrder) {
        // A-Z order
        if (titleA < titleB) return -1
        if (titleA > titleB) return 1
      } else {
        // Z-A order
        if (titleA > titleB) return -1
        if (titleA < titleB) return 1
      }
      
      return 0
    })
  }, 300)
}

const clearSearch = () => {
  searchTerm.value = ''
  searchResults.value = []
}



// 播放控制
const openFolderDialog = async () => {
  try {
    const selected = await FileUtils.selectFolder({
      title: '选择音乐文件夹'
    })
    
    if (selected) {
      const result = await musicLibraryStore.addMusicFolder(selected)
      await calculateDirectoryStats()
      logger.info(result.message)
      
      // 检查是否是初次添加音乐库
      if (musicLibraryStore.musicFolders.length === 1) {
        logger.info('初次添加音乐库，正在刷新配置和播放列表...')
        
        // 初次添加音乐库时，主动加载配置
        await configStore.loadConfig()
        
        // 刷新音乐文件夹以生成播放列表
        await musicLibraryStore.refreshMusicFolders()
        await calculateDirectoryStats()
        
        logger.info('初次音乐库配置和播放列表生成完成')
      }
    }
  } catch (error) {
    logger.error('Error opening folder dialog:', error)
  }
}

// 当前选中的播放列表
const selectedPlaylist = ref(null)

// 播放全部（当前显示的全部歌曲播放列表）
const playAll = async () => {
  // 找到全部歌曲播放列表
  const allSongsPlaylist = enhancedPlaylists.value.find(p => p.isAllSongsPlaylist)
  if (allSongsPlaylist && allSongsPlaylist.files.length > 0) {
    await playerStore.loadPlaylist(allSongsPlaylist.files)
    playerStore.play()
    handleClose()
  } else if (enhancedPlaylists.value.length > 0) {
    // 如果没有全部歌曲播放列表，则播放第一个播放列表
    const firstPlaylist = enhancedPlaylists.value[0]
    await playerStore.loadPlaylist(firstPlaylist.files)
    playerStore.play()
    handleClose()
  }
}

// 点击列表项时加载播放列表并解码但不播放
const loadPlaylist = async (playlist) => {
  await playerStore.loadPlaylist(playlist.files)
  // 解码第一首音频但不播放
  if (playlist.files && playlist.files.length > 0) {
    await playerStore.playTrack(playlist.files[0])
    playerStore.pause()
  }
  handleClose()
}

// 点击播放按钮时加载播放列表并立即播放第一首
const playPlaylist = async (playlist) => {
  await playerStore.loadPlaylist(playlist.files)
  playerStore.play()
  handleClose()
}

const toggleSortOrder = () => {
  configStore.toggleSortOrder()
  // 重新刷新播放列表以应用新的排序
  refreshDirectoryTrees()
}

const playFile = (file) => {
  const playlist = {
    name: '搜索结果',
    files: [file]
  }
  playPlaylist(playlist)
}

const addFileNext = (file) => {
  playerStore.addTrackNext(file)
  logger.info('Added track to play next:', file.displayTitle || file.name)
  
  // 显示成功通知
  errorHandler.handle(
    new Error('Track added to play next'),
    {
      type: ErrorType.PLAYBACK_ERROR,
      severity: ErrorSeverity.LOW,
      context: { trackName: file.displayTitle || file.name },
      showToUser: true,
      userMessage: t('library.addedToPlayNext')
    }
  )
}
</script>

<style scoped>
.music-library {
  position: fixed;
  top: 0;
  left: 0;
  width: 450px;
  max-width: 90vw;
  height: 100%;
  background-color: var(--md-sys-color-surface);
  box-shadow: var(--md-sys-elevation-level2);
  z-index: 1000;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  transform: translateX(0);
  transition: transform 0.3s cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

.music-library.slide-out {
  transform: translateX(-100%);
}

@media (max-width: 480px) {
  .music-library {
    width: 100vw;
    max-width: 100vw;
  }
  
  .list-item-headline {
    font-size: 14px;
  }
  
  .list-item-supporting {
    font-size: 12px;
  }
}

.library-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.library-title {
  font-size: 24px;
  font-weight: 500;
  margin: 0;
  color: var(--md-sys-color-on-surface);
}



/* 搜索栏 */
.search-bar {
  margin-bottom: 16px;
}

.search-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  padding: 8px 12px;
  height: 48px; /* 固定高度，与搜索后的高度一致 */
  box-sizing: border-box; /* 确保padding包含在高度内 */
  border: 1px solid var(--md-sys-color-outline);
}

.search-input-wrapper .material-symbols-rounded {
  color: var(--md-sys-color-on-surface-variant);
  margin-right: 8px;
}

.search-input-wrapper input {
  flex: 1;
  border: none;
  background: none;
  outline: none;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
}

.library-content {
  flex: 1;
  padding: 16px;
  overflow-y: auto;
  position: relative;
}

.playlists-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.section-title {
  font-size: 18px;
  font-weight: 500;
  margin: 0;
  color: var(--md-sys-color-on-surface);
  display: flex;
  align-items: center;
  gap: 12px;
}

.stats {
  font-size: 14px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface-variant);
}

.library-playlists, .library-directories, .search-results {
  margin-bottom: 24px;
}

.list {
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  overflow: hidden;
}

.list-item {
  display: flex;
  align-items: center;
  padding: 12px 16px;
  cursor: pointer;
  transition: all 0.2s ease;
  border-radius: 8px;
  margin: 2px 0;
}

.list-item:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  transform: translateX(2px);
}

.list-item:active {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 12%, transparent);
}

.list-item-leading {
  margin-right: 16px;
  color: var(--md-sys-color-on-surface-variant);
}

.list-item-content {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  position: relative;
  /* 确保可以正确计算高度 */
  min-height: 44px;
}

.list-item-headline {
  font-size: 16px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  /* 添加平滑过渡 */
  transition: all 0.2s ease;
  line-height: 1.4;
  /* 确保短标题不会有多余空间 */
  max-height: 1.4em; /* 约1行的高度 */
}

.list-item-supporting {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  /* 添加平滑过渡 */
  transition: all 0.2s ease;
  line-height: 1.4;
  /* 确保短艺术家名不会有多余空间 */
  max-height: 1.4em; /* 约1行的高度 */
}



.list-item-trailing {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
  margin-left: 8px;
}

.directory-tree {
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  padding: 8px;
}

/* 空状态 */
.library-empty {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  max-width: 300px;
}

.empty-state .material-symbols-rounded {
  font-size: 64px;
  color: var(--md-sys-color-on-surface-variant);
  margin-bottom: 16px;
}

.empty-state h3 {
  font-size: 20px;
  font-weight: 500;
  margin: 0 0 8px 0;
  color: var(--md-sys-color-on-surface);
}

.empty-state p {
  font-size: 14px;
  margin: 0 0 24px 0;
  color: var(--md-sys-color-on-surface-variant);
}

/* 加载状态 */
.loading-overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 5;
}

.loading-spinner {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--md-sys-color-on-surface);
}

.loading-spinner .material-symbols-rounded {
  font-size: 32px;
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}


.icon-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.filled-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 24px;
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.filled-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-primary-container));
}

.text-button {
  background: none;
  border: none;
  color: var(--md-sys-color-primary);
  cursor: pointer;
  padding: 8px 12px;
  border-radius: var(--md-sys-shape-corner-medium);
  font-size: 14px;
  transition: background-color 0.2s;
  display: flex;
  align-items: center;
  gap: 4px;
}

.text-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.sort-button {
  color: var(--md-sys-color-on-surface-variant);
}

.material-symbols-rounded {
  font-size: 20px;
  margin-right: 0 !important;
}
</style>