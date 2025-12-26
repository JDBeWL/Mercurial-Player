<template>
  <div class="progress-container">
    <div class="progress-bar-wrapper"
         ref="progressBarWrapper"
         @mousedown="handleMouseDown"
         @mouseenter="isHovering = true"
         @mouseleave="handleMouseLeave"
         :class="{ 'is-hovering': isHovering, 'is-dragging': isDragging }">
      <div class="progress-bar" ref="progressBar">
        <div class="progress-bar-fill" :style="{ width: `${displayPercent}%` }"></div>
        <div class="progress-bar-handle" :style="{ left: `${displayPercent}%` }"></div>
      </div>
    </div>
    <div class="time-display">
      <span class="time-current">{{ formatTime(displayTime) }}</span>
      <span class="time-duration">{{ formatTime(playerStore.duration) }}</span>
    </div>
  </div>
</template>

<script setup>
import { computed, ref, onUnmounted, watch } from 'vue'
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()
const progressBar = ref(null)
const progressBarWrapper = ref(null)
const isDragging = ref(false)
const isHovering = ref(false)
const dragPercent = ref(0)
const pendingSeek = ref(false) // 标记是否有待完成的 seek 操作

const progressPercent = computed(() => {
  if (playerStore.duration === 0) return 0
  return (playerStore.currentTime / playerStore.duration) * 100
})

const displayPercent = computed(() => {
  // 拖动中或等待 seek 完成时，显示拖动位置
  if (isDragging.value || pendingSeek.value) {
    return dragPercent.value
  }
  return progressPercent.value
})

const displayTime = computed(() => {
  if (isDragging.value || pendingSeek.value) {
    return (dragPercent.value / 100) * playerStore.duration
  }
  return playerStore.currentTime
})

// 监听 currentTime 变化，当接近目标位置时取消 pendingSeek
watch(() => playerStore.currentTime, (newTime) => {
  if (pendingSeek.value && playerStore.duration > 0) {
    const targetTime = (dragPercent.value / 100) * playerStore.duration
    // 当实际时间接近目标时间（误差 0.5 秒内），取消等待状态
    if (Math.abs(newTime - targetTime) < 0.5) {
      pendingSeek.value = false
    }
  }
})

const updateDragPosition = (event) => {
  if (!progressBarWrapper.value) return
  const rect = progressBarWrapper.value.getBoundingClientRect()
  const percent = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100))
  dragPercent.value = percent
}

const handleMouseDown = (event) => {
  if (playerStore.duration === 0) return
  isDragging.value = true
  pendingSeek.value = false // 重置
  updateDragPosition(event)
  
  document.addEventListener('mousemove', handleMouseMove)
  document.addEventListener('mouseup', handleMouseUp)
}

const handleMouseMove = (event) => {
  if (isDragging.value) {
    updateDragPosition(event)
  }
}

const handleMouseUp = () => {
  if (isDragging.value) {
    isDragging.value = false
    
    // 应用新的播放位置
    if (playerStore.duration > 0) {
      const newTime = (dragPercent.value / 100) * playerStore.duration
      // 标记等待 seek 完成，保持显示位置
      pendingSeek.value = true
      playerStore.seek(newTime)
      
      // 超时保护：如果 1 秒后还没收到更新，取消等待
      setTimeout(() => {
        pendingSeek.value = false
      }, 1000)
    }
  }
  document.removeEventListener('mousemove', handleMouseMove)
  document.removeEventListener('mouseup', handleMouseUp)
}

const handleMouseLeave = () => {
  if (!isDragging.value) {
    isHovering.value = false
  }
}

const formatTime = (seconds) => {
  if (isNaN(seconds) || !isFinite(seconds)) return '0:00'
  
  const minutes = Math.floor(seconds / 60)
  const remainingSeconds = Math.floor(seconds % 60)
  return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`
}

onUnmounted(() => {
  document.removeEventListener('mousemove', handleMouseMove)
  document.removeEventListener('mouseup', handleMouseUp)
})
</script>

<style scoped>
.progress-container {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-bar-wrapper {
  width: 100%;
  padding: 6px 0;
  cursor: pointer;
  position: relative;
}

.progress-bar {
  width: 100%;
  height: 4px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: 2px;
  overflow: visible;
  position: relative;
  transition: transform 0.15s ease;
  transform-origin: center center;
}

/* 悬停时扩大进度条高度 - 使用 transform 避免重排 */
.progress-bar-wrapper.is-hovering .progress-bar,
.progress-bar-wrapper.is-dragging .progress-bar {
  transform: scaleY(1.5);
}

.progress-bar-fill {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  background-color: var(--md-sys-color-primary);
  border-radius: 2px;
  transition: width 0.1s linear;
}

/* 拖动手柄 */
.progress-bar-handle {
  position: absolute;
  top: 50%;
  width: 12px;
  height: 12px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  transform: translate(-50%, -50%);
  opacity: 0;
  transition: opacity 0.15s ease, transform 0.15s ease, box-shadow 0.15s ease;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.2);
  pointer-events: none;
}

/* 悬停时显示手柄 - 添加 scaleY(0.667) 抵消父元素的 scaleY(1.5) 保持圆形 */
.progress-bar-wrapper.is-hovering .progress-bar-handle,
.progress-bar-wrapper.is-dragging .progress-bar-handle {
  opacity: 1;
  transform: translate(-50%, -50%) scaleY(0.667);
}

/* 拖动时手柄放大 */
.progress-bar-wrapper.is-dragging .progress-bar-handle {
  transform: translate(-50%, -50%) scale(1.3) scaleY(0.667);
  box-shadow: 0 3px 8px rgba(0, 0, 0, 0.3);
}

/* 拖动时的视觉反馈 */
.progress-bar-wrapper.is-dragging {
  cursor: grabbing;
}

.progress-bar-wrapper.is-dragging .progress-bar-fill {
  transition: none;
}

.time-display {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}
</style>