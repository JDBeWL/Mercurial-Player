<template>
  <div
    class="volume-control-container"
    @mouseenter="showVolume = true"
    @mouseleave="showVolume = false"
  >
    <button
      class="icon-button volume-button"
      :title="playerStore.isMuted ? $t('controls.unmute') : $t('controls.mute')"
      @click="playerStore.toggleMute"
    >
      <span class="material-symbols-rounded">{{ getVolumeIcon() }}</span>
    </button>

    <Transition name="volume-fade">
      <div v-show="showVolume" class="volume-slider-popup">
        <div
          ref="volumeSlider"
          class="slider vertical"
          :class="{ dragging: isDragging }"
          @mousedown="startDrag"
        >
          <div class="slider-track"></div>
          <div class="slider-fill" :style="{ height: `${playerStore.volume * 100}%` }"></div>
          <div class="slider-thumb" :style="{ bottom: `${playerStore.volume * 100}%` }"></div>
        </div>
        <div class="volume-value">{{ Math.round(playerStore.volume * 100) }}%</div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { usePlayerStore } from '../stores/player'
import { useDragValue } from '../composables/useDragValue'

const playerStore = usePlayerStore()
const volumeSlider = ref<HTMLElement | null>(null)
const showVolume = ref(false)

// 垂直滑块: 从底部计算百分比;拖拽骨架 (document 级监听/清理) 由 useDragValue 提供
const { isDragging, startDrag } = useDragValue({
  getPercent: (event) => {
    if (!volumeSlider.value) return 0
    const rect = volumeSlider.value.getBoundingClientRect()
    return Math.max(0, Math.min(1, (rect.bottom - event.clientY) / rect.height))
  },
  onStart: (percent, event) => {
    // 如果点击的是滑柄本身，不立即更新音量值，避免跳动
    const isThumb = (event.target as HTMLElement)?.classList?.contains('slider-thumb')
    if (!isThumb) {
      playerStore.setVolume(percent)
    }
    // 防止文本选择
    event.preventDefault()
  },
  onMove: (percent) => {
    playerStore.setVolume(percent)
  },
})

// 音量图标相关函数
const getVolumeIcon = () => {
  // 如果静音，显示静音图标
  if (playerStore.isMuted) {
    return 'volume_off'
  }

  const volume = playerStore.volume
  if (volume === 0 || volume < 0.01) {
    return 'volume_off'
  } else if (volume < 0.5) {
    return 'volume_down'
  } else {
    return 'volume_up'
  }
}
</script>

<style scoped>
.volume-control-container {
  position: relative;
  display: inline-block;
}

.volume-button {
  cursor: pointer;
}

.volume-slider-popup {
  position: absolute;
  bottom: calc(100% + 8px);
  right: 2px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: var(--md-sys-shape-corner-medium);
  padding: 8px;
  box-shadow: var(--md-sys-elevation-level3);
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 20px;
  height: 120px;
  z-index: 10;
}

.volume-slider-popup .slider {
  width: 8px;
  height: 100px;
  position: relative;
  border-radius: 4px;
  cursor: pointer;
  margin: 8px auto;
}

.volume-slider-popup .slider-track {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.1);
  border-radius: 4px;
}

.volume-slider-popup .slider:hover .slider-track {
  background-color: rgba(0, 0, 0, 0.15);
}

.volume-slider-popup .slider.dragging .slider-track {
  background-color: rgba(0, 0, 0, 0.2);
}

.volume-slider-popup .slider-fill {
  position: absolute;
  bottom: 0;
  left: 0;
  width: 100%;
  background-color: var(--md-sys-color-primary);
  border-radius: 4px;
}

.volume-slider-popup .slider-thumb {
  position: absolute;
  left: 50%;
  bottom: 0;
  transform: translateX(-50%);
  width: 14px;
  height: 14px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  cursor: grab;
  margin-bottom: -7px;
}

.volume-slider-popup .slider-thumb:hover {
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.4);
}

.volume-slider-popup .slider.dragging .slider-thumb {
  cursor: grabbing;
  background-color: var(--md-sys-color-on-primary);
}

.volume-value {
  font-size: 10px;
  color: var(--md-sys-color-on-surface);
  font-weight: 500;
  text-align: center;
}

/* 音量弹出动画 */
.volume-fade-enter-active,
.volume-fade-leave-active {
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}

.volume-fade-enter-from,
.volume-fade-leave-to {
  opacity: 0;
  transform: translateY(10px);
}
</style>
