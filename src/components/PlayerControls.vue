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
          :title="$t('controls.volume')"
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

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()
const volumeSlider = ref(null)
const isDragging = ref(false)
const showVolume = ref(false)

// 保存事件处理函数引用，以便正确清理
let volumeSliderClickHandler = null
let thumbMousedownHandler = null
let sliderMousedownHandler = null

const handleVolumeChange = (event) => {
  if (!volumeSlider.value) return
  
  const rect = volumeSlider.value.getBoundingClientRect()
  // 对于垂直滑块，我们需要从底部计算位置
  const percent = Math.max(0, Math.min(1, (rect.bottom - event.clientY) / rect.height))
  playerStore.setVolume(percent)
}

const startDrag = (event) => {
  isDragging.value = true
  handleVolumeChange(event)
  
  // 添加全局事件监听器
  document.addEventListener('mousemove', handleDrag)
  document.addEventListener('mouseup', stopDrag)
  
  // 防止文本选择
  event.preventDefault()
}

const handleDrag = (event) => {
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
    // 保存处理函数引用
    volumeSliderClickHandler = handleVolumeChange
    sliderMousedownHandler = startDrag
    
    volumeSlider.value.addEventListener('click', volumeSliderClickHandler)
    
    // 为滑块添加拖拽事件
    const thumb = volumeSlider.value.querySelector('.slider-thumb')
    if (thumb) {
      thumbMousedownHandler = startDrag
      thumb.addEventListener('mousedown', thumbMousedownHandler)
    }
    
    // 为整个滑块区域也添加拖拽事件
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
  if (volumeSlider.value) {
    if (volumeSliderClickHandler) {
      volumeSlider.value.removeEventListener('click', volumeSliderClickHandler)
    }
    if (sliderMousedownHandler) {
      volumeSlider.value.removeEventListener('mousedown', sliderMousedownHandler)
    }
    
    const thumb = volumeSlider.value.querySelector('.slider-thumb')
    if (thumb && thumbMousedownHandler) {
      thumb.removeEventListener('mousedown', thumbMousedownHandler)
    }
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
  background-color: var(--md-sys-color-primary);
  color: var(--theme-on-primary);
  border-radius: 50%;
  /* 修复阴影造成的视觉拉伸 */
  box-shadow: 0px 1px 2px -1px rgba(0, 0, 0, 0.2), 0px 1px 2px 0px rgba(0, 0, 0, 0.14), 0px 1px 4px 0px rgba(0, 0, 0, 0.12);
}

.play-button:hover {
  /* 修复阴影造成的视觉拉伸 */
  box-shadow: 0px 2px 4px -1px rgba(0, 0, 0, 0.2), 0px 2px 4px 0px rgba(0, 0, 0, 0.14), 0px 1px 6px 0px rgba(0, 0, 0, 0.12);
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
  right: 0px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: var(--md-sys-shape-corner-medium);
  padding: 8px;
  box-shadow: var(--md-sys-elevation-level3);
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 24px;
  height: 110px;
  transform-origin: bottom right;
  z-index: 10;
}

.volume-slider-popup .slider {
  width: 8px;
  height: 90px;
  position: relative;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: 3px;
  cursor: pointer;
  transition: background-color 0.2s ease;
  padding-top: 5px;
  margin: 0 auto;
}

.volume-slider-popup .slider:hover {
  background-color: var(--md-sys-color-on-surface-variant);
}

.volume-slider-popup .slider.dragging {
  background-color: var(--md-sys-color-primary);
}

.volume-slider-popup .slider-track {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  border-radius: 3px;
}

.volume-slider-popup .slider-fill {
  position: absolute;
  bottom: 0;
  left: 0;
  width: 100%;
  background-color: var(--md-sys-color-primary);
  border-radius: 3px;
  /* 移除过渡动画，让音量立即变化 */
}

.volume-slider-popup .slider-thumb {
  position: absolute;
  left: 50%;
  bottom: 0;
  transform: translateX(-50%) translateY(50%);
  width: 14px;
  height: 14px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  cursor: grab;
  transition: transform 0.1s ease;
}

.volume-slider-popup .slider-thumb:hover {
  transform: translateX(-50%) translateY(50%) scale(1.1);
}

.volume-slider-popup .slider.dragging .slider-thumb {
  cursor: grabbing;
  transform: translateX(-50%) translateY(50%) scale(1.2);
  background-color: var(--md-sys-color-on-primary);
}

.volume-value {
  font-size: 10px;
  color: var(--md-sys-color-on-surface);
  font-weight: 500;
  margin-top: 2px;
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
  transform: translateY(10px) scale(0.95);
}
</style>