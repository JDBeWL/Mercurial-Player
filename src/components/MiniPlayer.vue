<template>
  <div class="mini-player" data-tauri-drag-region>
    <!-- 背景模糊封面 -->
    <div class="background-cover" :style="{ backgroundImage: currentTrackCover }"></div>
    <div class="background-overlay"></div>

    <!-- 主要内容 -->
    <div class="content-container" data-tauri-drag-region>
      <!-- 左侧：封面 -->
      <div class="cover-container" data-tauri-drag-region>
        <div class="cover" :style="{ backgroundImage: currentTrackCover }">
          <div v-if="!currentTrack || !currentTrack.cover" class="cover-placeholder">
            <span class="material-symbols-rounded">music_note</span>
          </div>
        </div>
        <!-- 悬浮遮罩：恢复按钮 -->
        <div class="cover-overlay" @click="exitMiniMode" title="恢复主界面">
          <span class="material-symbols-rounded">open_in_full</span>
        </div>
      </div>

      <!-- 中间：信息和控制 -->
      <div class="info-controls" data-tauri-drag-region>
        <!-- 歌曲信息 -->
        <div class="track-info" data-tauri-drag-region>
          <div class="track-title" :title="getTrackTitle(currentTrack)">{{ getTrackTitle(currentTrack) }}</div>
          <div class="track-artist" :title="getTrackArtist(currentTrack)">{{ getTrackArtist(currentTrack) }}</div>
        </div>

        <!-- 控制按钮 -->
        <div class="controls" data-tauri-drag-region>
          <button class="icon-button small" @click="playerStore.previousTrack" :disabled="!playerStore.hasPreviousTrack">
            <span class="material-symbols-rounded">skip_previous</span>
          </button>
          
          <button class="icon-button play-pause" @click="playerStore.togglePlay">
            <span class="material-symbols-rounded filled">{{ playerStore.isPlaying ? 'pause' : 'play_arrow' }}</span>
          </button>
          
          <button class="icon-button small" @click="playerStore.nextTrack" :disabled="!playerStore.hasNextTrack">
            <span class="material-symbols-rounded">skip_next</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 底部进度条 -->
    <div class="progress-bar-container" 
         @mousedown="startSeeking"
         @mousemove="handleSeekMove"
         @mouseup="endSeeking"
         @mouseleave="endSeeking">
      <div class="progress-background"></div>
      <div class="progress-fill" :style="{ width: progressPercentage + '%' }"></div>
    </div>
  </div>
</template>

<script setup>
import { computed, ref, onMounted, onUnmounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import FileUtils from '../utils/fileUtils'
import { TitleExtractor } from '../utils/titleExtractor'

const playerStore = usePlayerStore()
const configStore = useConfigStore()
const { currentTrack, currentTime, duration, isPlaying } = storeToRefs(playerStore)

// 拖拽进度条相关状态
const isDragging = ref(false)
const dragPercentage = ref(0)

// 存储处理后的音轨信息
const processedTracks = ref({})

// 计算属性
const currentTrackCover = computed(() => {
  if (currentTrack.value && currentTrack.value.cover) {
    return `url('${currentTrack.value.cover}')`
  }
  return 'none'
})

// 获取音轨标题
const getTrackTitle = (track) => {
  if (!track || !track.path) {
    return '未播放'
  }

  const trackPath = track.path

  // 如果已经处理过该音轨，直接返回结果
  if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
    return processedTracks.value[trackPath].title
  }
  
  // 异步处理音轨信息，但不阻塞当前渲染
  if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
    processTrackInfo(trackPath)
  }

  // 如果还没处理完，暂时返回track中已有的name或文件名
  return track.name || FileUtils.getFileName(trackPath)
}

// 获取音轨艺术家
const getTrackArtist = (track) => {
  if (!track || !track.path) {
    return ''
  }

  const trackPath = track.path

  // 如果已经处理过该音轨，直接返回结果
  if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
    return processedTracks.value[trackPath].artist
  }

  // 异步处理音轨信息，但不阻塞当前渲染
  if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
    processTrackInfo(trackPath)
  }

  // 如果还没处理完，暂时返回track中已有的artist信息
  return track.artist || '未知艺术家'
}

// 异步处理音轨信息
const processTrackInfo = async (trackPath) => {
  try {
    // 如果已经在处理中，跳过
    if (processedTracks.value[trackPath]?.processing) return

    // 标记为处理中
    processedTracks.value[trackPath] = { processing: true }

    // 获取配置
    const config = {
      preferMetadata: configStore.titleExtraction?.preferMetadata ?? true,
      hideFileExtension: configStore.titleExtraction?.hideFileExtension ?? true,
      parseArtistTitle: configStore.titleExtraction?.parseArtistTitle ?? true,
      separator: configStore.titleExtraction?.separator ?? '-',
      customSeparators: configStore.titleExtraction?.customSeparators ?? ['-', '_', '.', ' ']
    }

    // 使用 TitleExtractor 智能提取标题信息
    const titleInfo = await TitleExtractor.extractTitle(trackPath, config)

    // 更新处理结果
    processedTracks.value[trackPath] = {
      processing: false,
      ...titleInfo
    }

  } catch (error) {
    console.error('处理音轨信息失败:', trackPath, error)
    // 出错时使用文件名作为标题
    processedTracks.value[trackPath] = {
      processing: false,
      title: FileUtils.getFileName(trackPath),
      artist: '',
      fileName: FileUtils.getFileName(trackPath),
      isFromMetadata: false
    }
  }
}

// 监听当前音轨变化，自动处理标题信息
watch(currentTrack, (newTrack) => {
  if (newTrack && newTrack.path) {
    processTrackInfo(newTrack.path)
  }
}, { immediate: true })

const progressPercentage = computed(() => {
  if (isDragging.value) return dragPercentage.value
  if (!duration.value) return 0
  return (currentTime.value / duration.value) * 100
})

// 方法
const exitMiniMode = () => {
  configStore.toggleMiniMode()
}

const startSeeking = (e) => {
  isDragging.value = true
  updateDragPosition(e)
}

const handleSeekMove = (e) => {
  if (isDragging.value) {
    updateDragPosition(e)
  }
}

const endSeeking = (e) => {
  if (isDragging.value) {
    isDragging.value = false
    // 应用新的播放位置
    if (duration.value) {
      const rect = e.currentTarget.getBoundingClientRect()
      const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width))
      const percentage = x / rect.width
      playerStore.seek(percentage * duration.value)
    }
  }
}

const updateDragPosition = (e) => {
  const rect = e.currentTarget.getBoundingClientRect()
  const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width))
  dragPercentage.value = (x / rect.width) * 100
}
</script>

<style scoped>
.mini-player {
  position: relative;
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background-color: var(--md-sys-color-surface);
  color: var(--md-sys-color-on-surface);
  user-select: none;
}

/* 背景模糊效果 */
.background-cover {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-size: cover;
  background-position: center;
  filter: blur(20px);
  opacity: 0.3;
  z-index: 0;
}

.background-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.2);
  z-index: 1;
}

.content-container {
  position: relative;
  z-index: 2;
  flex: 1;
  display: flex;
  align-items: center;
  padding: 12px;
  gap: 12px;
  min-width: 0; /* 防止flex子项溢出 */
}

/* 封面样式 */
.cover-container {
  position: relative;
  width: 64px;
  height: 64px;
  flex-shrink: 0;
  border-radius: 8px;
  overflow: hidden;
  box-shadow: 0 4px 8px rgba(0, 0, 0, 0.2);
  cursor: pointer;
}

.cover {
  width: 100%;
  height: 100%;
  background-size: cover;
  background-position: center;
  background-color: var(--md-sys-color-surface-variant);
  display: flex;
  align-items: center;
  justify-content: center;
}

.cover-placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

.cover-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity 0.2s ease;
  color: white;
}

.cover-container:hover .cover-overlay {
  opacity: 1;
}

/* 信息和控制样式 */
.info-controls {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: center;
  min-width: 0;
  gap: 4px;
}

.track-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.track-title {
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--md-sys-color-on-surface);
}

.track-artist {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.controls {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 4px;
}

.icon-button {
  background: none;
  border: none;
  padding: 4px;
  border-radius: 50%;
  color: var(--md-sys-color-on-surface);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.2s;
}

.icon-button:hover {
  background-color: var(--md-sys-color-surface-variant);
}

.icon-button:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.icon-button.small .material-symbols-rounded {
  font-size: 20px;
}

.icon-button.play-pause {
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  padding: 6px;
}

.icon-button.play-pause:hover {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.icon-button.play-pause .material-symbols-rounded {
  font-size: 24px;
}

/* 进度条样式 */
.progress-bar-container {
  position: relative;
  width: 100%;
  height: 4px;
  background-color: var(--md-sys-color-surface-variant);
  cursor: pointer;
  z-index: 3;
}

.progress-bar-container:hover {
  height: 6px;
}

.progress-fill {
  height: 100%;
  background-color: var(--md-sys-color-primary);
  transition: width 0.1s linear;
}

.material-symbols-rounded.filled {
  font-variation-settings: 'FILL' 1, 'wght' 400, 'GRAD' 0, 'opsz' 24;
}
</style>
