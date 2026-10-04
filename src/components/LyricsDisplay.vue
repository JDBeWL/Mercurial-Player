<template>
  <div
    class="lyrics-wrapper"
    :class="`lyrics-style-${configStore.lyrics?.lyricsStyle || 'modern'}`"
    @click="handleBlankClick"
  >
    <div
      ref="containerRef"
      class="lyrics-display"
      @scroll="handleScroll"
      @mouseenter="isHovering = true"
      @mouseleave="isHovering = false"
    >
      <div v-if="loading" class="loading">{{ $t('lyrics.loading') }}</div>

      <div v-else-if="!hasCurrentTrack" class="no-lyrics idle-state">
        <span>{{ $t('lyrics.noTrackPlaying') }}</span>
      </div>

      <!-- 无歌词兜底区：显隐开关见 showNoLyricsHint / showFetchLyricsButton -->
      <div
        v-else-if="!lyrics.length && (showNoLyricsHint || showFetchLyricsButton)"
        class="no-lyrics"
      >
        <span v-if="showNoLyricsHint">{{ $t('lyrics.notFound') }}</span>
        <button
          v-if="showFetchLyricsButton"
          class="fetch-lyrics-btn"
          :disabled="fetchingLyrics"
          @click="handleFetchLyrics"
        >
          <span class="material-symbols-rounded">{{
            fetchingLyrics ? 'hourglass_empty' : 'cloud_download'
          }}</span>
          {{ fetchingLyrics ? $t('lyrics.fetching') : $t('lyrics.fetchOnline') }}
        </button>
      </div>

      <div v-else>
        <div class="lyrics-spacer-up"></div>

        <div
          v-for="(line, index) in lyrics"
          :key="`${line.time}-${index}`"
          class="lyrics"
          :class="{ active: isActive(index) }"
          :style="lyricLineStyle"
          @click="handleLyricClick(line.time, index)"
        >
          <template v-if="line.karaoke && isActive(index)">
            <div class="first-line karaoke-line" :lang="lineLanguages[index]?.[0] || undefined">
              <!-- 卡拉OK进度隔离在 KaraokeLine 内：父组件渲染不依赖每帧更新的 visualTime -->
              <KaraokeLine :words="line.words ?? []" />
            </div>
            <div
              v-if="line.texts[1]"
              class="last-line translation"
              :lang="lineLanguages[index]?.[1] || undefined"
              :style="translationStyle"
            >
              {{ line.texts[1] }}
            </div>
          </template>

          <template v-else>
            <div class="first-line" :lang="lineLanguages[index]?.[0] || undefined">
              {{ line.texts[0] }}
            </div>
            <div
              v-if="line.texts[1]"
              class="last-line translation"
              :lang="lineLanguages[index]?.[1] || undefined"
              :style="translationStyle"
            >
              {{ line.texts[1] }}
            </div>
          </template>
        </div>

        <div class="lyrics-spacer-down"></div>
      </div>
    </div>

    <div v-if="lyrics.length || actionButtons.length" class="lyrics-bottom-bar">
      <div v-if="actionButtons.length" class="plugin-action-buttons">
        <button
          v-for="btn in actionButtons"
          :key="btn.id"
          class="action-btn"
          :title="btn.name"
          @click="handleActionButton(btn)"
        >
          <span class="material-symbols-rounded">{{ btn.icon }}</span>
        </button>
      </div>

      <div v-if="lyrics.length" class="lyrics-offset-control">
        <button class="offset-btn" :title="$t('lyrics.offsetDelay')" @click="adjustOffset(-0.5)">
          <span class="material-symbols-rounded">remove</span>
        </button>
        <span class="offset-value" :title="$t('lyrics.offsetReset')" @click="resetOffset">
          {{ formatOffset(playerStore.lyricsOffset) }}
        </span>
        <button class="offset-btn" :title="$t('lyrics.offsetAdvance')" @click="adjustOffset(0.5)">
          <span class="material-symbols-rounded">add</span>
        </button>
      </div>
    </div>

    <LyricsCandidatePicker
      :visible="showPicker"
      :loading="pickerLoading || fetchingLyrics"
      :candidates="pickerCandidates"
      @close="showPicker = false"
      @apply="handleApplyCandidate"
      @auto-fetch="handleAutoFetchFromPicker"
    />
  </div>
</template>

<script lang="ts">
import { usePlayerStore } from '@/stores/player'
import { useConfigStore } from '@/stores/config'
import {
  provide,
  nextTick,
  ref,
  watch,
  onMounted,
  onUnmounted,
  computed,
  type CSSProperties,
} from 'vue'
import { useLyrics } from '@/composables/useLyrics'
import { useVisualTime } from '@/composables/useVisualTime'
import { useLyricsScroll } from '@/composables/useLyricsScroll'
import { useLyricsTypography } from '@/composables/useLyricsTypography'
import { findLyricIndex } from '@/utils/lyricsParser'
import { pluginManager } from '@/plugins'
import type { ActionButton } from '@/plugins/pluginManager'
import logger from '@/utils/logger'
import KaraokeLine from './KaraokeLine.vue'
import LyricsCandidatePicker from './lyrics/LyricsCandidatePicker.vue'
import type { LyricCandidate, LyricKind } from '@/services/lyrics'
import { detectLyricLanguage, type LyricLanguage } from '@/utils/languageDetect'

export default {
  name: 'LyricsDisplay',
  components: { KaraokeLine, LyricsCandidatePicker },
  // 竖屏下"点歌词空白处返回封面"：只有组件内部知道哪些元素可点，判定在这里，动作交给父组件（App.vue）
  emits: ['blankClick'],
  setup(_props, { emit }) {
    const playerStore = usePlayerStore()
    const configStore = useConfigStore()
    const containerRef = ref<HTMLElement | null>(null)

    // 已加载的样式 CSS 会常驻 DOM，但 .lyrics-style-modern / .lyrics-style-classic 选择器互斥，不会冲突
    const loadedLyricsStyles = new Set<string>()
    const loadLyricsStyleCss = async (style: string | undefined): Promise<void> => {
      const normalized = style || 'modern'
      if (loadedLyricsStyles.has(normalized)) return
      loadedLyricsStyles.add(normalized)
      try {
        if (normalized === 'classic') {
          await import('@/assets/css/lyrics-classic.css')
        } else {
          await import('@/assets/css/lyrics-modern.css')
        }
      } catch (e) {
        logger.error('加载歌词样式 CSS 失败:', e)
        loadedLyricsStyles.delete(normalized) // 失败时允许重试
      }
    }
    void loadLyricsStyleCss(configStore.lyrics?.lyricsStyle || 'modern')
    watch(
      () => configStore.lyrics?.lyricsStyle,
      (newStyle) => {
        if (newStyle) void loadLyricsStyleCss(newStyle)
      },
    )

    const lyricsComposable = useLyrics()
    const { lyrics, loading, lyricsSource } = lyricsComposable

    // 本地高频 activeIndex 由 visualTime 驱动，避免依赖 store 的秒级更新造成滚动延迟
    const activeIndex = ref(-1)

    const hasCurrentTrack = computed(() => !!playerStore.currentTrack)

    // 无歌词时的提示文字与获取按钮显隐（默认显示，旧配置缺字段时同样视为显示）
    const showNoLyricsHint = computed(() => configStore.lyrics?.showNoLyricsHint !== false)
    const showFetchLyricsButton = computed(
      () => configStore.lyrics?.showFetchLyricsButton !== false,
    )

    const actionButtons = computed(() => {
      return pluginManager.getExtensions('actionButtons').filter((btn) => btn.location === 'lyrics')
    })

    const handleActionButton = async (btn: ActionButton & { pluginId: string }): Promise<void> => {
      try {
        await btn.action()
      } catch (error) {
        logger.error('插件按钮执行失败:', error)
      }
    }

    const fetchingLyrics = ref(false)

    const showPicker = ref(false)
    const pickerCandidates = ref<LyricCandidate[]>([])
    const pickerLoading = ref(false)

    // 关掉时走手动挑选弹窗而不是自动取最优
    const autoSelectBestLyrics = computed(() => configStore.lyrics?.autoSelectBestLyrics !== false)

    const handleFetchLyrics = async (): Promise<void> => {
      if (autoSelectBestLyrics.value) {
        fetchingLyrics.value = true
        try {
          await lyricsComposable.fetchAndSaveLyrics()
        } finally {
          fetchingLyrics.value = false
        }
        return
      }
      showPicker.value = true
      pickerLoading.value = true
      pickerCandidates.value = []
      try {
        if (typeof lyricsComposable.fetchCandidates === 'function') {
          pickerCandidates.value = await lyricsComposable.fetchCandidates()
        }
      } catch (error) {
        logger.error('Failed to collect lyric candidates:', error)
        pickerCandidates.value = []
      } finally {
        pickerLoading.value = false
      }
    }

    const handleApplyCandidate = async (
      candidate: LyricCandidate,
      kind: LyricKind,
    ): Promise<void> => {
      fetchingLyrics.value = true
      try {
        const ok = await lyricsComposable.applyCandidate(candidate, kind)
        if (ok) showPicker.value = false
      } finally {
        fetchingLyrics.value = false
      }
    }

    const handleAutoFetchFromPicker = async (): Promise<void> => {
      fetchingLyrics.value = true
      try {
        await lyricsComposable.fetchAndSaveLyrics()
        showPicker.value = false
      } finally {
        fetchingLyrics.value = false
      }
    }

    // provide 的是 visualTime ref 本身（provide 不解包 ref），逐字进度的渲染依赖因此只留在子组件里
    const { visualTime, advanceVisualTime, resetFrameClock, syncToCurrentTime } = useVisualTime()
    provide('lyricsVisualTime', visualTime)
    let rafId: number | null = null

    const startAnimationLoop = (): void => {
      if (rafId) return // 防止重复启动
      resetFrameClock()
      syncToCurrentTime()

      const animate = (timestamp: number): void => {
        advanceVisualTime(timestamp)
        rafId = requestAnimationFrame(animate)
      }
      rafId = requestAnimationFrame(animate)
    }

    const stopAnimationLoop = (): void => {
      if (rafId) {
        cancelAnimationFrame(rafId)
        rafId = null
      }
    }

    watch(
      () => playerStore.isPlaying,
      (isPlaying) => {
        if (isPlaying) {
          startAnimationLoop()
        } else {
          stopAnimationLoop()
          // 暂停时把 visualTime 对齐真实时间，避免它跑在播放位置前面
          syncToCurrentTime()
        }
      },
      { immediate: true },
    )

    // 切歌时把 visualTime 归到当前时间（通常是 0），不能沿用上歌的时间轴
    watch(
      () => playerStore.currentTrack?.path,
      () => {
        syncToCurrentTime()
        activeIndex.value = -1
      },
    )

    // 二分查找当前行索引，并写回 store
    const updateActiveIndex = (time: number): void => {
      const offset = playerStore.lyricsOffset || 0
      const currentTime = time - offset

      const idx = findLyricIndex(lyrics.value, currentTime)

      if (idx !== activeIndex.value) {
        activeIndex.value = idx
        playerStore.currentLyricIndex = idx // 同步到 store
      }
    }

    let lastCalcTime = 0
    const CALC_INTERVAL = 50 // 每 50ms 计算一次，足够流畅且减少开销

    watch(visualTime, (time) => {
      if (!lyrics.value.length) {
        if (activeIndex.value !== -1) activeIndex.value = -1
        return
      }

      const now = performance.now()
      if (now - lastCalcTime < CALC_INTERVAL) return
      lastCalcTime = now

      updateActiveIndex(time)
    })

    // 对齐方式与共享歌词字体合并成 computed：模板内联 style 里写 CSS 自定义属性会让 vue-tsc 报错
    const { lyricFontStyle, translationStyle } = useLyricsTypography()

    const lyricLineStyle = computed<CSSProperties>(() => {
      const alignment = (configStore.lyrics?.lyricsAlignment || 'center') as
        'left' | 'center' | 'right'
      return {
        '--align-origin':
          alignment === 'right'
            ? 'right center'
            : alignment === 'center'
              ? 'center center'
              : 'left center',
        textAlign: alignment,
        ...lyricFontStyle.value,
      }
    })

    // 每行的 lang 标注（原文/译文分别检测）；只依赖歌词数据，滚动/激活变化不会触发重算
    const lineLanguages = computed<[LyricLanguage, LyricLanguage][]>(() =>
      lyrics.value.map((line) => [
        detectLyricLanguage(line.texts[0]),
        detectLyricLanguage(line.texts[1]),
      ]),
    )

    const isActive = (index: number): boolean => index === activeIndex.value

    // 滚动状态机实现在 composables/useLyricsScroll
    const {
      isUserScroll,
      isHovering,
      handleScroll,
      jumpToActiveLyric,
      scrollToActiveLyric,
      breakUserScrollLock,
      dispose,
    } = useLyricsScroll({ containerRef, activeIndex, lyrics })

    // 挂载初始化期间由 onMounted 显式定位，抑制 activeIndex 变化触发的自动跟随滚动
    let suppressAutoScrollOnMount = false

    // forceSync 连锁 rAF 的 id 集合，卸载时取消
    const syncFrameIds = new Set<number>()

    watch(activeIndex, () => {
      if (suppressAutoScrollOnMount) return
      if (!isUserScroll.value) {
        scrollToActiveLyric()
      }
    })

    watch(loading, (newVal) => {
      if (!newVal) {
        // 加载完成时 visualTime 可能已落后于播放位置，先强制同步再滚动
        visualTime.value = playerStore.currentTime
        void nextTick(() => scrollToActiveLyric(true))
      }
    })

    const handleLyricClick = async (time: number, index: number): Promise<void> => {
      if (time < 0) return

      // 点击跳转必须打破用户滚动锁定，否则滚轮留下的锁会让点击歌词不跟过去
      breakUserScrollLock()

      await playerStore.seek(time)

      visualTime.value = time
      const forceSync = (): void => {
        visualTime.value = playerStore.currentTime
      }
      // 先取消上一轮还没跑的 forceSync，避免两串 rAF 互相覆盖
      syncFrameIds.forEach((id) => cancelAnimationFrame(id))
      syncFrameIds.clear()
      const trackFrame = (fn: () => void): void => {
        const id = requestAnimationFrame(() => {
          syncFrameIds.delete(id)
          fn()
        })
        syncFrameIds.add(id)
      }
      trackFrame(forceSync)
      trackFrame(() => trackFrame(forceSync))

      // 明确传入目标 index，确保 DOM class 更新滞后时也能找到元素
      void nextTick(() => scrollToActiveLyric(true, true, index))
    }

    // 具名函数才能成对地 removeEventListener
    const handleResize = (): void => scrollToActiveLyric(true)

    const adjustOffset = (delta: number): void => {
      playerStore.adjustLyricsOffset(delta)
    }

    const resetOffset = (): void => {
      playerStore.resetLyricsOffset()
    }

    const formatOffset = (offset: number): string => {
      if (offset === 0) return '0s'
      const sign = offset > 0 ? '+' : ''
      return `${sign}${offset.toFixed(1)}s`
    }

    onMounted(() => {
      // 组件重新挂载时需立即恢复高亮与定位，挂载时主动按当前播放位置计算
      if (lyrics.value.length && !loading.value) {
        visualTime.value = playerStore.currentTime
        suppressAutoScrollOnMount = true
        updateActiveIndex(visualTime.value)
        // 避免 scrollToActiveLyric(true) 的 160ms 延迟导致淡入中途出现可见跳转
        void nextTick(() => {
          suppressAutoScrollOnMount = false
          jumpToActiveLyric()
        })
      }
      // 动画循环由 watch(isPlaying) 控制启停，无需在此启动
      window.addEventListener('resize', handleResize)
    })

    onUnmounted(() => {
      stopAnimationLoop()
      // dispose 负责清滚动冷却定时器与交互状态
      dispose()
      // 取消 pending 的 forceSync rAF
      syncFrameIds.forEach((id) => cancelAnimationFrame(id))
      syncFrameIds.clear()
      window.removeEventListener('resize', handleResize)
    })

    // 歌词行是整宽块级元素，closest('.lyrics') 会把行内大片留白也算成点歌词，
    // 所以用 Range 量出文字实际矩形（CSS px）；getClientRects() 为空时保守返回 true
    const isPointOnLyricText = (event: MouseEvent, line: HTMLElement): boolean => {
      const range = document.createRange()
      range.selectNodeContents(line)
      const rects = Array.from(range.getClientRects())
      if (!rects.length) return true
      return rects.some(
        (rect) =>
          event.clientX >= rect.left &&
          event.clientX <= rect.right &&
          event.clientY >= rect.top &&
          event.clientY <= rect.bottom,
      )
    }

    // 只有确实落在空白上才发事件：排除 button（获取歌词/插件动作/偏移加减）、.offset-value（重置偏移）、
    // .lyrics-bottom-bar（控制区）和歌词文字（语义是跳到这一句，见 isPointOnLyricText）
    const handleBlankClick = (event: MouseEvent): void => {
      const el = event.target as HTMLElement | null
      if (el?.closest('button, .offset-value, .lyrics-bottom-bar')) return
      const line = el?.closest('.lyrics') as HTMLElement | null
      if (line && isPointOnLyricText(event, line)) return
      emit('blankClick')
    }

    return {
      lyrics,
      loading,
      containerRef,
      configStore,
      lyricsSource,
      hasCurrentTrack,
      showNoLyricsHint,
      showFetchLyricsButton,
      playerStore,
      isActive,
      handleLyricClick,
      handleScroll,
      handleBlankClick,
      isHovering,
      fetchingLyrics,
      handleFetchLyrics,
      showPicker,
      pickerCandidates,
      pickerLoading,
      handleApplyCandidate,
      handleAutoFetchFromPicker,
      adjustOffset,
      resetOffset,
      formatOffset,
      actionButtons,
      handleActionButton,
      lyricLineStyle,
      translationStyle,
      lineLanguages,
    }
  },
}
</script>

<style scoped>
.lyrics-wrapper {
  width: 100%;
  height: 100%;
  position: relative;
  overflow: hidden;
}

.lyrics-display {
  height: 100%;
  /* 20px = 原左右 (8+32)/2，唱词光学中心不变；32px 那侧本是给可见滚动条留的，
     滚动条已隐藏（见下面的 scrollbar-width / ::-webkit-scrollbar），改成两侧等宽。竖屏在文件末尾再收窄 */
  padding: 0 20px;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: none;
  scroll-behavior: smooth;
}

.lyrics-display::-webkit-scrollbar {
  display: none;
}

.loading,
.no-lyrics {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 24px;
  gap: 16px;
}

.idle-state {
  color: var(--md-sys-color-on-surface-variant);
  opacity: 0.6;
}

.fetch-lyrics-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 24px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

@media (hover: hover) {
  .fetch-lyrics-btn:hover:not(:disabled) {
    background-color: color-mix(
      in srgb,
      var(--md-sys-color-on-surface) 8%,
      var(--md-sys-color-secondary-container)
    );
  }
}

.fetch-lyrics-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.fetch-lyrics-btn .material-symbols-rounded {
  font-size: 20px;
}

.lyrics-bottom-bar {
  position: absolute;
  bottom: 16px;
  left: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  pointer-events: none;
  opacity: 0;
  transition: opacity 0.3s ease;
}

@media (hover: hover) {
  .lyrics-wrapper:hover .lyrics-bottom-bar {
    opacity: 1;
  }
}

.plugin-action-buttons {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 20px;
  pointer-events: auto;
  margin-right: auto;
}

.action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: 50%;
  background-color: transparent;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: all 0.2s ease;
}

@media (hover: hover) {
  .action-btn:hover {
    background-color: var(--md-sys-color-surface-container-highest);
    color: var(--md-sys-color-primary);
  }
}

.action-btn .material-symbols-rounded {
  font-size: 20px;
}

.lyrics-offset-control {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 20px;
  pointer-events: auto;
}

.offset-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 50%;
  background-color: transparent;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

@media (hover: hover) {
  .offset-btn:hover {
    background-color: var(--md-sys-color-surface-container-highest);
  }
}

.offset-btn .material-symbols-rounded {
  font-size: 16px;
}

.offset-value {
  min-width: 40px;
  text-align: center;
  font-size: 11px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 10px;
  transition: background-color 0.2s ease;
}

@media (hover: hover) {
  .offset-value:hover {
    background-color: var(--md-sys-color-surface-container-highest);
  }
}

.lyrics-spacer-up {
  height: 30vh;
}

.lyrics-spacer-down {
  height: 45vh;
}

/* 竖屏：左右仍然对称（12px / 12px），只是比横屏的 20px 更贴边，给唱词让出横向空间。
   只按方向判断、不挂平台守卫："窄屏必须能看"是兜底规则，桌面窗口拉成窄高时同样该放宽 */
@media (orientation: portrait) {
  .lyrics-display {
    padding: 0 12px;
  }
}
</style>
