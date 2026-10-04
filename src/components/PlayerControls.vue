<template>
  <div class="player-controls">
    <div class="controls-row">
      <!-- 四态压成单键循环（顺序 -> 列表循环 -> 单曲循环 -> 随机），省出的宽度让 play 键居中；
           图标本身就是当前模式，再加 active 高亮是冗余 -->
      <button class="icon-button" :title="playModeTitle" @click="playerStore.cyclePlayMode">
        <span
          class="material-symbols-rounded play-mode-icon"
          :class="{ 'is-order': !isPlayModeActive }"
          >{{ playModeIcon }}</span
        >
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

      <!-- 附加按钮由宿主插到传输键之后：桌面与横屏此行有富余宽度，竖屏接近满宽时宿主会换到右组 -->
      <slot name="after-next" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { usePlayerStore } from '../stores/player'

const { t } = useI18n()
const playerStore = usePlayerStore()

/** 非"顺序播放"的三种模式都算激活（顺序播放要给它画上斜线） */
const isPlayModeActive = computed<boolean>(
  () => playerStore.isShuffle || playerStore.repeatMode !== 'none',
)

const playModeIcon = computed<string>(() => {
  if (playerStore.isShuffle) return 'shuffle'
  if (playerStore.repeatMode === 'track') return 'repeat_one'
  // 列表循环与顺序播放共用 repeat 图标；斜线的做法与约束见样式里的 .is-order 注释
  return 'repeat'
})

const playModeTitle = computed<string>(() => {
  if (playerStore.isShuffle) return t('controls.shuffle')
  if (playerStore.repeatMode === 'track') return t('controls.repeatOne')
  if (playerStore.repeatMode === 'list') return t('controls.repeatList')
  return t('controls.repeatOff')
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

@media (hover: hover) {
  .play-button:hover {
    background-color: color-mix(
      in srgb,
      var(--md-sys-color-on-surface) 8%,
      var(--md-sys-color-secondary-container)
    );
  }
}

.play-button .material-symbols-rounded {
  font-size: 32px;
}

/* 竖屏只改间距：按钮尺寸不在这里定义，同一行还有播放列表 / 音量等非本组件按钮，
   分散两处会不一致；统一尺寸见 App.css 的 orientation: portrait 规则。 */
@media (orientation: portrait) {
  .controls-row {
    gap: 14px;
  }

  .play-button .material-symbols-rounded {
    font-size: 36px;
  }
}

/* 顺序态 = repeat 加一道斜线（Material Symbols 无 repeat_off）。斜线要长过字形盒（1.25em > 1em）、
   取 -45deg 那条对角线：短了两端埋进字形、与 repeat 箭头连成 Z 形，另一条会横穿回环笔画。
   别用背景色描边留白：--immersive-bg 不在按钮继承链上，写死 surface 色会露错色。 */
.play-mode-icon {
  position: relative;
}

.play-mode-icon.is-order::after {
  content: '';
  position: absolute;
  inset: 0;
  margin: auto;
  width: 1.25em;
  height: 0.11em;
  border-radius: 0.06em;
  background-color: currentColor;
  transform: rotate(-45deg);
}
</style>
