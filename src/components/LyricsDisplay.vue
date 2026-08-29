<template>
  <div
    class="lyrics-wrapper"
    :class="`lyrics-style-${configStore.lyrics?.lyricsStyle || 'modern'}`"
  >
    <div
      ref="containerRef"
      class="lyrics-display"
      @scroll="handleScroll"
      @mouseenter="isHovering = true"
      @mouseleave="isHovering = false"
    >
      <div v-if="loading" class="loading">{{ $t('lyrics.loading') }}</div>

      <!-- 没有播放音乐时显示空闲状态 -->
      <div v-else-if="!hasCurrentTrack" class="no-lyrics idle-state">
        <span>{{ $t('lyrics.noTrackPlaying') }}</span>
      </div>

      <!-- 有音乐但没有歌词（提示文字与获取按钮均可在设置中隐藏，两者都隐藏时整个区域留空） -->
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

    <!-- 底部控制栏 -->
    <div v-if="lyrics.length || actionButtons.length" class="lyrics-bottom-bar">
      <!-- 插件操作按钮 -->
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

      <!-- 歌词偏移控制 -->
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
import { detectLyricLanguage, type LyricLanguage } from '@/utils/languageDetect'

export default {
  name: 'LyricsDisplay',
  components: { KaraokeLine },
  setup() {
    const playerStore = usePlayerStore()
    const configStore = useConfigStore()
    const containerRef = ref<HTMLElement | null>(null)

    // 按需加载歌词样式 CSS:
    // 监听 lyricsStyle 变化,首次切换到某样式时动态 import 对应 CSS。
    // 已加载的 CSS 会常驻 DOM,但由于 .lyrics-style-modern / .lyrics-style-classic
    // 选择器互斥,不会产生样式冲突。
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
    // 首次加载当前样式
    loadLyricsStyleCss(configStore.lyrics?.lyricsStyle || 'modern')
    // 监听样式切换
    watch(
      () => configStore.lyrics?.lyricsStyle,
      (newStyle) => {
        if (newStyle) loadLyricsStyleCss(newStyle)
      },
    )

    // 使用 composable
    const lyricsComposable = useLyrics()
    const { lyrics, loading, lyricsSource } = lyricsComposable

    // 本地高频 activeIndex，基于 visualTime 计算，避免滚动延迟
    const activeIndex = ref(-1)

    // 是否有当前播放的曲目
    const hasCurrentTrack = computed(() => !!playerStore.currentTrack)

    // 无歌词时的提示文字与获取按钮显隐（默认显示，旧配置缺字段时同样视为显示）
    const showNoLyricsHint = computed(() => configStore.lyrics?.showNoLyricsHint !== false)
    const showFetchLyricsButton = computed(
      () => configStore.lyrics?.showFetchLyricsButton !== false,
    )

    // 获取插件注册的操作按钮
    const actionButtons = computed(() => {
      return pluginManager.getExtensions('actionButtons').filter((btn) => btn.location === 'lyrics')
    })

    // 处理插件按钮点击
    const handleActionButton = async (btn: ActionButton & { pluginId: string }): Promise<void> => {
      try {
        await btn.action()
      } catch (error) {
        logger.error('插件按钮执行失败:', error)
      }
    }

    // 手动获取歌词状态
    const fetchingLyrics = ref(false)

    const handleFetchLyrics = async (): Promise<void> => {
      fetchingLyrics.value = true
      try {
        if (typeof lyricsComposable.fetchAndSaveLyrics === 'function') {
          await lyricsComposable.fetchAndSaveLyrics()
        } else {
          logger.error('fetchAndSaveLyrics is not a function')
        }
      } finally {
        fetchingLyrics.value = false
      }
    }

    // --- 视觉时间系统 ---
    // 将 visualTime ref 本身提供给 KaraokeLine（provide 不解包 ref），
    // 使逐字进度渲染依赖只存在于子组件中
    const { visualTime, advanceVisualTime, resetFrameClock, syncToCurrentTime } = useVisualTime()
    provide('lyricsVisualTime', visualTime)
    let rafId: number | null = null

    // 启动高频时间循环（仅在播放时运行）
    const startAnimationLoop = (): void => {
      if (rafId) return // 防止重复启动
      resetFrameClock()
      // 启动时先同步到真实时间
      syncToCurrentTime()

      const animate = (timestamp: number): void => {
        advanceVisualTime(timestamp)
        rafId = requestAnimationFrame(animate)
      }
      rafId = requestAnimationFrame(animate)
    }

    // 停止动画循环
    const stopAnimationLoop = (): void => {
      if (rafId) {
        cancelAnimationFrame(rafId)
        rafId = null
      }
    }

    // 监听播放状态，控制动画循环的启停
    watch(
      () => playerStore.isPlaying,
      (isPlaying) => {
        if (isPlaying) {
          startAnimationLoop()
        } else {
          stopAnimationLoop()
          // 暂停时同步到真实时间
          syncToCurrentTime()
        }
      },
      { immediate: true },
    )

    // 监听歌曲切换，立即重置 visualTime
    watch(
      () => playerStore.currentTrack?.path,
      () => {
        // 切歌时立即同步到当前时间（通常是 0）
        syncToCurrentTime()
        activeIndex.value = -1
      },
    )

    // 根据时间计算当前歌词索引（二分查找），并同步到 store
    const updateActiveIndex = (time: number): void => {
      // 应用歌词偏移
      const offset = playerStore.lyricsOffset || 0
      const currentTime = time - offset

      const idx = findLyricIndex(lyrics.value, currentTime)

      if (idx !== activeIndex.value) {
        activeIndex.value = idx
        playerStore.currentLyricIndex = idx // 同步到 store
      }
    }

    // 基于高频 visualTime 计算 activeIndex，实现即时滚动
    // 使用节流来减少计算频率
    let lastCalcTime = 0
    const CALC_INTERVAL = 50 // 每 50ms 计算一次，足够流畅且减少开销

    watch(visualTime, (time) => {
      if (!lyrics.value.length) {
        if (activeIndex.value !== -1) activeIndex.value = -1
        return
      }

      // 节流：避免每帧都计算
      const now = performance.now()
      if (now - lastCalcTime < CALC_INTERVAL) return
      lastCalcTime = now

      updateActiveIndex(time)
    })

    // 歌词行样式：将对齐方式与共享的歌词字体合并为 computed,
    // 避免在模板内联 style 中使用 CSS 自定义属性导致 vue-tsc 类型报错
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

    // 每行歌词的语言标注（原文/译文分别检测），供 lang 属性使用；
    // 仅依赖歌词数据本身，歌词不变时不会因滚动/激活状态变化而重算
    const lineLanguages = computed<[LyricLanguage, LyricLanguage][]>(() =>
      lyrics.value.map((line) => [
        detectLyricLanguage(line.texts[0]),
        detectLyricLanguage(line.texts[1]),
      ]),
    )

    // --- 样式计算逻辑 ---
    const isActive = (index: number): boolean => index === activeIndex.value

    // --- 滚动控制(状态机实现见 composables/useLyricsScroll) ---
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

    // 监听 activeIndex 变化以滚动
    watch(activeIndex, () => {
      if (suppressAutoScrollOnMount) return
      // 只有在非用户滚动状态下才自动跟随
      if (!isUserScroll.value) {
        scrollToActiveLyric()
      }
    })

    // 歌词加载完成后滚动到当前位置
    watch(loading, (newVal) => {
      if (!newVal) {
        // 歌词加载完成后，强制同步 visualTime
        visualTime.value = playerStore.currentTime
        nextTick(() => scrollToActiveLyric(true))
      }
    })

    // 用户点击歌词跳转
    const handleLyricClick = async (time: number, index: number): Promise<void> => {
      if (time < 0) return

      // 点击跳转应打破用户滚动锁定，并强制执行
      breakUserScrollLock()

      await playerStore.seek(time)

      visualTime.value = time
      const forceSync = (): void => {
        visualTime.value = playerStore.currentTime
      }
      requestAnimationFrame(forceSync)
      requestAnimationFrame(() => requestAnimationFrame(forceSync))

      // 明确传入目标 index，確保即使 DOM class 更新滞后也能正确找到元素
      nextTick(() => scrollToActiveLyric(true, true, index))
    }

    // 保存 resize 处理函数引用，以便正确清理
    const handleResize = (): void => scrollToActiveLyric(true)

    // 歌词偏移控制
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
      // 组件重新挂载时需立即恢复高亮与定位，挂载时主动根据当前播放位置计算。
      if (lyrics.value.length && !loading.value) {
        visualTime.value = playerStore.currentTime
        suppressAutoScrollOnMount = true
        updateActiveIndex(visualTime.value)
        // 避免 scrollToActiveLyric(true) 的 160ms 延迟导致淡入中途出现可见跳转
        nextTick(() => {
          suppressAutoScrollOnMount = false
          jumpToActiveLyric()
        })
      }
      // 动画循环由 watch(isPlaying) 控制启停，无需在此启动
      window.addEventListener('resize', handleResize)
    })

    onUnmounted(() => {
      stopAnimationLoop()
      // 清理滚动冷却定时器与状态
      dispose()
      // 清理 resize 事件监听器
      window.removeEventListener('resize', handleResize)
    })

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
      isHovering,
      fetchingLyrics,
      handleFetchLyrics,
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
  padding: 0 32px 0 8px;
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

.idle-icon {
  font-size: 64px;
  margin-bottom: 8px;
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

.fetch-lyrics-btn:hover:not(:disabled) {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
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

.lyrics-wrapper:hover .lyrics-bottom-bar {
  opacity: 1;
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

.action-btn:hover {
  background-color: var(--md-sys-color-surface-container-highest);
  color: var(--md-sys-color-primary);
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

.offset-btn:hover {
  background-color: var(--md-sys-color-surface-container-highest);
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

.offset-value:hover {
  background-color: var(--md-sys-color-surface-container-highest);
}

.lyrics-spacer-up {
  height: 30vh;
}

.lyrics-spacer-down {
  height: 45vh;
}
</style>
