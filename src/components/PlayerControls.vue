<template>
  <div class="player-controls">
    <div class="controls-row">
      <!-- 播放模式：把原来的「随机播放」「循环播放」两颗按钮压成一颗，按一次走一格 ——
           顺序播放 → 列表循环 → 单曲循环 → 随机播放 → 顺序播放……
           底部那一行因此从 6 颗减到 5 颗（本组件 4 颗 + .side-controls 的播放列表），
           手机上不再拥挤，play 按钮也刚好落在正中间。
           图标只表明"当前是什么模式"，**不加 active 高亮**：四种模式各有各的样子，
           高亮反而多一层冗余。顺序播放 **没有专用图标**，就是 repeat 加一道斜线
           （Material Symbols 里没有 repeat_off，斜线用 ::after 画）。 -->
      <button
        class="icon-button"
        :title="playModeTitle"
        @click="playerStore.cyclePlayMode"
      >
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
  () => playerStore.isShuffle || playerStore.repeatMode !== 'none'
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

/* 竖屏（手机）：行内间距拉开，避免误触。按钮**尺寸**不在这里定义 ——
   底部那一行里还有播放列表 / 音量等不属于本组件的按钮，尺寸分散在两处
   就会不一致（播放列表曾因此一直是 40px，与这里的 48px 对不齐）。
   统一尺寸见 App.css 的 `@media (orientation: portrait) .controls-area` 规则。 */
@media (orientation: portrait) {
  .controls-row {
    gap: 14px;
  }

  .play-button .material-symbols-rounded {
    font-size: 36px;
  }
}

/* ===== 「顺序播放」= repeat 图标 + 一道斜线 =====
   Material Symbols 没有 repeat_off，斜线只能自己画。
   要点：
   - **长度必须超过字形盒（1.25em > 1em）**：短斜线（最初写的 0.92em）两端都埋在
     字形里，而 repeat 自身的两个箭头正好也落在 45° 对角线上 —— 两者会在视觉上连成
     一个"Z"，完全读不出"划掉"的意思。两端各出头约 0.125em 之后才一眼可辨。
   - 斜线方向不能用 +45°（另一条对角线）：那条正好横穿 repeat 的回环笔画，糊成一团。
   - 尺寸用 em，跟着图标字号走 —— 竖屏把图标放大到 36px 时斜线自动同比例变长；
     颜色用 currentColor，深/浅主题、hover、disabled 态都不用另外处理。
   - **不加"背景色描边"来给斜线留白**：那个描边得是按钮背后的实际颜色，而沉浸式模式下
     背景是 `--immersive-bg`（只在 `.immersive-layer` 上定义、不在按钮的继承链上），
     写死 surface 色会在沉浸式下露出错误颜色的一条线。宁可朴素一点也不出错。
   - `inset: 0; margin: auto` 而不是手算位移：字形盒就是 line-height（=1）撑出的正方形，
     这样斜线正好压在字形中心；::after 画在文字之后，天然盖在图标上面，不需要 z-index。 */
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
