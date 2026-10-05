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
        <!-- 时间提示定位用 displayPercent（滑柄位置），不用 hoverPercent -->
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

    <!-- 已播 / 总时长（秒）。桌面靠悬停气泡显示时间，手指没有 hover，手机竖屏常显这一行。
         位置留到进度条下方 18px：热区 (::before) 向下探了 14px，贴着放会点字误触成拖动。 -->
    <div class="progress-time-row">
      <span class="progress-time">{{ formatTime(displayTime) }}</span>
      <!-- 中间插槽：无内容时仍占 flex:1，两端时间顶到边缘，布局不因有无内容而跳动 -->
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
const pendingSeek = ref(false) // seek 已发出、等 currentTime 追上目标
const hoverPercent = ref(0)

const progressPercent = computed(() => {
  if (playerStore.duration === 0) return 0
  return (playerStore.currentTime / playerStore.duration) * 100
})

const displayPercent = computed(() => {
  // 拖动中或 pendingSeek 期间显示拖动位置：等回报会把滑柄闪回旧位置
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

// currentTime 追上目标后即撤掉 pendingSeek（见上方定义）
watch(
  () => playerStore.currentTime,
  (newTime) => {
    if (pendingSeek.value && playerStore.duration > 0) {
      const targetTime = (dragPercent.value / 100) * playerStore.duration
      // 容差 0.5 秒：seek 的实际落点不会正好命中目标时刻
      if (Math.abs(newTime - targetTime) < 0.5) {
        pendingSeek.value = false
      }
    }
  },
)

const updateDragPosition = (percent: number) => {
  dragPercent.value = Math.max(0, Math.min(100, percent * 100))
}

// 拖拽骨架（document 级监听与清理）由 useDragValue 提供
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
    pendingSeek.value = false // 丢弃上一次未落地的等待
    updateDragPosition(percent) // 按下即定位到点击处，不等 move
    return undefined
  },
  onMove: updateDragPosition,
  onEnd: (percent) => {
    if (playerStore.duration > 0) {
      updateDragPosition(percent)
      const newTime = (dragPercent.value / 100) * playerStore.duration
      // 松手即进入等待，保持显示位置（见 pendingSeek）
      pendingSeek.value = true
      playerStore.seek(newTime)

      // 兜底超时 1 秒：currentTime 没回报就退出等待，否则滑柄卡死
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

/* 时间行：桌面不占位（时间只在悬停气泡里），只有手机竖屏由下面的 [data-mobile] 规则打开 */
.progress-time-row {
  display: none;
}

/* 中间插槽吃掉两端时间之间的空间；居中 + 超长省略，窄屏上音频信息不能把时间挤走 */
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
  /* 拖拽走 pointer events：不禁默认手势时，安卓 WebView 会把按下判成页面滚动并在移动时抛
     pointercancel，结果是拖不动进度条 */
  touch-action: none;
}

/* 元素零高度，热区靠伪元素上下各 10px 撑出 */
.progress-bar-wrapper::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  top: -10px;
  bottom: -10px;
}

/* 触摸设备：上下各 10px 的热区对手指太薄，扩到各 14px（共 28px）；高度仍为 0，只下探不动布局 */
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
  transition: height 0.2s ease;
}

/* 悬停时把进度条加粗。用 height 而不是 transform: scaleY(2)
.progress-bar-wrapper.is-dragging .progress-bar {
  height: 4px;
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
  box-shadow: 0 0 3px rgba(0, 0, 0, 0.28);
  pointer-events: none;
  opacity: 0;
}

/* 显示手柄 */
.progress-bar-wrapper.is-hovering .progress-bar-handle,
.progress-bar-wrapper.is-dragging .progress-bar-handle {
  opacity: 1;
  transform: translate(-50%, -50%) scale(1);
}

.progress-bar-wrapper.is-dragging .progress-bar-handle {
  transform: translate(-50%, -50%) scale(1.2);
  box-shadow: 0 0 6px rgba(0, 0, 0, 0.36);
}

.progress-bar-wrapper.is-dragging {
  cursor: grabbing;
}

/* 拖动时关掉 width 过渡，填充才能实时跟手 */
.progress-bar-wrapper.is-dragging .progress-bar-fill {
  transition: none;
}

.hover-time-tooltip {
  position: absolute;
  top: -28px;
  /* clamp 边界用的半宽：内容约 80px、半宽约 40px，取 45px 留余量 */
  --tooltip-half-width: 45px;
  transform: translateX(-50%);
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
    transform: translateX(-50%) translateY(4px);
  }
  to {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
}

/* 竖屏（手机）常显时间行。根节点在 App.vue 被 .global-progress-bar 拉了 -16px 通栏负边距
   （让进度条贴到窗口边缘），所以文字要补回 16px 才与曲目信息、控制栏对齐。 */
@media (orientation: portrait) {
  .progress-container[data-mobile='true'] .progress-time-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    /* 18px 的来由见模板里的时间行注释 */
    margin-top: 18px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--md-sys-color-on-surface-variant);
    /* 等宽数字：时间跳动时宽度不变，两端文字不会左右抖动 */
    font-variant-numeric: tabular-nums;
  }
}
</style>
