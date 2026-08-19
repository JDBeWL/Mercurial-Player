<template>
  <div class="player-controls">
    <div class="controls-row">
      <button
        class="icon-button"
        :class="{ active: playerStore.isShuffle }"
        :title="$t('controls.shuffle')"
        @click="playerStore.toggleShuffle"
      >
        <span class="material-symbols-rounded">shuffle</span>
      </button>

      <button
        class="icon-button"
        :disabled="!playerStore.hasPreviousTrack"
        :title="$t('controls.previous')"
        @click="playerStore.previousTrack"
      >
        <span class="material-symbols-rounded">skip_previous</span>
      </button>

      <button
        class="icon-button play-button"
        :title="playerStore.isPlaying ? $t('controls.pause') : $t('controls.play')"
        @click="playerStore.togglePlay"
      >
        <span class="material-symbols-rounded">{{
          playerStore.isPlaying ? 'pause' : 'play_arrow'
        }}</span>
      </button>

      <button
        class="icon-button"
        :disabled="!playerStore.hasNextTrack"
        :title="$t('controls.next')"
        @click="playerStore.nextTrack"
      >
        <span class="material-symbols-rounded">skip_next</span>
      </button>

      <button
        class="icon-button"
        :class="{ active: playerStore.repeatMode !== 'none' }"
        :title="getRepeatTitle()"
        @click="playerStore.toggleRepeat"
      >
        <span class="material-symbols-rounded">{{ getRepeatIcon() }}</span>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { usePlayerStore } from '../stores/player'

const playerStore = usePlayerStore()

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
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
}

.play-button .material-symbols-rounded {
  font-size: 32px;
}
</style>
