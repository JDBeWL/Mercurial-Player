<template>
  <div class="player-controls">
    <div class="controls-row">
      <!-- 播放模式：把原来的「随机播放」「循环播放」两颗按钮压成一颗，按一次走一格（顺序、列表循环、
           单曲循环、随机，再回到顺序），底部那一行因此从 6 颗减到 5 颗，手机上不再拥挤、play 按钮也刚好
           落在正中间。图标只表明"当前是什么模式"，不加 active 高亮：四种模式各有各的样子，高亮是冗余。 -->
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

      <!-- 附加按钮（播放列表）由宿主插到这里：桌面端与手机横屏这一行有富余宽度，让它跟传输键成组；
           竖屏已经接近满宽，宿主会把它留在右侧那一组 -->
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
  // 列表循环与顺序播放共用 repeat，区别只在顺序态多一道斜线（见 .is-order）
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

/* 竖屏（手机）：行内间距拉开，避免误触。按钮尺寸不在这里定义，底部那一行里还有播放列表 / 音量等不属于
   本组件的按钮，尺寸分散在两处就会不一致。统一尺寸见 App.css 的 `@media (orientation: portrait)` 规则。 */
@media (orientation: portrait) {
  .controls-row {
    gap: 14px;
  }

  .play-button .material-symbols-rounded {
    font-size: 36px;
  }
}

/* 「顺序播放」= repeat 图标加一道斜线，Material Symbols 没有 repeat_off。斜线必须长过字形盒 (1.25em 大于
   1em)、且取 45° 那条对角线：短了两端埋在字形里、与 repeat 自身的箭头连成"Z"；另一条对角线横穿回环笔画。
   不要给斜线加"背景色描边"留白：沉浸模式的 --immersive-bg 不在按钮的继承链上，写死 surface 色会露错色。 */
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
