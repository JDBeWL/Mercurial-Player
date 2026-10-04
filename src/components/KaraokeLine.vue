<template>
  <span
    v-for="(word, idx) in words"
    :key="idx"
    class="karaoke-text"
    :class="{ active: isWordActive(word) }"
    :style="getKaraokeStyle(word)"
    >{{ word.text }}</span
  >
</template>

<script lang="ts">
import { defineComponent, inject, type PropType, type CSSProperties, type Ref } from 'vue'
import type { KaraokeWord } from '@/types'
import { usePlayerStore } from '@/stores/player'

// 卡拉 OK 逐字进度子组件：把对 visualTime（每帧更新）的渲染依赖隔离在这里，
// 父组件 LyricsDisplay 不再每帧重渲染；visualTime 由 LyricsDisplay 以 'lyricsVisualTime' provide ref 本身
export default defineComponent({
  name: 'KaraokeLine',
  props: {
    words: { type: Array as PropType<KaraokeWord[]>, required: true },
  },
  setup() {
    const playerStore = usePlayerStore()
    // 后备值：未注入时退化为 store 时间（暂停态足够，且不引入父级依赖）
    const visualTime = inject<Ref<number> | null>('lyricsVisualTime', null)

    const currentTime = (): number => {
      const t = visualTime ? visualTime.value : playerStore.currentTime
      return t - (playerStore.lyricsOffset || 0)
    }

    const isWordActive = (word: KaraokeWord): boolean => {
      const t = currentTime()
      return t >= word.start && t < word.end
    }

    const getKaraokeStyle = (word: KaraokeWord): CSSProperties => {
      const t = currentTime()
      if (t >= word.end) return { '--progress': '100%' }
      if (t < word.start) return { '--progress': '0%' }

      // 夹在 0-100，时间戳重叠时 --progress 不会越界
      const duration = word.end - word.start
      const elapsed = t - word.start
      const progress = Math.min(100, Math.max(0, (elapsed / duration) * 100))
      return { '--progress': `${progress.toFixed(2)}%` }
    }

    return { isWordActive, getKaraokeStyle }
  },
})
</script>
