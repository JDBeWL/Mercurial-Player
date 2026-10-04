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
        <!-- 点击委托：整份列表只在容器上挂 1 个 click，行用 data-path、按钮用 data-action -->
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

// Track 加显示用的缓存字段（标题 / 艺术家 / 封面 URL）
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

// 队列信息开关走 config.general.showQueueInfo，缺省视为开
const showQueueInfo = computed(() => configStore.general.showQueueInfo !== false)

// 第 X 首 / 共 Y 首，后面再拼总时长
const queueInfoText = computed(() => {
  const base = t('player.queueInfo', {
    current: currentTrackIndex.value + 1,
    total: playlist.value.length,
  })

  // 总时长（秒），有时长数据才附加
  const total = playlist.value.reduce((sum, track) => sum + (track.duration || 0), 0)
  if (!total) return base

  return `${base} · ${formatTime(total)}`
})

const scrollContainer = ref<HTMLElement | null>(null)

// 滚动期间用 .is-scrolling 关掉 hover，免得每滚动一帧都重绘；150ms 没 scroll 事件算停下
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

const handleClose = (): void => {
  emit('close')
}

// 只比对当前曲目的 path，O(1) 而不是整表 O(N)
const currentPath = computed<string | null>(() => currentTrack.value?.path || null)

// 该行是否为当前正在播放的曲目，决定播放按钮显示播放还是暂停。
// 播放 / 暂停原是两个互斥的 v-if 按钮，未命中的那个会在 DOM 留一个注释占位节点，合并后每行少一个
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

// 组件内不另建标题缓存 Map：与 useTrackInfo.processedTracks 的共享 LRU 重叠，大列表下是额外内存开销
const getTrackTitle = (track: Track): string => {
  return FileUtils.getTrackDisplayName(track, configStore.titleExtraction.hideFileExtension)
}

const getTrackArtist = (track: Track): string => {
  return track.displayArtist || track.artist || ''
}

// shallowRef：不对整表做深度响应，封面增量靠就地改对象 + triggerRef 通知渲染
const processedPlaylist = shallowRef<ProcessedTrack[]>([])

// path -> processedTrack 索引，供增量复用与点击反查
let processedMap = new Map<string, ProcessedTrack>()

const buildProcessedTrack = (track: Track): ProcessedTrack => ({
  ...track,
  cachedTitle: getTrackTitle(track),
  cachedArtist: getTrackArtist(track),
  coverUrl: track.coverPath ? convertFileSrc(track.coverPath) : undefined,
})

// 增量重建播放列表，只重算变化项
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

    // path 与 coverPath 都没变就复用旧对象，保住 v-memo 的引用相等
    if (existing && existing.coverPath === track.coverPath) {
      result[i] = existing
    } else {
      result[i] = buildProcessedTrack(track)
      changed = true
    }
    newProcessedMap.set(track.path, result[i]!)
  }

  // 只有长度变化或确有新增 / 修改项才换掉数组引用，避免每次 playlist 变动都重渲染
  if (
    changed ||
    result.length !== processedPlaylist.value.length ||
    processedMap.size !== newProcessedMap.size
  ) {
    processedPlaylist.value = result
  }
  processedMap = newProcessedMap
}

// 结构 watch：getter 只读每项的 path，列表增删 / 移动 / 替换才触发 O(N) 处理；
// 封面等字段级 mutation 不再触发 deep watch 的 O(N) 级联（封面走下面的版本号通道）
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

// 封面增量通道：store 的 _loadPlaylistCovers 每批处理完递增 playlistCoverVersion，
// 这里取走更新并就地改 processedTrack（配合 triggerRef 与 v-memo，只重渲染变化的那几项），
// 代价只随变化数增长，不会每批都全量重排整个列表。
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

// 滚到当前曲目：nextTick 等 DOM 补丁完成，否则 querySelectorAll 数不到刚渲染的行
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

// 数据可能晚于挂载到达：等 processedPlaylist 首次非空再滚一次，然后停掉这个 watch
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

// 卸载：解开两个 watch 和滚动防抖，再清掉缓存
onUnmounted(() => {
  if (scrollTimeout) {
    clearTimeout(scrollTimeout)
    scrollTimeout = null
  }

  stopWatchPlaylist()
  stopWatchScrollOnMount?.()

  processedMap.clear()
  processedMap = new Map()
  processedPlaylist.value = []
})

const removeTrackByPath = (path: string): void => {
  playerStore.removeTrack(path)
}

/** 委托分派：remove / pause 由 data-action 命中，行本体和播放按钮都用 path 反查 track（O(1)） */
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
  /* 与 MusicLibrary 保持一致：浮层面板要 level2 阴影才分得清内容区 */
  box-shadow: var(--md-sys-elevation-level2);
  z-index: 1000;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  will-change: transform;
  /* fixed 相对视口定位，不吃 #app 的安全区 padding，安卓边到边时会顶进状态栏 / 手势条，
     这里单独让出来 */
  padding-top: env(safe-area-inset-top, 0px);
  padding-bottom: env(safe-area-inset-bottom, 0px);
  box-sizing: border-box;
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

@media (hover: hover) {
  .list-item:hover {
    background-color: var(--md-sys-color-hover-overlay);
  }
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

/* 竖屏（手机）：400px 定宽抽屉会露出左侧背景，看着像没铺满的浮层，改成整屏。
   与上面的 480px 规则分开写：那条按宽度收窄，这条按方向 */
@media (orientation: portrait) {
  .playlist-view[data-mobile='true'] {
    width: 100vw;
    max-width: 100vw;
  }
}
</style>
