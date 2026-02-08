<template>
  <div class="player-controls">
    <div class="controls-row">
      <button 
        class="icon-button" 
        @click="playerStore.toggleShuffle"
        :class="{ active: playerStore.isShuffle }"
        :title="$t('controls.shuffle')"
      >
        <span class="material-symbols-rounded">shuffle</span>
      </button>
      
      <button 
        class="icon-button" 
        @click="playerStore.previousTrack"
        :disabled="!playerStore.hasPreviousTrack"
        :title="$t('controls.previous')"
      >
        <span class="material-symbols-rounded">skip_previous</span>
      </button>
      
      <button class="icon-button play-button" @click="playerStore.togglePlay" :title="playerStore.isPlaying ? $t('controls.pause') : $t('controls.play')">
        <span class="material-symbols-rounded">{{ playerStore.isPlaying ? 'pause' : 'play_arrow' }}</span>
      </button>
      
      <button 
        class="icon-button" 
        @click="playerStore.nextTrack"
        :disabled="!playerStore.hasNextTrack"
        :title="$t('controls.next')"
      >
        <span class="material-symbols-rounded">skip_next</span>
      </button>
      
      <button 
        class="icon-button" 
        @click="playerStore.toggleRepeat"
        :class="{ active: playerStore.repeatMode !== 'none' }"
        :title="getRepeatTitle()"
      >
        <span class="material-symbols-rounded">{{ getRepeatIcon() }}</span>
      </button>
      
      <!-- 悬浮式音量控制 -->
      <div class="volume-control-container" @mouseenter="showVolume = true" @mouseleave="showVolume = false">
        <button 
          class="icon-button volume-button" 
          :title="playerStore.isMuted ? $t('controls.unmute') : $t('controls.mute')"
          @click="playerStore.toggleMute"
        >
          <span class="material-symbols-rounded">{{ getVolumeIcon() }}</span>
        </button>
        
        <Transition name="volume-fade">
          <div class="volume-slider-popup" v-show="showVolume">
            <div class="slider vertical" ref="volumeSlider" :class="{ dragging: isDragging }">
              <div class="slider-track"></div>
              <div class="slider-fill" :style="{ height: `${playerStore.volume * 100}%` }"></div>
              <div class="slider-thumb" :style="{ bottom: `${playerStore.volume * 100}%` }"></div>
            </div>
            <div class="volume-value">{{ Math.round(playerStore.volume * 100) }}%</div>
          </div>
        </Transition>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()
const volumeSlider = ref<HTMLElement | null>(null)
const isDragging = ref(false)
const showVolume = ref(false)

// 保存事件处理函数引用，以便正确清理
let sliderMousedownHandler: ((event: MouseEvent) => void) | null = null

const handleVolumeChange = (event: MouseEvent) => {
  if (!volumeSlider.value) return
  
  const rect = volumeSlider.value.getBoundingClientRect()
  // 对于垂直滑块，我们需要从底部计算位置
  const percent = Math.max(0, Math.min(1, (rect.bottom - event.clientY) / rect.height))
  playerStore.setVolume(percent)
}

const startDrag = (event: MouseEvent) => {
  // 如果点击的是滑柄本身，不立即更新音量值，避免跳动
  const isThumb = (event.target as HTMLElement)?.classList?.contains('slider-thumb')
  
  isDragging.value = true
  
  if (!isThumb) {
    // 点击轨道时，立即跳转到点击位置
    handleVolumeChange(event)
  }
  
  // 添加全局事件监听器
  document.addEventListener('mousemove', handleDrag)
  document.addEventListener('mouseup', stopDrag)
  
  // 防止文本选择
  event.preventDefault()
}

const handleDrag = (event: MouseEvent) => {
  if (isDragging.value) {
    handleVolumeChange(event)
  }
}

const stopDrag = () => {
  isDragging.value = false
  
  // 移除全局事件监听器
  document.removeEventListener('mousemove', handleDrag)
  document.removeEventListener('mouseup', stopDrag)
}

onMounted(() => {
  if (volumeSlider.value) {
    sliderMousedownHandler = startDrag
    volumeSlider.value.addEventListener('mousedown', sliderMousedownHandler)
  }
})

// 循环模式相关函数
const getRepeatIcon = () => {
  if (playerStore.repeatMode === 'track') {
    return 'repeat_one'
  } else if (playerStore.repeatMode === 'list') {
    return 'repeat'
  }
  return 'repeat'
}

const getRepeatTitle = () => {
  if (playerStore.repeatMode === 'track') {
    return '单曲循环'
  } else if (playerStore.repeatMode === 'list') {
    return '列表循环'
  }
  return '循环播放'
}

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

onUnmounted(() => {
  // 清理全局事件监听器
  document.removeEventListener('mousemove', handleDrag)
  document.removeEventListener('mouseup', stopDrag)
  
  // 清理音量滑块的事件监听器
  if (volumeSlider.value && sliderMousedownHandler) {
    volumeSlider.value.removeEventListener('mousedown', sliderMousedownHandler)
  }
})
</script>

<style scoped>
.player-controls {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}

.controls-row {
  display: flex;
  align-items: center;
  gap: 8px;
  position: relative;
}

.play-button {
  width: 56px;
  height: 56px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border-radius: 50%;
  transition: all 0.2s ease;
}

.play-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-secondary-container));
}

.play-button .material-symbols-rounded {
  font-size: 32px;
}

/* 悬浮式音量控制 */
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
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.volume-fade-enter-from,
.volume-fade-leave-to {
  opacity: 0;
  transform: translateY(10px);
}
</style>