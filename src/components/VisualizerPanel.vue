<template>
  <div class="visualizer-panel">
    <div ref="visualizerContainer" class="visualizer-container">
      <canvas ref="canvasRef"></canvas>
    </div>

    <div class="single-line-lyrics">
      <div v-if="currentLyric" class="lyric-content">
        <div
          :class="[
            'lyric-original',
            { 'has-translation': !!currentLyric.texts[1] },
            isLyricTypeASS ? 'lyric-original-ass' : 'lyric-original-lrc',
          ]"
          :style="lyricFontStyle"
          :lang="originalLang || undefined"
        >
          <template v-if="currentLyric.words?.length">
            <!-- 逐字高亮的渲染沿用主歌词页，样式由 getKaraokeStyle 给 -->
            <span
              v-for="(word, idx) in currentLyric.words"
              :key="idx"
              class="karaoke-word"
              :style="getKaraokeStyle(word)"
            >
              {{ word.text }}
            </span>
          </template>
          <template v-else>
            {{ currentLyric.texts[0] }}
          </template>
        </div>
        <div
          v-if="currentLyric.texts[1]"
          class="lyric-translation"
          :style="translationStyle"
          :lang="translationLang || undefined"
        >
          {{ currentLyric.texts[1] }}
        </div>
      </div>
      <div v-else class="lyric-placeholder"></div>
    </div>
  </div>
</template>

<script lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue'
import { usePlayerStore } from '@/stores/player'
import { useLyrics } from '@/composables/useLyrics'
import { useVisualTime } from '@/composables/useVisualTime'
import { useLyricsTypography } from '@/composables/useLyricsTypography'
import { listen } from '@tauri-apps/api/event'
import { setVisualizerVisible } from '@/services/appService'
import logger from '@/utils/logger'
import { detectLyricLanguage } from '@/utils/languageDetect'
import { supportsColorMix } from '@/utils/cssSupport'
import type { KaraokeWord } from '@/types'

export default {
  name: 'VisualizerPanel',
  setup() {
    const playerStore = usePlayerStore()
    const { lyrics, activeIndex } = useLyrics()

    const canvasRef = ref<HTMLCanvasElement | null>(null)
    const visualizerContainer = ref<HTMLElement | null>(null)
    let animationId: number | null = null
    // bin 数必须与 Rust 侧 audio::spectrum::SPECTRUM_BINS 一致
    const SPECTRUM_SIZE = 128
    let audioData = new Float32Array(SPECTRUM_SIZE)
    let smoothedAudioData = new Float32Array(SPECTRUM_SIZE)
    let spectrumListener: (() => void) | null = null
    let isAnimating = false
    // onMounted 内有 await：注册完成时组件可能已卸载，需要事后自查
    let disposed = false

    let pendingSpectrumData: Float32Array | null = null

    let cachedGradient: CanvasGradient | null = null
    let lastCanvasHeight = 0
    let lastPrimaryColor: string | null = null

    const getPrimaryColor = () => {
      return (
        getComputedStyle(document.documentElement)
          .getPropertyValue('--md-sys-color-primary')
          .trim() || '#6750a4'
      )
    }

    const getOrCreateGradient = (ctx: CanvasRenderingContext2D, height: number) => {
      const primaryColor = getPrimaryColor()
      if (!cachedGradient || lastCanvasHeight !== height || lastPrimaryColor !== primaryColor) {
        cachedGradient = ctx.createLinearGradient(0, height, 0, 0)
        cachedGradient.addColorStop(0, primaryColor)
        cachedGradient.addColorStop(1, `${primaryColor}40`)
        lastCanvasHeight = height
        lastPrimaryColor = primaryColor
      }
      return cachedGradient
    }

    // 每帧 128 次 beginPath/fill 合成一次 draw call，显著降低 CPU 绘制开销
    const renderBars = (
      ctx: CanvasRenderingContext2D,
      width: number,
      height: number,
      drawData: Float32Array,
    ) => {
      const bufferLength = SPECTRUM_SIZE
      const barWidth = (width / bufferLength) * 0.8
      const gap = (width / bufferLength) * 0.2
      let x = 0

      ctx.fillStyle = getOrCreateGradient(ctx, height)
      ctx.shadowBlur = 0

      ctx.beginPath()
      const useRoundRect = !!ctx.roundRect
      for (let i = 0; i < bufferLength; i++) {
        const value = drawData[i]!
        let barHeight = Math.pow(value, 0.9) * height * 0.9

        if (barHeight > height) barHeight = height
        if (barHeight < 2) barHeight = 2

        if (useRoundRect) {
          ctx.roundRect(x, height - barHeight, barWidth, barHeight, [5, 5, 0, 0])
        } else {
          ctx.rect(x, height - barHeight, barWidth, barHeight)
        }

        x += barWidth + gap
      }
      ctx.fill()
      ctx.shadowBlur = 0
    }

    const smoothDataInPlace = (
      currentData: Float32Array,
      targetData: Float32Array,
      smoothingFactor = 0.85,
    ): void => {
      for (let i = 0; i < SPECTRUM_SIZE; i++) {
        const current = currentData[i] || 0
        const target = targetData[i] || 0
        currentData[i] = current * smoothingFactor + target * (1 - smoothingFactor)
      }
    }

    const currentLyric = computed(() => {
      if (activeIndex.value !== -1 && lyrics.value[activeIndex.value]) {
        return lyrics.value[activeIndex.value]
      }
      return null
    })

    // 有逐字时间戳就按 ASS 渲染，否则按 LRC
    const isLyricTypeASS = computed(() => {
      return currentLyric.value && currentLyric.value.words && currentLyric.value.words.length > 0
    })

    // 歌词字体与主歌词页共用同一配置，见 useLyricsTypography
    const { lyricFontStyle, translationStyle } = useLyricsTypography()

    // 当前行的语言标注（原文/译文分别检测），供 lang 属性与字体 locl 区域字形使用
    const originalLang = computed(() => detectLyricLanguage(currentLyric.value?.texts[0]))
    const translationLang = computed(() => detectLyricLanguage(currentLyric.value?.texts[1]))

    // 视觉时间：卡拉OK 用的帧钟，见 useVisualTime
    const { visualTime, advanceVisualTime, resetFrameClock, syncToCurrentTime } = useVisualTime()

    watch(
      () => playerStore.currentTrack?.path,
      () => {
        syncToCurrentTime()
        karaokeStyleCache.clear()
      },
    )

    const karaokeStyleCache = new Map<string, Record<string, string>>()
    const activeColor = 'var(--md-sys-color-primary)'
    // 未唱色：不支持 color-mix() 时退回 outline，原因见样式里 .karaoke-word 的注释
    const inactiveColor = supportsColorMix()
      ? 'color-mix(in srgb, var(--md-sys-color-primary) 40%, rgba(255, 255, 255, 0.1))'
      : 'var(--md-sys-color-outline)'

    const getKaraokeStyle = (word: KaraokeWord) => {
      const offset = playerStore.lyricsOffset || 0
      const t = visualTime.value - offset

      let progress
      if (t >= word.end) {
        progress = 100
      } else if (t < word.start) {
        progress = 0
      } else {
        progress = ((t - word.start) / (word.end - word.start)) * 100
      }

      const roundedProgress = Math.round(progress)
      const cacheKey = `${word.start}-${word.end}-${roundedProgress}`

      let cached = karaokeStyleCache.get(cacheKey)
      if (cached) return cached

      const p = roundedProgress
      cached = {
        '--progress': `${p}%`,
        backgroundImage: `linear-gradient(90deg, ${activeColor} ${p}%, ${inactiveColor} ${p}%)`,
      }

      if (karaokeStyleCache.size > 500) {
        const firstKey = karaokeStyleCache.keys().next().value
        if (firstKey !== undefined) {
          karaokeStyleCache.delete(firstKey)
        }
      }
      karaokeStyleCache.set(cacheKey, cached)
      return cached
    }

    // 暂停时保留最后一帧，不再推进 rAF
    const drawFrozenFrame = () => {
      if (!canvasRef.value || !visualizerContainer.value) return

      const canvas = canvasRef.value
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      const width = canvas.width
      const height = canvas.height

      ctx.clearRect(0, 0, width, height)

      const drawData = smoothedAudioData
      if (!drawData || drawData.length === 0) return

      renderBars(ctx, width, height, drawData)
    }

    const drawVisualizer = (timestamp: number): void => {
      if (!canvasRef.value || !visualizerContainer.value) return

      if (!playerStore.isPlaying) {
        isAnimating = false
        animationId = null
        return
      }

      if (pendingSpectrumData) {
        const data = pendingSpectrumData
        for (let i = 0; i < SPECTRUM_SIZE && i < data.length; i++) {
          audioData[i] = data[i]!
        }
        pendingSpectrumData = null
      }

      const canvas = canvasRef.value
      const ctx = canvas.getContext('2d')
      if (!ctx) return
      const width = canvas.width
      const height = canvas.height

      ctx.clearRect(0, 0, width, height)

      smoothDataInPlace(smoothedAudioData, audioData, 0.85)

      const drawData = smoothedAudioData

      if (drawData.length === 0) {
        ctx.beginPath()
        ctx.moveTo(0, height - 2)
        ctx.lineTo(width, height - 2)
        ctx.strokeStyle = getPrimaryColor()
        ctx.stroke()

        animationId = requestAnimationFrame(drawVisualizer)
        return
      }

      renderBars(ctx, width, height, drawData)

      ctx.shadowBlur = 0

      // 推进视觉时间；P 控制器在 useVisualTime 内
      advanceVisualTime(timestamp)

      animationId = requestAnimationFrame(drawVisualizer)
    }

    const startAnimation = () => {
      if (isAnimating) return
      isAnimating = true
      resetFrameClock()
      animationId = requestAnimationFrame(drawVisualizer)
    }

    const stopAnimation = () => {
      if (animationId) {
        cancelAnimationFrame(animationId)
        animationId = null
      }
      isAnimating = false
      pendingSpectrumData = null
    }

    watch(
      () => playerStore.isPlaying,
      (playing) => {
        if (playing) {
          startAnimation()
        } else {
          stopAnimation()
        }
      },
    )

    const resizeCanvas = () => {
      if (canvasRef.value && visualizerContainer.value) {
        canvasRef.value.width = visualizerContainer.value.clientWidth
        canvasRef.value.height = visualizerContainer.value.clientHeight
        if (!playerStore.isPlaying) {
          drawFrozenFrame()
        }
      }
    }

    onMounted(async () => {
      // 先申请频谱、后注册监听：反序会让首帧为空。
      // 卸载时 onUnmounted 的 false 一定晚于本条送达，最终仍是关闭。
      setVisualizerVisible(true).catch((error) => {
        logger.warn('Failed to enable spectrum data:', error)
      })
      window.addEventListener('resize', resizeCanvas)
      resizeCanvas()

      // 后端以 60fps 发 spectrum-update：数据先存一份，由 rAF 按屏幕刷新率消费，不逐事件重绘
      try {
        const unlisten = await listen<{ data: Float32Array }>('spectrum-update', (event) => {
          if (event.payload && event.payload.data) {
            pendingSpectrumData = event.payload.data
          }
        })
        if (disposed) unlisten()
        else spectrumListener = unlisten
      } catch (error) {
        logger.error('Failed to setup spectrum listener:', error)
      }

      if (disposed) return

      if (playerStore.isPlaying) {
        startAnimation()
      } else {
        drawFrozenFrame()
      }
    })

    onUnmounted(() => {
      disposed = true
      setVisualizerVisible(false).catch((error) => {
        logger.warn('Failed to disable spectrum data:', error)
      })
      window.removeEventListener('resize', resizeCanvas)
      stopAnimation()
      spectrumListener?.()
      spectrumListener = null
    })

    return {
      canvasRef,
      visualizerContainer,
      currentLyric,
      isLyricTypeASS,
      getKaraokeStyle,
      lyricFontStyle,
      translationStyle,
      originalLang,
      translationLang,
    }
  },
}
</script>

<style scoped>
.visualizer-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  gap: 24px;
}

.visualizer-container {
  flex: 0 0 35%;
  margin-top: 4%;
  width: 100%;
  min-height: 100px;
  background-color: var(--md-sys-color-surface-container-high);
  border-radius: 0;
  overflow: hidden;
  position: relative;
}

canvas {
  width: 100%;
  height: 100%;
  display: block;
}

.single-line-lyrics {
  flex: 1;
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  text-align: right;
  overflow: hidden;
}

.lyric-content {
  display: flex;
  flex-direction: column;
  width: 100%;
  gap: 8px;
}

.lyric-original {
  /* 与主歌词面板共用「歌词字号」倍率（变量写在根元素上，见 applyLyricsFontScale） */
  font-size: calc(32px * var(--lyrics-scale, 1));
  font-weight: 500;
  color: var(--md-sys-color-primary);
  line-height: 1.3;
  transition: all 0.3s ease;
  /* -webkit-line-clamp：最多显示 2 行 */
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.lyric-original-lrc {
  color: var(--md-sys-color-primary);
}

.lyric-original-ass {
  color: var(--md-sys-color-primary);
}

.lyric-translation {
  /* 同上：跟随歌词字号倍率 */
  font-size: calc(32px * var(--lyrics-scale, 1));
  color: var(--md-sys-color-primary);
  font-weight: 500;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* 卡拉OK 逐字高亮的"未唱部分"底色。判据与 supportsColorMix() 保持一致：
   自定义属性值不做语法校验，老 WebView 会原样保留 color-mix() 字符串，
   整条声明失效后配合 background-clip: text 会让歌词整行消失。 */
.karaoke-word {
  --inactive-color: var(--md-sys-color-outline);
  --active-color: var(--md-sys-color-primary);
  background-clip: text;
  -webkit-background-clip: text;
  color: transparent;
}

@supports (color: color-mix(in srgb, red 40%, blue)) {
  .karaoke-word {
    --inactive-color: color-mix(in srgb, var(--md-sys-color-primary) 40%, rgba(255, 255, 255, 0.1));
  }
}

.lyric-placeholder {
  color: var(--md-sys-color-outline);
  font-size: 24px;
}
</style>
