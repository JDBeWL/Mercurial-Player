<template>
  <div class="mini-player" data-tauri-drag-region>
    <!-- 背景模糊封面 -->
    <div
      class="background-cover"
      data-tauri-drag-region
      :style="{ backgroundImage: currentTrackCover }"
    ></div>
    <div class="background-overlay" data-tauri-drag-region></div>

    <!-- 主要内容 -->
    <div class="content-container" data-tauri-drag-region>
      <!-- 左侧：封面 -->
      <div class="cover-container" data-tauri-drag-region>
        <div class="cover" data-tauri-drag-region :style="{ backgroundImage: currentTrackCover }">
          <div
            v-if="!currentTrack || !currentTrack.coverPath"
            class="cover-placeholder"
            data-tauri-drag-region
          >
            <span class="material-symbols-rounded">album</span>
          </div>
        </div>
        <!-- 悬浮遮罩：恢复按钮 -->
        <div
          class="cover-overlay"
          data-tauri-drag-region="false"
          :title="$t('player.restoreMainWindow')"
          @click="exitMiniMode"
        >
          <span class="material-symbols-rounded">open_in_full</span>
        </div>
      </div>

      <!-- 中间：信息和控制 -->
      <div class="info-controls" data-tauri-drag-region>
        <!-- 歌曲信息 -->
        <div class="track-info" data-tauri-drag-region>
          <div
            class="track-title"
            data-tauri-drag-region
            :title="getTrackTitle(currentTrack, $t('player.noTrack'))"
          >
            {{ getTrackTitle(currentTrack, $t('player.noTrack')) }}
          </div>
          <div
            class="track-artist"
            data-tauri-drag-region
            :title="getTrackArtist(currentTrack, '')"
          >
            {{ getTrackArtist(currentTrack, '') }}
          </div>
        </div>

        <!-- 控制按钮：容器可拖拽，按钮本身不可拖拽 -->
        <div class="controls" data-tauri-drag-region>
          <button
            class="icon-button small"
            data-tauri-drag-region="false"
            :disabled="!playerStore.hasPreviousTrack"
            @click="playerStore.previousTrack"
          >
            <span class="material-symbols-rounded">skip_previous</span>
          </button>

          <button
            class="icon-button play-pause"
            data-tauri-drag-region="false"
            @click="playerStore.togglePlay"
          >
            <span class="material-symbols-rounded filled">{{
              playerStore.isPlaying ? 'pause' : 'play_arrow'
            }}</span>
          </button>

          <button
            class="icon-button small"
            data-tauri-drag-region="false"
            :disabled="!playerStore.hasNextTrack"
            @click="playerStore.nextTrack"
          >
            <span class="material-symbols-rounded">skip_next</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 底部进度条 (拖拽由 useDragValue 的 document 级监听驱动,鼠标移出不会中断) -->
    <div
      ref="progressContainer"
      class="progress-bar-container"
      :class="{ dragging: isDragging }"
      data-tauri-drag-region="false"
      @mousedown="startSeeking"
    >
      <div class="progress-fill" :style="{ width: progressPercentage + '%' }"></div>
      <!-- 时间预览提示 -->
      <div v-if="isDragging" class="time-tooltip" :style="{ left: dragPercentage + '%' }">
        {{ formatTime(previewTime) }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from '../stores/player'
import { useConfigStore } from '../stores/config'
import { useTrackInfo } from '../composables/useTrackInfo'
import { useDragValue } from '../composables/useDragValue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { formatTime } from '../utils/format'

const playerStore = usePlayerStore()
const configStore = useConfigStore()
const { currentTrack, currentTime, duration } = storeToRefs(playerStore)

// 使用 composable 处理音轨信息
const { getTrackTitle, getTrackArtist, watchTrack } = useTrackInfo()

// 监听当前音轨变化
watchTrack(() => currentTrack.value)

// 拖拽进度条: document 级监听骨架由 useDragValue 提供,
// 拖拽中只更新预览位置,松手时才应用 seek
const progressContainer = ref<HTMLElement | null>(null)
const dragPercentage = ref<number>(0)
// seek 目标时间: 松手后先显示落点,等 currentTime 追上再恢复跟随播放,
// 避免后端尚未推回新位置时进度条闪回旧值
const pendingSeekTime = ref<number | null>(null)
let pendingSeekTimeoutId: ReturnType<typeof setTimeout> | null = null

const clearPendingSeek = (): void => {
  pendingSeekTime.value = null
  if (pendingSeekTimeoutId) {
    clearTimeout(pendingSeekTimeoutId)
    pendingSeekTimeoutId = null
  }
}

watch(
  () => currentTime.value,
  (t) => {
    if (pendingSeekTime.value != null && Math.abs(t - pendingSeekTime.value) < 0.5) {
      clearPendingSeek()
    }
  },
)

const { isDragging, startDrag: startSeeking } = useDragValue({
  getPercent: (event) => {
    if (!progressContainer.value) return 0
    const rect = progressContainer.value.getBoundingClientRect()
    const x = Math.max(0, Math.min(event.clientX - rect.left, rect.width))
    return x / rect.width
  },
  onStart: (percent) => {
    clearPendingSeek()
    dragPercentage.value = percent * 100
  },
  onMove: (percent) => {
    dragPercentage.value = percent * 100
  },
  onEnd: (percent) => {
    dragPercentage.value = 0 // 重置拖拽位置，避免下次拖拽开始时闪现旧位置
    // 应用新的播放位置
    if (duration.value) {
      const target = percent * duration.value
      pendingSeekTime.value = target
      // 超时保护: seek 被拒/无响应时 1 秒后恢复跟随播放位置
      pendingSeekTimeoutId = setTimeout(clearPendingSeek, 1000)
      playerStore.seek(target)
    }
  },
})

// 计算属性
const currentTrackCover = computed<string>(() => {
  if (currentTrack.value && currentTrack.value.coverPath) {
    // 使用 convertFileSrc 将本地文件路径转换为可渲染的 URL
    return `url('${convertFileSrc(currentTrack.value.coverPath)}')`
  }
  return 'none'
})

const progressPercentage = computed<number>(() => {
  if (isDragging.value) return dragPercentage.value
  // 松手后到 currentTime 追上前,停在落点位置
  if (pendingSeekTime.value != null && duration.value) {
    return (pendingSeekTime.value / duration.value) * 100
  }
  if (!duration.value) return 0
  return (currentTime.value / duration.value) * 100
})

// 预览时间（拖拽时显示）
const previewTime = computed<number>(() => {
  if (!duration.value) return 0
  return (dragPercentage.value / 100) * duration.value
})

// 方法
const exitMiniMode = (): void => {
  configStore.toggleMiniMode()
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
  font-family: 'Roboto', 'Roboto Fallback', sans-serif;
}

/* 背景模糊效果 */
.background-cover {
  position: absolute;
  top: -20px;
  left: -20px;
  right: -20px;
  bottom: -20px;
  background-size: cover;
  background-position: center;
  filter: blur(30px) saturate(1.2);
  opacity: 0.4;
  z-index: 0;
}

.background-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background: linear-gradient(
    135deg,
    rgba(var(--md-sys-color-surface-rgb, 0, 0, 0), 0.6) 0%,
    rgba(var(--md-sys-color-surface-rgb, 0, 0, 0), 0.3) 100%
  );
  z-index: 1;
}

.content-container {
  position: relative;
  z-index: 2;
  flex: 1;
  display: flex;
  align-items: center;
  padding: 8px 12px;
  gap: 12px;
  min-width: 0;
}

/* 封面样式 */
.cover-container {
  position: relative;
  height: calc(100% - 10px);
  aspect-ratio: 1;
  flex-shrink: 0;
  border-radius: 8px;
  overflow: hidden;
  box-shadow: 0 3px 10px rgba(0, 0, 0, 0.2);
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
  opacity: 0.6;
}

.cover-placeholder .material-symbols-rounded {
  font-size: 26px;
}

.cover-overlay {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: opacity 0.2s ease;
  color: var(--md-sys-color-on-surface);
}

.cover-overlay .material-symbols-rounded {
  font-size: 22px;
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
  gap: 1px;
}

.track-title {
  font-size: 14px;
  font-weight: 500;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--md-sys-color-on-surface);
}

.track-artist {
  font-size: 12px;
  font-weight: 400;
  line-height: 1.2;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  opacity: 0.85;
}

.controls {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-top: 2px;
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
  transition: all 0.15s ease;
}

.icon-button:hover:not(:disabled) {
  background-color: var(--md-sys-color-surface-variant);
}

.icon-button:active:not(:disabled) {
  transform: scale(0.95);
}

.icon-button:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.icon-button.small .material-symbols-rounded {
  font-size: 20px;
}

.icon-button.play-pause {
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  padding: 6px;
  margin: 0 2px;
}

.icon-button.play-pause:hover {
  background-color: var(--md-sys-color-primary);
  filter: brightness(1.1);
}

.icon-button.play-pause .material-symbols-rounded {
  font-size: 20px;
}

/* 进度条样式 */
.progress-bar-container {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  width: 100%;
  height: 3px;
  background-color: var(--md-sys-color-surface-variant);
  cursor: pointer;
  z-index: 10;
  flex-shrink: 0;
  transition: height 0.15s ease;
}

.progress-bar-container:hover {
  height: 5px;
}

.progress-fill {
  height: 100%;
  background-color: var(--md-sys-color-primary);
  transition: width 0.1s linear;
}

/* 拖拽中禁用过渡,填充条即时跟随鼠标 (0.1s 缓动会造成明显不跟手) */
.progress-bar-container.dragging .progress-fill {
  transition: none;
}

/* 时间预览提示 */
.time-tooltip {
  position: absolute;
  bottom: 100%;
  transform: translateX(-50%);
  margin-bottom: 8px;
  padding: 4px 8px;
  background-color: var(--md-sys-color-inverse-surface);
  color: var(--md-sys-color-inverse-on-surface);
  font-size: 11px;
  font-weight: 500;
  border-radius: 4px;
  white-space: nowrap;
  pointer-events: none;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
  z-index: 100;
}

.time-tooltip::after {
  content: '';
  position: absolute;
  top: 100%;
  left: 50%;
  transform: translateX(-50%);
  border: 4px solid transparent;
  border-top-color: var(--md-sys-color-inverse-surface);
}

.material-symbols-rounded.filled {
  font-variation-settings:
    'FILL' 1,
    'wght' 400,
    'GRAD' 0,
    'opsz' 24;
}
</style>
