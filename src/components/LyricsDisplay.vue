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

      <!-- 有音乐但没有歌词 -->
      <div v-else-if="!lyrics.length" class="no-lyrics">
        <span>{{ $t('lyrics.notFound') }}</span>
        <button
          v-if="configStore.lyrics?.enableOnlineFetch || true"
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
            <div class="first-line karaoke-line">
              <span
                v-for="(word, idx) in line.words"
                :key="idx"
                class="karaoke-text"
                :class="{ active: isWordActive(word) }"
                :style="getKaraokeStyle(word)"
                >{{ word.text }}</span
              >
            </div>
            <div v-if="line.texts[1]" class="last-line translation">{{ line.texts[1] }}</div>
          </template>

          <template v-else>
            <div class="first-line">{{ line.texts[0] }}</div>
            <div v-if="line.texts[1]" class="last-line translation">{{ line.texts[1] }}</div>
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
import { nextTick, ref, watch, onMounted, onUnmounted, computed, type CSSProperties } from 'vue'
import { useLyrics } from '@/composables/useLyrics'
import { pluginManager } from '@/plugins'
import type { ActionButton } from '@/plugins/pluginManager'
import logger from '@/utils/logger'
import type { KaraokeWord } from '@/types'

export default {
  name: 'LyricsDisplay',
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
    const visualTime = ref(0)
    const isUserScroll = ref(false) // 标记用户是否正在交互
    let rafId: number | null = null
    let lastFrameTime = 0

    // 启动高频时间循环（仅在播放时运行）
    const startAnimationLoop = (): void => {
      if (rafId) return // 防止重复启动
      lastFrameTime = 0 // 重置时间戳
      // 启动时先同步到真实时间
      visualTime.value = playerStore.currentTime

      const animate = (timestamp: number): void => {
        if (!lastFrameTime) lastFrameTime = timestamp
        const deltaTime = Math.min((timestamp - lastFrameTime) / 1000, 0.1) // 限制最大 deltaTime 为 100ms
        lastFrameTime = timestamp

        const realTime = playerStore.currentTime

        // 播放中：基于帧间隔累加时间，并动态调整速度以消除漂移
        const diff = visualTime.value - realTime // 正值表示视觉领先，负值表示落后

        if (Math.abs(diff) > 0.5) {
          // 误差超过 0.5s，直接硬同步
          visualTime.value = realTime
        } else if (Math.abs(diff) > 0.05) {
          // 误差在 0.05s ~ 0.5s 之间，使用 P 控制器平滑追赶
          const speed = 1.0 - diff * 2.0
          const clampedSpeed = Math.max(0.7, Math.min(1.3, speed))
          visualTime.value += deltaTime * clampedSpeed
        } else {
          // 误差很小，正常累加
          visualTime.value += deltaTime
        }

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
          visualTime.value = playerStore.currentTime
        }
      },
      { immediate: true },
    )

    // 监听真实时间跳变（如拖拽进度条），立即同步
    watch(
      () => playerStore.currentTime,
      (newTime, oldTime) => {
        // 检测 seek 操作：时间跳变超过 1.5s（正常播放每次只增加 0.5s）
        // 或者时间倒退（说明用户往回拖了）
        const jump = newTime - oldTime
        if (Math.abs(jump) > 1.5 || jump < -0.1) {
          visualTime.value = newTime
        }
      },
    )

    // 监听歌曲切换，立即重置 visualTime
    watch(
      () => playerStore.currentTrack?.path,
      () => {
        // 切歌时立即同步到当前时间（通常是 0）
        visualTime.value = playerStore.currentTime
        activeIndex.value = -1
      },
    )

    // 基于高频 visualTime 计算 activeIndex，实现即时滚动
    // 使用节流来减少计算频率，避免每帧都触发响应式更新
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

      // 应用歌词偏移
      const offset = playerStore.lyricsOffset || 0
      const currentTime = time - offset

      // 二分查找当前歌词索引
      let l = 0,
        r = lyrics.value.length - 1,
        idx = -1
      while (l <= r) {
        const mid = (l + r) >> 1
        if (lyrics.value[mid].time <= currentTime) {
          idx = mid
          l = mid + 1
        } else {
          r = mid - 1
        }
      }

      if (idx !== activeIndex.value) {
        activeIndex.value = idx
        playerStore.currentLyricIndex = idx // 同步到 store
      }
    })

    // 歌词行样式：将对齐方式、字体和缩放锚点合并为 computed,
    // 避免在模板内联 style 中使用 CSS 自定义属性导致 vue-tsc 类型报错
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
        fontFamily: configStore.lyrics?.lyricsFontFamily || 'Roboto',
      }
    })

    // --- 样式计算逻辑 ---
    const isActive = (index: number): boolean => index === activeIndex.value

    const isWordActive = (word: KaraokeWord): boolean => {
      // 应用歌词偏移
      const offset = playerStore.lyricsOffset || 0
      const t = visualTime.value - offset
      // 只有在时间范围内才算激活，并且考虑下一个单词的开始时间
      return t >= word.start && t < word.end
    }

    // 计算卡拉OK单词的填充进度 (0% - 100%)
    const getKaraokeStyle = (word: KaraokeWord): CSSProperties => {
      // 应用歌词偏移
      const offset = playerStore.lyricsOffset || 0
      const t = visualTime.value - offset
      if (t >= word.end) return { '--progress': '100%' }
      if (t < word.start) return { '--progress': '0%' }

      // 确保进度计算精确，避免浮点数误差
      const duration = word.end - word.start
      const elapsed = t - word.start
      const progress = Math.min(100, Math.max(0, (elapsed / duration) * 100))
      return { '--progress': `${progress.toFixed(2)}%` }
    }

    // --- 滚动控制 ---
    const isAutoScrolling = ref(false) // 标记是否正在自动滚动
    const isHovering = ref(false) // 标记鼠标是否悬停
    let scrollTimeout: ReturnType<typeof setTimeout> | null = null

    const handleScroll = (): void => {
      // 如果是自动滚动触发的事件，忽略
      if (isAutoScrolling.value) return

      // 只有当鼠标悬停在歌词区域时，才认为是用户的主动滚动
      if (!isHovering.value) return

      // 用户手动滚动
      isUserScroll.value = true

      // 用户停止滚动 2.5s 后恢复自动跟随
      if (scrollTimeout) clearTimeout(scrollTimeout)
      scrollTimeout = setTimeout(() => {
        isUserScroll.value = false
      }, 2500)
    }

    const scrollToActiveLyric = (
      immediate = false,
      isUserClick = false,
      targetIndex = -1,
    ): void => {
      if (!containerRef.value) return

      const idx = targetIndex !== -1 ? targetIndex : activeIndex.value
      // 如果索引无效或列表为空
      if (idx === -1 || !lyrics.value.length) return

      const container = containerRef.value
      // 直接通过索引查找元素，比 querySelector(".active") 更可靠
      const lyricElements = container.querySelectorAll<HTMLElement>('.lyrics')
      if (!lyricElements || !lyricElements[idx]) return

      const activeEl = lyricElements[idx]

      // 计算目标滚动位置 (把 active 行中心放到容器中心)
      const computeTargetScroll = (): number => {
        const containerH = container.clientHeight
        const elTop = activeEl.offsetTop
        const elH = activeEl.clientHeight
        const offsetRatio = 0.5
        return Math.max(0, elTop - containerH * offsetRatio + elH / 2)
      }

      // 标记开始自动滚动，防止 handleScroll 误判
      isAutoScrolling.value = true

      // 双阶段滚动策略:
      // 经典模式 active 行 font-size 从 24px→32px (0.15s transition),
      // 过渡期间 offsetTop/clientHeight 是中间值,直接读会导致定位不准
      // (多行多句场景高度变化更大,偏差更明显)。
      //
      // 阶段 1 (nextTick): 立即用当前尺寸做预估滚动,让用户立即看到响应
      // 阶段 2 (setTimeout 160ms): 等 font-size 过渡完成后,用稳定尺寸修正
      //
      // immediate/isUserClick 场景 (用户点击跳转): 只做一次立即滚动 (用稳定后的尺寸),
      //   因为用户点击的行之前不是 active,立即读到的尺寸是稳定的 24px 状态,
      //   但目标位置应该是 32px 状态,所以也等过渡完成
      const FONT_SIZE_TRANSITION_MS = 160 // 0.15s + 10ms 余量

      if (immediate || isUserClick) {
        // 用户点击: 等 font-size 过渡完成后一次性滚到准确位置
        setTimeout(() => {
          const targetScroll = computeTargetScroll()
          container.style.scrollBehavior = 'auto'
          container.scrollTop = targetScroll
          requestAnimationFrame(() => {
            container.style.scrollBehavior = 'smooth'
            setTimeout(() => (isAutoScrolling.value = false), 100)
          })
        }, FONT_SIZE_TRANSITION_MS)
      } else {
        // 自动跟随: 立即预估滚动 + 过渡完成后修正
        nextTick(() => {
          // 阶段 1: 立即用当前 (过渡中) 尺寸预估
          const estimatedScroll = computeTargetScroll()
          container.style.scrollBehavior = 'smooth'
          container.scrollTop = estimatedScroll

          // 阶段 2: 等 font-size 过渡完成后用稳定尺寸修正
          setTimeout(() => {
            const correctedScroll = computeTargetScroll()
            container.scrollTop = correctedScroll
            setTimeout(() => (isAutoScrolling.value = false), 500)
          }, FONT_SIZE_TRANSITION_MS)
        })
      }
    }

    // 监听 activeIndex 变化以滚动
    watch(activeIndex, () => {
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
      isUserScroll.value = false
      if (scrollTimeout) clearTimeout(scrollTimeout)

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
      // 动画循环由 watch(isPlaying) 控制启停，无需在此启动
      window.addEventListener('resize', handleResize)
    })

    onUnmounted(() => {
      stopAnimationLoop()
      // 清理 scrollTimeout
      if (scrollTimeout) {
        clearTimeout(scrollTimeout)
        scrollTimeout = null
      }
      // 清理 resize 事件监听器
      window.removeEventListener('resize', handleResize)
      // 清理任何其他可能的定时器或事件监听器
      isUserScroll.value = false
      isAutoScrolling.value = false
    })

    return {
      lyrics,
      loading,
      containerRef,
      configStore,
      lyricsSource,
      hasCurrentTrack,
      playerStore,
      isActive,
      isWordActive,
      getKaraokeStyle,
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
  padding: 0 32px;
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
