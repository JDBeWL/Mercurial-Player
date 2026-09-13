<template>
  <div class="playlist-view">
    <div class="playlist-header">
      <div class="playlist-header-left">
        <h2 class="playlist-title">{{ $t('playlist.title') }}</h2>
        <span
          v-if="showQueueInfo && playlist.length > 0"
          class="playlist-queue-info"
          :title="queueInfoText"
        >
          {{ queueInfoText }}
        </span>
      </div>
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
        <!-- 使用委托让整份列表只在容器上挂 1 个 click，而不是每行 3 个。
             行与按钮通过 data-path / data-action 声明意图，由 handleListClick 分派。 -->
        <div class="list" @click="handleListClick">
          <div
            v-for="(track, index) in processedPlaylist"
            :key="track.path"
            v-memo="[track.path, track.path === currentPath, isTrackPlaying(track), track.coverUrl]"
            class="list-item"
            :class="{ selected: track.path === currentPath }"
            :data-path="track.path"
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
                type="button"
                class="play-button"
                :title="isTrackPlaying(track) ? $t('playlist.pause') : $t('playlist.play')"
                :data-action="isTrackPlaying(track) ? 'pause' : 'play'"
              >
                <span class="material-symbols-rounded">{{
                  isTrackPlaying(track) ? 'pause' : 'play_arrow'
                }}</span>
              </button>
              <button
                type="button"
                class="remove-button"
                :title="$t('playlist.remove')"
                data-action="remove"
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
  triggerRef,
  onMounted,
  onUnmounted,
  nextTick,
  type WatchStopHandle,
} from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import FileUtils from '../utils/fileUtils'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import type { Track } from '../types'
import { formatTime } from '../utils/format'

// 处理后的 track 类型 (扩展自 Track, 添加缓存字段)
interface ProcessedTrack extends Track {
  cachedTitle: string
  cachedArtist: string
  coverUrl?: string
}

const emit = defineEmits<{
  close: []
}>()

const playerStore = usePlayerStore()
const configStore = useConfigStore()
const { t } = useI18n()
const { playlist, currentTrack, currentTrackIndex } = storeToRefs(playerStore)

// 是否显示播放队列信息
const showQueueInfo = computed(() => configStore.general.showQueueInfo !== false)

// 第 X 首 / 共 Y 首 · 总时长
const queueInfoText = computed(() => {
  const base = t('player.queueInfo', {
    current: currentTrackIndex.value + 1,
    total: playlist.value.length,
  })

  // 播放列表总时长（有时长数据才附加）
  const total = playlist.value.reduce((sum, track) => sum + (track.duration || 0), 0)
  if (!total) return base

  return `${base} · ${formatTime(total)}`
})

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

// 该行是否为当前正在播放的曲目:决定播放按钮显示播放还是暂停。
// 播放/暂停曾是两个互斥的 v-if 按钮,未命中的那个会在 DOM 里留下一个注释占位节点,
// 合并为单按钮后每行少一个注释节点
const isTrackPlaying = (track: Track): boolean =>
  track.path === currentPath.value && playerStore.isPlaying

const playTrack = (track: Track): void => {
  if (playerStore.currentTrack?.path === track.path && !playerStore.isPlaying) {
    playerStore.resume()
  } else {
    void playerStore.playTrack(track)
  }
}

const pauseTrack = (): void => {
  playerStore.pause()
}

// 标题/艺术家显示:简单的 || 链式调用,无需缓存
// 组件内不再另建 Map:与 useTrackInfo.processedTracks 的共享 LRU 重叠,且大列表下是额外内存开销
const getTrackTitle = (track: Track): string => {
  return FileUtils.getTrackDisplayName(track, configStore.titleExtraction.hideFileExtension)
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
    const track = raw[i]!
    const existing = processedMap.get(track.path)

    // 复用已有对象（如果 path 和 coverPath 都没变）
    if (existing && existing.coverPath === track.coverPath) {
      result[i] = existing
    } else {
      result[i] = buildProcessedTrack(track)
      changed = true
    }
    newProcessedMap.set(track.path, result[i]!)
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
// 结构 watch：getter 只读取每项的 path，列表增删/移动/替换才触发 O(N) 处理；
// 封面等字段级 mutation 不再触发 deep watch 的 O(N) 级联（封面走下方版本号通道）
const stopWatchPlaylist = watch(
  () => {
    const raw = playlist.value
    const paths = new Array<string>(raw.length)
    for (let i = 0; i < raw.length; i++) paths[i] = raw[i]!.path
    return paths.join('\n')
  },
  processPlaylist,
  { immediate: true },
)

// ===== 封面增量更新 =====
// store 的 _loadPlaylistCovers 每处理完一批递增 playlistCoverVersion，
// 这里取走更新并就地修改 processedTrack（配合 triggerRef 与 v-memo，
// 只重渲染封面真正变化的项目），复杂度 O(变更数) 而非 O(N²)
watch(
  () => playerStore.playlistCoverVersion,
  () => {
    const updates = playerStore.takeCoverUpdates()
    if (updates.size === 0) return
    let applied = 0
    for (const [path, coverPath] of updates) {
      const pt = processedMap.get(path)
      if (pt && pt.coverPath !== coverPath) {
        pt.coverPath = coverPath
        pt.coverUrl = convertFileSrc(coverPath)
        applied++
      }
    }
    if (applied > 0) triggerRef(processedPlaylist)
  },
  { immediate: true },
)

// 滚动到当前播放的歌曲
const scrollToCurrentTrack = (): void => {
  if (!currentTrack.value || processedPlaylist.value.length === 0 || !scrollContainer.value) return

  const currentIndex = processedPlaylist.value.findIndex((t) => t.path === currentTrack.value!.path)
  if (currentIndex === -1) return

  void nextTick(() => {
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

// 组件挂载时滚动到当前歌曲
let hasScrolledOnMount = false
let stopWatchScrollOnMount: WatchStopHandle | null = null

onMounted(() => {
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

/**
 * 播放列表点击委托。
 */
const handleListClick = (event: MouseEvent): void => {
  const target = event.target as HTMLElement | null
  if (!target) return

  const row = target.closest<HTMLElement>('[data-path]')
  const path = row?.dataset.path
  if (!path) return

  const action = target.closest<HTMLElement>('[data-action]')?.dataset.action
  if (action === 'remove') {
    removeTrackByPath(path)
    return
  }
  if (action === 'pause') {
    pauseTrack()
    return
  }

  // 命中行本体或播放按钮:都由 path 反查 track(O(1))
  const track = processedMap.get(path)
  if (track) playTrack(track)
}
</script>

<style scoped>
.playlist-view {
  position: fixed;
  top: 0;
  right: 0;
  width: 400px;
  max-width: 90vw;
  height: 100%;
  background-color: var(--md-sys-color-surface);
  /* 与 MusicLibrary 保持一致:浮层面板需要 level2 阴影与内容区分 */
  box-shadow: var(--md-sys-elevation-level2);
  z-index: 1000;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  will-change: transform;
  /* transition: transform 0.4s cubic-bezier(0.25, 0.46, 0.45, 0.94); */
}

.playlist-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.playlist-header-left {
  display: flex;
  align-items: baseline;
  gap: 12px;
  min-width: 0;
}

.playlist-title {
  font-size: 24px;
  font-weight: 500;
  margin: 0;
  color: var(--md-sys-color-on-surface);
  white-space: nowrap;
}

/* 标题右侧的播放队列信息 */
.playlist-queue-info {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.playlist-content {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  padding: 0 8px 16px 8px;
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
  .playlist-view {
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
</style>
