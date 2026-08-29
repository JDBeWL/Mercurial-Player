/**
 * 歌词滚动控制 composable
 *
 * 从 LyricsDisplay.vue 抽离:用户滚动判定(悬停 + 2.5s 冷却)、
 * 居中定位计算、双阶段滚动(预估 + font-size 过渡完成后修正)。
 */
import { ref, nextTick, type Ref } from 'vue'
import type { LyricLine } from '@/types'

/** active 行 font-size 过渡时长(0.15s)+ 10ms 余量,须与样式中的 0.15s 保持一致 */
const FONT_SIZE_TRANSITION_MS = 160

export function useLyricsScroll(deps: {
  /** 滚动容器 */
  containerRef: Ref<HTMLElement | null>
  /** 当前高亮行索引 */
  activeIndex: Ref<number>
  /** 歌词行列表(用于判断非空) */
  lyrics: Ref<LyricLine[]>
}): {
  /** 用户是否正在手动滚动(冷却期内) */
  isUserScroll: Ref<boolean>
  /** 是否正在程序化自动滚动 */
  isAutoScrolling: Ref<boolean>
  /** 鼠标是否悬停在歌词区(模板 @mouseenter/@mouseleave 绑定) */
  isHovering: Ref<boolean>
  /** 容器 @scroll 处理 */
  handleScroll: () => void
  /** 挂载首帧瞬时定位 */
  jumpToActiveLyric: () => void
  /** 滚动到当前高亮行 */
  scrollToActiveLyric: (immediate?: boolean, isUserClick?: boolean, targetIndex?: number) => void
  /** 打破用户滚动锁定(用户点击歌词跳转时调用) */
  breakUserScrollLock: () => void
  /** 组件卸载时调用:清理滚动冷却定时器 */
  dispose: () => void
} {
  const { containerRef, activeIndex, lyrics } = deps

  const isUserScroll = ref(false) // 标记用户是否正在交互
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

  // 计算目标滚动位置
  const computeCenteredScroll = (container: HTMLElement, activeEl: HTMLElement): number => {
    return Math.max(
      0,
      activeEl.offsetTop - container.clientHeight * 0.5 + activeEl.clientHeight / 2,
    )
  }

  // 挂载首帧定位：在浏览器首次绘制前以瞬时滚动 (scroll-behavior: auto)
  // 注：active 行 font-size 过渡 (24px→32px) 只影响其自身高度
  // (中心定位偏差约 4px，可忽略)，不影响 offsetTop，无需等过渡完成。
  const jumpToActiveLyric = (): void => {
    const container = containerRef.value
    if (!container) return
    const idx = activeIndex.value
    if (idx === -1 || !lyrics.value.length) return
    const activeEl = container.querySelectorAll<HTMLElement>('.lyrics')[idx]
    if (!activeEl) return

    isAutoScrolling.value = true
    container.style.scrollBehavior = 'auto'
    container.scrollTop = computeCenteredScroll(container, activeEl)
    // 下一帧恢复 smooth，供后续自动跟随使用；同时避免程序化滚动被误判为用户滚动
    requestAnimationFrame(() => {
      container.style.scrollBehavior = 'smooth'
      setTimeout(() => (isAutoScrolling.value = false), 100)
    })
  }

  const scrollToActiveLyric = (immediate = false, isUserClick = false, targetIndex = -1): void => {
    if (!containerRef.value) return

    const idx = targetIndex !== -1 ? targetIndex : activeIndex.value
    // 如果索引无效或列表为空
    if (idx === -1 || !lyrics.value.length) return

    const container = containerRef.value
    // 直接通过索引查找元素，比 querySelector(".active") 更可靠
    const lyricElements = container.querySelectorAll<HTMLElement>('.lyrics')
    if (!lyricElements || !lyricElements[idx]) return

    const activeEl = lyricElements[idx]
    const computeTargetScroll = (): number => computeCenteredScroll(container, activeEl)

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

  const breakUserScrollLock = (): void => {
    isUserScroll.value = false
    if (scrollTimeout) clearTimeout(scrollTimeout)
  }

  const dispose = (): void => {
    if (scrollTimeout) {
      clearTimeout(scrollTimeout)
      scrollTimeout = null
    }
    isUserScroll.value = false
    isAutoScrolling.value = false
  }

  return {
    isUserScroll,
    isAutoScrolling,
    isHovering,
    handleScroll,
    jumpToActiveLyric,
    scrollToActiveLyric,
    breakUserScrollLock,
    dispose,
  }
}
