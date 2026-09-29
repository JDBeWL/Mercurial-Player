<template>
  <div class="progress-container">
    <div
      ref="progressBarWrapper"
      class="progress-bar-wrapper"
      :class="{ 'is-hovering': isHovering, 'is-dragging': isDragging }"
      @pointerdown="handlePointerDown"
      @pointerup="handlePointerUp"
      @mouseenter="isHovering = true"
      @mouseleave="handleMouseLeave"
      @mousemove="handleMouseMoveHover"
    >
      <div class="progress-bar">
        <div class="progress-bar-fill" :style="{ width: `${displayPercent}%` }"></div>
        <div class="progress-bar-handle" :style="{ left: `${displayPercent}%` }"></div>
        <!-- 悬停/拖动时间提示 - 跟随真实滑柄位置 -->
        <div
          v-if="isHovering || isDragging"
          class="hover-time-tooltip"
          :style="{
            left: `clamp(var(--tooltip-half-width), ${displayPercent}%, calc(100% - var(--tooltip-half-width)))`,
          }"
        >
          {{ formatTime(displayTime) }} / {{ formatTime(playerStore.duration) }}
        </div>
      </div>
    </div>

    <!-- 已播 / 总时长。桌面靠悬停气泡显示时间，触摸设备没有 hover，竖屏下把这一行常显出来，否则手机上
         完全看不到播放进度时间。位置刻意留在进度条下方 18px：进度条的热区（::before）向下探了 14px，
         贴着放的话点时间文字会误触成"拖动进度条"。 -->
    <div class="progress-time-row">
      <span class="progress-time">{{ formatTime(displayTime) }}</span>
      <!-- 中间插槽：给"音频信息"这类与时间同高的附加信息留的位置（App.vue 用）。
           没有内容时它仍占 flex:1，把两端时间顶到边缘，布局不因有无内容而跳动。 -->
      <span class="progress-time-middle">
        <slot name="time-middle" />
      </span>
      <span class="progress-time">{{ formatTime(playerStore.duration) }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { usePlayerStore } from '../stores/player'
import { useDragValue } from '../composables/useDragValue'
import { formatTime } from '../utils/format'

const playerStore = usePlayerStore()
const progressBarWrapper = ref<HTMLElement | null>(null)
const isHovering = ref(false)
const dragPercent = ref(0)
const pendingSeek = ref(false) // 标记是否有待完成的 seek 操作
const hoverPercent = ref(0)

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
watch(
  () => playerStore.currentTime,
  (newTime) => {
    if (pendingSeek.value && playerStore.duration > 0) {
      const targetTime = (dragPercent.value / 100) * playerStore.duration
      // 当实际时间接近目标时间（误差 0.5 秒内），取消等待状态
      if (Math.abs(newTime - targetTime) < 0.5) {
        pendingSeek.value = false
      }
    }
  },
)

const updateDragPosition = (percent: number) => {
  dragPercent.value = Math.max(0, Math.min(100, percent * 100))
}

// 拖拽骨架 (document 级监听/清理) 由 useDragValue 提供
const { isDragging, startDrag: handlePointerDown } = useDragValue({
  getPercent: (event) => {
    if (!progressBarWrapper.value) return 0
    const rect = progressBarWrapper.value.getBoundingClientRect()
    return Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width))
  },
  onStart: (percent) => {
    if (playerStore.duration === 0) {
      return false
    }
    pendingSeek.value = false // 重置
    updateDragPosition(percent) // 按下即跳转到点击位置
    return undefined
  },
  onMove: updateDragPosition,
  onEnd: (percent) => {
    // 应用新的播放位置
    if (playerStore.duration > 0) {
      updateDragPosition(percent)
      const newTime = (dragPercent.value / 100) * playerStore.duration
      // 标记等待 seek 完成，保持显示位置
      pendingSeek.value = true
      playerStore.seek(newTime)

      // 超时保护：如果 1 秒后还没收到更新，取消等待
      setTimeout(() => {
        pendingSeek.value = false
      }, 1000)
    }
  },
})

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

/** 触摸产生的兼容 mouseenter 不会配一个 mouseleave，气泡会一直挂着，抬手时收掉 */
const handlePointerUp = (event: PointerEvent) => {
  if (event.pointerType !== 'mouse') {
    isHovering.value = false
  }
}
</script>

<style scoped>
.progress-container {
  width: 100%;
  display: flex;
  flex-direction: column;
  margin-bottom: 8px;
}

/* 时间行：桌面不占位（时间只在悬停气泡里出现），竖屏才由媒体查询打开 */
.progress-time-row {
  display: none;
}

/* 中间插槽：吃掉两端时间之间的空间。内容居中、超长省略 ——
   音频信息（格式/码率/采样率…）在窄屏上很容易过长，不能让它把时间挤走。 */
.progress-time-middle {
  flex: 1;
  min-width: 0;
  text-align: center;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.progress-bar-wrapper {
  width: 100%;
  height: 0px; /* 零高度不占布局空间，避免撑大磨砂玻璃面板 */
  display: flex;
  align-items: center;
  cursor: pointer;
  position: relative;
  /* 拖拽走 pointer events：不禁用默认手势的话，手指按下后浏览器会把这一下
     判成页面滚动并在移动时抛 pointercancel，导致安卓上"拖不动进度条" */
  touch-action: none;
}

/* 伪元素上下各延伸 10px 扩大可点击范围。 */
.progress-bar-wrapper::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  top: -10px;
  bottom: -10px;
}

/* 触摸设备（手机/平板）：22px 的有效高度对手指太薄，扩到 32px 便于点按拖动。
   高度保持 0 不变，只把热区往下探出去一点，不影响布局。 */
@media (pointer: coarse) {
  .progress-bar-wrapper::before {
    top: -14px;
    bottom: -14px;
  }
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
  transition:
    transform 0.2s ease,
    opacity 0.2s ease;
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

/* 竖屏（手机）常显时间行。这个组件的根在 App.vue 上被 .global-progress-bar 拉了 -16px 的通栏负边距（让
   进度条贴到窗口左右边缘），所以文字要补回 16px 才能与上方的曲目信息、下方的控制栏对齐。 */
@media (orientation: portrait) {
  .progress-container[data-mobile='true'] .progress-time-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    /* 18px 见模板注释：给进度条向下探 14px 的触控热区留出空白 */
    margin-top: 18px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--md-sys-color-on-surface-variant);
    /* 等宽数字：时间跳动时宽度不变，两端文字不会左右抖动 */
    font-variant-numeric: tabular-nums;
  }
}
</style>
