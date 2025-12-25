<template>
  <div class="playlist-view" :class="{ 'slide-out': isClosing }">
    <div class="playlist-header">
      <h2 class="playlist-title">{{ $t('playlist.title') }}</h2>
      <button class="icon-button" @click="handleClose">
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>
    
    <div class="playlist-content">
      <div v-if="playlist.length === 0" class="playlist-empty">
        <div class="empty-state">
          <span class="material-symbols-rounded">queue_music</span>
          <h3>{{ $t('playlist.empty') }}</h3>
          <p>{{ $t('playlist.addSongs') }}</p>
        </div>
      </div>
      
      <div v-else class="playlist-songs">
        <div class="list">
          <div 
            v-for="track in processedPlaylist" 
            :key="track.path"
            v-memo="[track.path, isCurrentTrackMap.get(track.path), playerStore.isPlaying]"
            class="list-item"
            :class="{ selected: isCurrentTrackMap.get(track.path) }"
            @click="playTrack(track)"
          >

            <div class="track-cover" v-if="track.cover">
              <img :src="track.cover" :alt="track.cachedTitle" loading="lazy" decoding="async" />
            </div>
            <div class="track-cover-placeholder" v-else>
              <span class="material-symbols-rounded">album</span>
            </div>
            <div class="list-item-content">
              <div class="list-item-headline" :title="track.cachedTitle">{{ track.cachedTitle }}</div>
              <div class="list-item-supporting" :title="track.cachedArtist">{{ track.cachedArtist }}</div>
            </div>
            <div class="list-item-trailing">
              <button 
                v-if="!isCurrentTrackMap.get(track.path) || !playerStore.isPlaying"
                class="icon-button play-button" 
                @click.stop="playTrack(track)"
                :title="$t('playlist.play')"
              >
                <span class="material-symbols-rounded">play_arrow</span>
              </button>
              <button 
                v-if="isCurrentTrackMap.get(track.path) && playerStore.isPlaying"
                class="icon-button pause-button" 
                @click.stop="pauseTrack"
                :title="$t('playlist.pause')"
              >
                <span class="material-symbols-rounded">pause</span>
              </button>
              <button class="icon-button remove-button" @click.stop="removeTrackByPath(track.path)" :title="$t('playlist.remove')">
                <span class="material-symbols-rounded">close</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, watch, shallowRef, nextTick, onMounted, onUnmounted } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import FileUtils from '../utils/fileUtils'

const emit = defineEmits(['close'])

const playerStore = usePlayerStore()
const { playlist, currentTrack, isPlaying } = storeToRefs(playerStore)

// 播放列表内容容器的引用
const playlistContentRef = ref(null)

// 控制动画状态
const isClosing = ref(false)
let closeTimeout = null

// 滚动到当前播放的歌曲
const scrollToCurrentTrack = () => {
  if (!currentTrack.value || playlist.value.length === 0) return
  
  const currentIndex = playlist.value.findIndex(t => t.path === currentTrack.value.path)
  if (currentIndex === -1) return
  
  nextTick(() => {
    const container = document.querySelector('.playlist-content')
    const items = document.querySelectorAll('.list-item')
    
    if (container && items[currentIndex]) {
      const item = items[currentIndex]
      const containerRect = container.getBoundingClientRect()
      const itemRect = item.getBoundingClientRect()
      
      // 计算滚动位置，让当前歌曲显示在容器中间
      const scrollTop = item.offsetTop - container.offsetTop - (containerRect.height / 2) + (itemRect.height / 2)
      
      container.scrollTo({
        top: Math.max(0, scrollTop),
        behavior: 'smooth'
      })
    }
  })
}

// 组件挂载时滚动到当前歌曲
onMounted(() => {
  // 稍微延迟以确保列表已渲染
  setTimeout(scrollToCurrentTrack, 100)
})

// 关闭动画处理
const handleClose = () => {
  isClosing.value = true
  if (closeTimeout) clearTimeout(closeTimeout)
  closeTimeout = setTimeout(() => {
    emit('close')
    closeTimeout = null
  }, 300) // 与CSS动画时间一致
}

// 使用 shallowRef 和延迟更新，减少主线程阻塞
// 只在 currentTrack 实际变化时更新，而不是在每次渲染时计算
const isCurrentTrackMap = shallowRef(new Map())
const currentPathRef = ref(null)

// 存储待清理的回调ID
let idleCallbackIds = []
let timeoutIds = []

// 使用 requestIdleCallback 延迟更新，避免阻塞滚动
const updateCurrentTrackMap = () => {
  const map = new Map()
  const currentPath = currentPathRef.value
  if (currentPath && playlist.value.length > 0) {
    // 批量更新，减少循环开销
    for (let i = 0; i < playlist.value.length; i++) {
      const track = playlist.value[i]
      map.set(track.path, track.path === currentPath)
    }
  }
  isCurrentTrackMap.value = map
}

// 清理所有待处理的回调
const cleanupCallbacks = () => {
  // 清理 requestIdleCallback（如果支持）
  if ('cancelIdleCallback' in window) {
    idleCallbackIds.forEach(id => cancelIdleCallback(id))
  }
  idleCallbackIds = []
  
  // 清理 setTimeout
  timeoutIds.forEach(id => clearTimeout(id))
  timeoutIds = []
}

// 安全的延迟执行函数
const scheduleUpdate = (fn) => {
  cleanupCallbacks() // 清理之前的回调
  
  if ('requestIdleCallback' in window) {
    const id = requestIdleCallback(fn, { timeout: 100 })
    idleCallbackIds.push(id)
  } else {
    const id = setTimeout(fn, 0)
    timeoutIds.push(id)
  }
}

// 监听 currentTrack 变化，使用 requestIdleCallback 延迟更新
const stopWatchCurrentTrack = watch(currentTrack, (newTrack) => {
  const newPath = newTrack?.path || null
  if (newPath !== currentPathRef.value) {
    currentPathRef.value = newPath
    scheduleUpdate(updateCurrentTrackMap)
  }
}, { immediate: true })

// 监听 playlist 变化，但延迟更新
const stopWatchPlaylist = watch(playlist, () => {
  if (currentPathRef.value) {
    scheduleUpdate(updateCurrentTrackMap)
  }
}, { deep: false })

const playTrack = (track) => {
  playerStore.playTrack(track)
}

const pauseTrack = () => {
  playerStore.pause()
}

// 创建标题缓存，限制大小防止内存溢出
const MAX_CACHE_SIZE = 1000 // 最多缓存1000个音轨的信息
const titleCache = new Map()
const artistCache = new Map()

// 清理缓存函数，保持缓存大小在限制内
const cleanupCache = (cache) => {
  if (cache.size > MAX_CACHE_SIZE) {
    // 删除最旧的条目（Map 保持插入顺序）
    const entriesToDelete = Array.from(cache.keys()).slice(0, cache.size - MAX_CACHE_SIZE)
    entriesToDelete.forEach(key => cache.delete(key))
  }
}

// 智能获取音轨标题 - 使用缓存提高性能
const getTrackTitle = (track) => {
  // 使用音轨路径作为缓存键
  if (titleCache.has(track.path)) {
    return titleCache.get(track.path)
  }
  
  let title
  // 优先使用displayTitle字段（如果已预处理）
  if (track.displayTitle) {
    title = track.displayTitle
  }
  // 其次使用title字段
  else if (track.title) {
    title = track.title
  }
  // 最后使用文件名
  else {
    title = FileUtils.getFileName(track.path)
  }
  
  // 缓存结果
  titleCache.set(track.path, title)
  cleanupCache(titleCache) // 防止缓存无限增长
  return title
}

// 智能获取音轨艺术家 - 使用缓存提高性能
const getTrackArtist = (track) => {
  // 使用音轨路径作为缓存键
  if (artistCache.has(track.path)) {
    return artistCache.get(track.path)
  }
  
  let artist
  // 优先使用displayArtist字段（如果已预处理）
  if (track.displayArtist) {
    artist = track.displayArtist
  }
  // 其次使用artist字段
  else if (track.artist) {
    artist = track.artist
  }
  // 返回空字符串
  else {
    artist = ''
  }
  
  // 缓存结果
  artistCache.set(track.path, artist)
  cleanupCache(artistCache) // 防止缓存无限增长
  return artist
}

// 使用 shallowRef 减少响应式开销，避免深度响应式追踪
// 使用延迟更新策略，避免在滚动时阻塞主线程
const processedPlaylist = shallowRef([])
let processingIdleCallbackId = null
let processingTimeoutId = null

// 延迟处理播放列表，避免阻塞主线程
const processPlaylistAsync = () => {
  // 清理之前的回调
  if (processingIdleCallbackId && 'cancelIdleCallback' in window) {
    cancelIdleCallback(processingIdleCallbackId)
    processingIdleCallbackId = null
  }
  if (processingTimeoutId) {
    clearTimeout(processingTimeoutId)
    processingTimeoutId = null
  }
  
  // 使用 requestIdleCallback 在浏览器空闲时处理
  const process = () => {
    if (playlist.value.length === 0) {
      processedPlaylist.value = []
      return
    }
    
    // 批量处理，减少中间对象创建
    const result = []
    for (let i = 0; i < playlist.value.length; i++) {
      const track = playlist.value[i]
      result.push({
        ...track,
        cachedTitle: getTrackTitle(track),
        cachedArtist: getTrackArtist(track)
      })
    }
    processedPlaylist.value = result
  }
  
  if ('requestIdleCallback' in window) {
    processingIdleCallbackId = requestIdleCallback(process, { timeout: 200 })
  } else {
    // 降级到 setTimeout，使用较短的延迟
    processingTimeoutId = setTimeout(process, 16) // 约一帧的时间
  }
}

// 监听 playlist 变化，延迟处理
const stopWatchProcessedPlaylist = watch(playlist, processPlaylistAsync, { immediate: true, deep: false })

// 组件卸载时清理所有资源
onUnmounted(() => {
  // 清理定时器
  if (closeTimeout) {
    clearTimeout(closeTimeout)
    closeTimeout = null
  }
  
  // 清理 watch
  stopWatchCurrentTrack()
  stopWatchPlaylist()
  stopWatchProcessedPlaylist()
  
  // 清理所有回调
  cleanupCallbacks()
  
  // 清理处理播放列表的回调
  if (processingIdleCallbackId && 'cancelIdleCallback' in window) {
    cancelIdleCallback(processingIdleCallbackId)
  }
  if (processingTimeoutId) {
    clearTimeout(processingTimeoutId)
  }
  
  // 清理缓存（可选，如果需要立即释放内存）
  // titleCache.clear()
  // artistCache.clear()
})

// 通过路径删除音轨，而不是索引
const removeTrackByPath = (path) => {
  const newPlaylist = playlist.value.filter(track => track.path !== path)
  playerStore.loadPlaylist(newPlaylist)
}
</script>

<style scoped>
.playlist-view {
  position: fixed;
  top: 0;
  right: 0;
  width: 400px;
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

.playlist-view.slide-out {
  transform: translateX(100%);
}

.playlist-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.playlist-title {
  font-size: 24px;
  font-weight: 500;
  margin: 0;
  color: var(--md-sys-color-on-surface);
}

.playlist-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
  transform: translateZ(0);
  -webkit-overflow-scrolling: touch;
  /* 使用 content-visibility 优化滚动性能 */
  content-visibility: auto;
  contain-intrinsic-size: auto 500px;
}

.playlist-empty {
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
  margin: 0;
  color: var(--md-sys-color-on-surface-variant);
}

.playlist-songs {
  height: 100%;
}

.list {
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  overflow: visible;
  padding: 2px;
  contain: layout style paint;
  content-visibility: auto;
}

.list-item {
  display: flex;
  align-items: center;
  padding: 12px 16px;
  margin: 2px 0;
  cursor: pointer;
  position: relative;
  overflow: hidden;
  border-radius: 8px;
  /* 优化滚动性能：使用 transform 启用硬件加速 */
  transform: translateZ(0);
  /* 使用 contain 优化渲染性能，限制重绘范围 */
  contain: layout style paint;
  content-visibility: auto;
  contain-intrinsic-size: auto 72px;
}

.list-item:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.list-item.selected {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  border-radius: 8px;
  z-index: 1;
}



.track-cover,
.track-cover-placeholder {
  width: 48px;
  height: 48px;
  border-radius: 4px;
  margin-right: 12px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.track-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.track-cover-placeholder {
  background-color: var(--md-sys-color-surface-variant);
}

.track-cover-placeholder .material-symbols-rounded {
  font-size: 24px;
  color: var(--md-sys-color-on-surface-variant);
}

.list-item-content {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

.list-item-headline {
  font-size: 16px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.list-item.selected .list-item-headline {
  color: var(--md-sys-color-on-primary-container);
}

.list-item-supporting {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.list-item.selected .list-item-supporting {
  color: var(--md-sys-color-on-primary-container);
}

.list-item-trailing {
  display: flex;
  gap: 4px;
  align-items: center;
}

.play-button,
.pause-button,
.remove-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background-color: transparent;
  border: none;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.play-button:hover,
.pause-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 12%, transparent);
}

.remove-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-error) 12%, transparent);
}

.play-button .material-symbols-rounded,
.pause-button .material-symbols-rounded {
  font-size: 20px;
  color: var(--md-sys-color-on-surface-variant);
}

.remove-button .material-symbols-rounded {
  font-size: 18px;
  color: var(--md-sys-color-on-surface-variant);
}

.list-item.selected .play-button .material-symbols-rounded,
.list-item.selected .pause-button .material-symbols-rounded {
  color: var(--md-sys-color-on-primary-container);
}

@media (max-width: 480px) {
  .list-item-headline {
    font-size: 14px;
  }
  
  .list-item-supporting {
    font-size: 12px;
  }
}
</style>