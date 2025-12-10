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
            v-for="(track, index) in processedPlaylist" 
            :key="index"
            class="list-item"
            :class="{ selected: isCurrentTrack(track) }"
            @click="playTrack(track)"
          >
            <div class="list-item-leading">
              <span 
                class="material-sounds-playing-icon" 
                v-if="isCurrentTrack(track) && playerStore.isPlaying"
              >
                <span class="bar bar-1"></span>
                <span class="bar bar-2"></span>
                <span class="bar bar-3"></span>
              </span>
              <span class="material-symbols-rounded playing-icon" v-else-if="isCurrentTrack(track)">
                equalizer
              </span>
              <span class="material-symbols-rounded" v-else>
                music_note
              </span>
            </div>
            <div class="list-item-content">
              <div class="list-item-headline" :title="track.cachedTitle">{{ track.cachedTitle }}</div>
              <div class="list-item-supporting" :title="track.cachedArtist">{{ track.cachedArtist }}</div>
            </div>
            <div class="list-item-trailing">
              <button class="icon-button" @click.stop="removeTrack(index)">
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
import { ref, watch, onMounted, computed } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import FileUtils from '../utils/fileUtils'
import { TitleExtractor } from '../utils/titleExtractor'

const emit = defineEmits(['close'])

const playerStore = usePlayerStore()
const configStore = useConfigStore()
const { playlist, currentTrack, isPlaying } = storeToRefs(playerStore)

// 控制动画状态
const isClosing = ref(false)

// 关闭动画处理
const handleClose = () => {
  isClosing.value = true
  setTimeout(() => {
    emit('close')
  }, 300) // 与CSS动画时间一致
}



const isCurrentTrack = (track) => {
  return currentTrack.value && currentTrack.value.path === track.path
}

const playTrack = (track) => {
  playerStore.playTrack(track)
}

// 创建标题缓存
const titleCache = new Map()
const artistCache = new Map()

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
  return artist
}

// 创建计算属性以提高性能
const processedPlaylist = computed(() => {
  return playlist.value.map(track => ({
    ...track,
    cachedTitle: getTrackTitle(track),
    cachedArtist: getTrackArtist(track)
  }))
})

const removeTrack = (index) => {
  // 创建一个新的播放列表，并移除指定索引的音轨
  const newPlaylist = [...playlist.value]
  newPlaylist.splice(index, 1)
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
  /* 启用硬件加速和优化滚动性能 */
  transform: translateZ(0);
  -webkit-overflow-scrolling: touch;
  will-change: scroll-position;
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
  /* 优化列表渲染性能 */
  contain: layout style paint;
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
  will-change: background-color; /* 优化性能 */
}

.list-item:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.list-item.selected {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  border-radius: 8px;
  z-index: 1;
}

.list-item-leading {
  margin-right: 16px;
  color: var(--md-sys-color-on-surface-variant);
}

.list-item.selected .list-item-leading {
  color: var(--theme-on-primary-container);
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
  color: var(--theme-on-primary-container);
}

.list-item-supporting {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.list-item.selected .list-item-supporting {
  color: var(--theme-on-primary-container);
}


.list-item-trailing {
  display: flex;
  gap: 8px;
}

/* 声音播放动画图标 */
.material-sounds-playing-icon {
  display: inline-flex;
  align-items: flex-end;
  height: 24px;
  width: 24px;
  color: inherit;
}

@media (max-width: 480px) {
  .list-item-headline {
    font-size: 14px;
  }
  
  .list-item-supporting {
    font-size: 12px;
  }
}

.bar {
  display: inline-block;
  width: 3px;
  margin: 0 1px;
  background-color: currentColor;
  border-radius: 3px;
  animation: sound-wave 0.6s infinite ease-in-out;
}

.bar-1 {
  height: 6px;
  animation-delay: 0s;
}

.bar-2 {
  height: 12px;
  animation-delay: 0.2s;
}

.bar-3 {
  height: 8px;
  animation-delay: 0.4s;
}

@keyframes sound-wave {
  0%, 100% {
    transform: scaleY(0.5);
    opacity: 0.7;
  }
  50% {
    transform: scaleY(1);
    opacity: 1;
  }
}

.list-item.selected .material-sounds-playing-icon,
.list-item.selected .playing-icon {
  color: var(--theme-on-primary-container);
}
</style>