<template>
  <div class="progress-container">
    <div class="progress-bar-wrapper"
         ref="progressBarWrapper"
         @mousedown="handleMouseDown"
         @mouseenter="isHovering = true"
         @mouseleave="handleMouseLeave"
         @mousemove="handleMouseMoveHover"
         :class="{ 'is-hovering': isHovering, 'is-dragging': isDragging }">
      <div class="progress-bar" ref="progressBar">
        <div class="progress-bar-fill" :style="{ width: `${displayPercent}%` }"></div>
        <div class="progress-bar-handle" :style="{ left: `${displayPercent}%` }"></div>
        <!-- 悬停/拖动时间提示 - 跟随真实滑柄位置 -->
        <div v-if="isHovering || isDragging" class="hover-time-tooltip" :style="{ left: `clamp(var(--tooltip-half-width), ${displayPercent}%, calc(100% - var(--tooltip-half-width)))` }">
          {{ formatTime(displayTime) }} / {{ formatTime(playerStore.duration) }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onUnmounted, watch } from 'vue'
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()
const progressBar = ref<HTMLElement | null>(null)
const progressBarWrapper = ref<HTMLElement | null>(null)
const isDragging = ref(false)
const isHovering = ref(false)
const dragPercent = ref(0)
const pendingSeek = ref(false) // 标记是否有待完成的 seek 操作
const hoverPercent = ref(0)
const hoverTime = computed(() => {
  if (playerStore.duration === 0) return 0
  return (hoverPercent.value / 100) * playerStore.duration
})

// 提示显示的位置和时间的计算属性
const tooltipPercent = computed(() => {
  if (isDragging.value) return dragPercent.value
  return hoverPercent.value
})

const tooltipTime = computed(() => {
  if (playerStore.duration === 0) return 0
  if (isDragging.value) return (dragPercent.value / 100) * playerStore.duration
  return (hoverPercent.value / 100) * playerStore.duration
})

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

const updateDragPosition = (event: MouseEvent) => {
  if (!progressBarWrapper.value) return
  const rect = progressBarWrapper.value.getBoundingClientRect()
  const percent = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100))
  dragPercent.value = percent
}

const handleMouseDown = (event: MouseEvent) => {
  if (playerStore.duration === 0) return
  isDragging.value = true
  pendingSeek.value = false // 重置
  updateDragPosition(event)
  
  document.addEventListener('mousemove', handleMouseMove)
  document.addEventListener('mouseup', handleMouseUp)
}

const handleMouseMove = (event: MouseEvent) => {
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

const handleMouseMoveHover = (event: MouseEvent) => {
  if (!progressBarWrapper.value || isDragging.value) return
  const rect = progressBarWrapper.value.getBoundingClientRect()
  const percent = Math.max(0, Math.min(100, ((event.clientX - rect.left) / rect.width) * 100))
  hoverPercent.value = percent
}

const handleMouseLeave = () => {
  if (!isDragging.value) {
    isHovering.value = false
  }
}

const formatTime = (seconds: number): string => {
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
  margin-bottom: 12px;
}

.progress-bar-wrapper {
  width: 100%;
  height: 16px;
  display: flex;
  align-items: center;
  cursor: pointer;
  position: relative;
}

.progress-bar {
  width: 100%;
  height: 2px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: 1px;
  overflow: visible;
  position: relative;
  transition: transform 0.2s ease;
  transform-origin: center center;
}

/* 悬停时扩大进度条高度 - 使用 transform 避免影响布局 */
.progress-bar-wrapper.is-hovering .progress-bar,
.progress-bar-wrapper.is-dragging .progress-bar {
  transform: scaleY(2);
}

.progress-bar-fill {
  position: absolute;
  top: 0;
  left: 0;
  height: 100%;
  background-color: var(--md-sys-color-primary);
  border-radius: inherit;
  transition: width 0.1s linear;
}

/* 拖动手柄 - 网易云风格小圆点 */
.progress-bar-handle {
  position: absolute;
  top: 50%;
  width: 8px;
  height: 8px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  transform: translate(-50%, -50%) scale(0);
  transition: transform 0.2s ease, opacity 0.2s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  pointer-events: none;
  opacity: 0;
}

/* 悬停时显示手柄 - 添加 scaleY(0.5) 抵消父元素的 scaleY(2) 保持圆形 */
.progress-bar-wrapper.is-hovering .progress-bar-handle,
.progress-bar-wrapper.is-dragging .progress-bar-handle {
  opacity: 1;
  transform: translate(-50%, -50%) scale(1) scaleY(0.5);
}

/* 拖动时手柄放大 */
.progress-bar-wrapper.is-dragging .progress-bar-handle {
  transform: translate(-50%, -50%) scale(1.2) scaleY(0.5);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
}

/* 拖动时的视觉反馈 */
.progress-bar-wrapper.is-dragging {
  cursor: grabbing;
}

.progress-bar-wrapper.is-dragging .progress-bar-fill {
  transition: none;
}

/* 悬停时间提示 */
.hover-time-tooltip {
  position: absolute;
  top: -28px;
  /* tooltip 半宽,用于 clamp 边界处理;内容约 80px,半宽约 40px,留余量取 45px */
  --tooltip-half-width: 45px;
  transform: translateX(-50%) scaleY(0.5);
  transform-origin: center bottom;
  background-color: var(--md-sys-color-inverse-surface);
  color: var(--md-sys-color-inverse-on-surface);
  font-size: 11px;
  padding: 4px 8px;
  border-radius: 4px;
  white-space: nowrap;
  pointer-events: none;
  z-index: 100;
  opacity: 0;
  animation: tooltipFadeIn 0.15s ease forwards;
}

@keyframes tooltipFadeIn {
  from {
    opacity: 0;
    transform: translateX(-50%) scaleY(0.5) translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateX(-50%) scaleY(0.5) translateY(0);
  }
}


</style>