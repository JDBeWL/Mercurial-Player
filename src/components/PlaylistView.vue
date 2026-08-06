<template>
  <div class="playlist-view">
    <div class="playlist-header">
      <h2 class="playlist-title">{{ $t('playlist.title') }}</h2>
      <button class="icon-button" @click="handleClose">
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>

    <div
      ref="scrollContainer"
      class="playlist-content"
      :class="{ 'is-scrolling': isScrolling }"
      @scroll="handleScroll"
    >
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
            v-for="(track, index) in processedPlaylist"
            :key="track.path"
            v-memo="[track.path, track.path === currentPath, playerStore.isPlaying, track.coverUrl]"
            class="list-item"
            :class="{ selected: track.path === currentPath }"
            @click="playTrack(track)"
          >
            <div v-if="track.coverUrl" class="track-cover">
              <img
                :src="track.coverUrl"
                :alt="track.cachedTitle"
                :loading="index < 3 ? 'eager' : 'lazy'"
                :fetchpriority="index === 0 ? 'high' : 'auto'"
                decoding="async"
              />
            </div>
            <div v-else class="track-cover-placeholder">
              <span class="material-symbols-rounded">album</span>
            </div>
            <div class="list-item-content">
              <div class="list-item-headline" :title="track.cachedTitle">
                {{ track.cachedTitle }}
              </div>
              <div class="list-item-supporting" :title="track.cachedArtist">
                {{ track.cachedArtist }}
              </div>
            </div>
            <div class="list-item-trailing">
              <button
                v-if="track.path !== currentPath || !playerStore.isPlaying"
                class="play-button"
                :title="$t('playlist.play')"
                @click.stop="playTrack(track)"
              >
                <span class="material-symbols-rounded">play_arrow</span>
              </button>
              <button
                v-if="track.path === currentPath && playerStore.isPlaying"
                class="pause-button"
                :title="$t('playlist.pause')"
                @click.stop="pauseTrack"
              >
                <span class="material-symbols-rounded">pause</span>
              </button>
              <button
                class="remove-button"
                :title="$t('playlist.remove')"
                @click.stop="removeTrackByPath(track.path)"
              >
                <span class="material-symbols-rounded">close</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  ref,
  computed,
  watch,
  shallowRef,
  onMounted,
  onUnmounted,
  nextTick,
  type WatchStopHandle,
} from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import FileUtils from '../utils/fileUtils'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { Track } from '../types'

// 处理后的 track 类型 (扩展自 Track, 添加缓存字段)
interface ProcessedTrack extends Track {
  cachedTitle: string
  cachedArtist: string
  coverUrl?: string
}

const emit = defineEmits<{
  (e: 'close'): void
}>()

const playerStore = usePlayerStore()
const { playlist, currentTrack } = storeToRefs(playerStore)

// 滚动容器引用
const scrollContainer = ref<HTMLElement | null>(null)

// 滚动状态检测（用于禁用滚动时的 hover 效果）
const isScrolling = ref(false)
let scrollTimeout: ReturnType<typeof setTimeout> | null = null

const handleScroll = (): void => {
  isScrolling.value = true
  if (scrollTimeout) clearTimeout(scrollTimeout)
  scrollTimeout = setTimeout(() => {
    isScrolling.value = false
    scrollTimeout = null
  }, 150)
}

// 关闭处理
const handleClose = (): void => {
  emit('close')
}

// ===== 核心优化：用简单的 computed 替代 Map 遍历 =====
// 只追踪当前曲目的 path，O(1) 而非 O(N)
const currentPath = computed<string | null>(() => currentTrack.value?.path || null)

const playTrack = (track: Track): void => {
  if (playerStore.currentTrack?.path === track.path && !playerStore.isPlaying) {
    playerStore.resume()
  } else {
    playerStore.playTrack(track)
  }
}

const pauseTrack = (): void => {
  playerStore.pause()
}

// 标题/艺术家显示:简单的 || 链式调用,无需缓存
// (之前的 Map 缓存与 useTrackInfo.processedTracks 功能重叠,且 1000 条 Map 占用额外内存)
const getTrackTitle = (track: Track): string => {
  return track.displayTitle || track.title || FileUtils.getFileName(track.path)
}

const getTrackArtist = (track: Track): string => {
  return track.displayArtist || track.artist || ''
}

// ===== 核心优化：processedPlaylist 使用路径索引实现增量更新 =====
const processedPlaylist = shallowRef<ProcessedTrack[]>([])

// 用于快速查找已处理过的 track（path -> processedTrack 索引）
let processedMap = new Map<string, ProcessedTrack>()

// 构建单个 processed track 对象
const buildProcessedTrack = (track: Track): ProcessedTrack => ({
  ...track,
  cachedTitle: getTrackTitle(track),
  cachedArtist: getTrackArtist(track),
  coverUrl: track.coverPath ? convertFileSrc(track.coverPath) : undefined,
})

// 处理播放列表：增量更新，只重建变化的部分
const processPlaylist = (): void => {
  const raw = playlist.value
  if (raw.length === 0) {
    processedPlaylist.value = []
    processedMap = new Map()
    return
  }

  const newProcessedMap = new Map<string, ProcessedTrack>()
  const result = new Array<ProcessedTrack>(raw.length)
  let changed = false

  for (let i = 0; i < raw.length; i++) {
    const track = raw[i]
    const existing = processedMap.get(track.path)

    // 复用已有对象（如果 path 和 coverPath 都没变）
    if (existing && existing.coverPath === track.coverPath) {
      result[i] = existing
    } else {
      result[i] = buildProcessedTrack(track)
      changed = true
    }
    newProcessedMap.set(track.path, result[i])
  }

  // 列表长度变化或有新增/修改项时才更新
  if (
    changed ||
    result.length !== processedPlaylist.value.length ||
    processedMap.size !== newProcessedMap.size
  ) {
    processedPlaylist.value = result
  }
  processedMap = newProcessedMap
}

// ===== 核心优化：合并 watch，消除冗余 =====
// watch 监听 playlist 变化，深度监听设为 true，否则 splice 无法被检测到
const stopWatchPlaylist = watch(playlist, processPlaylist, { immediate: true, deep: true })

// ===== 优化：封面更新使用版本号通知，而非遍历式 watch =====
// 使用一个轻量的 coverVersion 计数器，由 store 的 _loadPlaylistCovers 完成后递增
// 但由于 store 的 cover 加载是通过直接修改 track.coverPath 实现的（mutation），
// 我们需要一个轻量的轮询方式来检测 coverPath 变化
let coverCheckTimer: ReturnType<typeof setInterval> | null = null
let lastCoverSnapshot = ''

const checkCoverUpdates = (): void => {
  // 只在有播放列表时检查
  if (playlist.value.length === 0) return

  // 轻量检查：只比较没有 cover 的 track 数量是否减少了
  // 这比 map+join 整个列表的字符串便宜得多
  let uncoveredCount = 0
  let sampleCover = ''
  for (let i = 0; i < playlist.value.length; i++) {
    const cp = playlist.value[i].coverPath
    if (!cp) {
      uncoveredCount++
    } else if (!sampleCover) {
      sampleCover = cp
    }
  }

  const snapshot = `${uncoveredCount}:${sampleCover}`
  if (snapshot !== lastCoverSnapshot) {
    lastCoverSnapshot = snapshot
    processPlaylist()
  }
}

// 使用低频率的定时器来检测 cover 变化（2秒一次，而非每帧）
// 只在组件存活期间运行
const startCoverCheck = (): void => {
  // 初始延迟后开始检查，给 store 的 _loadPlaylistCovers 时间开始工作
  coverCheckTimer = setInterval(checkCoverUpdates, 2000)
}

const stopCoverCheck = (): void => {
  if (coverCheckTimer) {
    clearInterval(coverCheckTimer)
    coverCheckTimer = null
  }
}

// 滚动到当前播放的歌曲
const scrollToCurrentTrack = (): void => {
  if (!currentTrack.value || processedPlaylist.value.length === 0 || !scrollContainer.value) return

  const currentIndex = processedPlaylist.value.findIndex((t) => t.path === currentTrack.value!.path)
  if (currentIndex === -1) return

  nextTick(() => {
    if (!scrollContainer.value) return
    const items = scrollContainer.value.querySelectorAll('.list-item')
    if (items[currentIndex]) {
      items[currentIndex].scrollIntoView({
        behavior: 'smooth',
        block: 'center',
      })
    }
  })
}

// 组件挂载时滚动到当前歌曲 & 启动 cover 检测
let hasScrolledOnMount = false
let stopWatchScrollOnMount: WatchStopHandle | null = null

onMounted(() => {
  startCoverCheck()

  stopWatchScrollOnMount = watch(
    processedPlaylist,
    (newList) => {
      if (!hasScrolledOnMount && newList.length > 0 && currentTrack.value) {
        hasScrolledOnMount = true
        scrollToCurrentTrack()
        if (stopWatchScrollOnMount) {
          stopWatchScrollOnMount()
        }
      }
    },
    { immediate: true },
  )
})

// 组件卸载时清理所有资源
onUnmounted(() => {
  if (scrollTimeout) {
    clearTimeout(scrollTimeout)
    scrollTimeout = null
  }

  stopCoverCheck()
  stopWatchPlaylist()
  stopWatchScrollOnMount?.()

  // 清理缓存和状态
  processedMap.clear()
  processedMap = new Map()
  processedPlaylist.value = []
})

// 通过路径删除音轨
const removeTrackByPath = (path: string): void => {
  playerStore.removeTrack(path)
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
  /* box-shadow: var(--md-sys-elevation-level2); */
  z-index: 1000;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  will-change: transform;
  /* transition: transform 0.4s cubic-bezier(0.25, 0.46, 0.45, 0.94); */
}

/* .playlist-view.slide-out {
  transform: translateX(100%);
} */

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
  overflow-x: hidden;
  padding: 16px;
  contain: layout style paint;
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

.is-scrolling .list {
  pointer-events: none;
}

.list {
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  overflow: visible;
  padding: 2px;
}

.list-item {
  display: flex;
  align-items: center;
  padding: 12px 16px;
  margin: 2px 0;
  cursor: pointer;
  overflow: hidden;
  border-radius: 8px;
  contain: layout style paint;
}

.list-item:hover {
  background-color: var(--md-sys-color-hover-overlay);
}

.list-item.selected {
  background-color: var(--md-sys-color-hover-overlay);
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

@media (max-width: 480px) {
  .list-item-headline {
    font-size: 14px;
  }

  .list-item-supporting {
    font-size: 12px;
  }
}
</style>
