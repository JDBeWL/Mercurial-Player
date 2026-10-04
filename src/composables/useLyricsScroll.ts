/** 歌词滚动控制 composable,从 LyricsDisplay.vue 抽离:用户滚动判定 + 居中定位 + 双阶段滚动 */
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

  const isUserScroll = ref(false)
  const isAutoScrolling = ref(false)
  const isHovering = ref(false)
  let scrollTimeout: ReturnType<typeof setTimeout> | null = null

  // 跟踪滚动流程里所有 pending 定时器与 rAF,dispose 时统一取消,避免组件卸载后回调仍触发
  const disposed = { value: false } // 用对象包装,便于在下面的箭头函数里赋值
  const pendingTimers = new Set<ReturnType<typeof setTimeout>>()
  const pendingFrames = new Set<number>()

  const trackTimeout = (fn: () => void, ms: number): void => {
    if (disposed.value) return
    const id = setTimeout(() => {
      pendingTimers.delete(id)
      fn()
    }, ms)
    pendingTimers.add(id)
  }

  const trackFrame = (fn: () => void): void => {
    if (disposed.value) return
    const id = requestAnimationFrame(() => {
      pendingFrames.delete(id)
      fn()
    })
    pendingFrames.add(id)
  }

  const handleScroll = (): void => {
    // 程序化写 scrollTop 同样会触发 scroll 事件,只能靠 isAutoScrolling 区分自动与手动
    if (isAutoScrolling.value) return

    // 只有悬停在歌词区域内才算用户主动滚动
    if (!isHovering.value) return

    isUserScroll.value = true

    // 用户停止滚动 2.5s 后恢复自动跟随
    if (scrollTimeout) clearTimeout(scrollTimeout)
    scrollTimeout = setTimeout(() => {
      isUserScroll.value = false
    }, 2500)
  }

  // 居中偏移 = 行顶到容器中心的距离;首行上方空间不足时结果为负,故 clamp 到 0
  const computeCenteredScroll = (container: HTMLElement, activeEl: HTMLElement): number => {
    return Math.max(
      0,
      activeEl.offsetTop - container.clientHeight * 0.5 + activeEl.clientHeight / 2,
    )
  }

  // 挂载首帧在浏览器首次绘制前以瞬时滚动 (scroll-behavior: auto) 定位;
  // active 行 font-size 过渡 (24px->32px) 只改变自身高度,中心偏差约 4px 可忽略,不影响 offsetTop
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
    // 下一帧恢复 smooth 供后续自动跟随;此间 isAutoScrolling 仍为真,避免程序化滚动被误判为用户滚动
    trackFrame(() => {
      container.style.scrollBehavior = 'smooth'
      trackTimeout(() => (isAutoScrolling.value = false), 100)
    })
  }

  const scrollToActiveLyric = (immediate = false, isUserClick = false, targetIndex = -1): void => {
    if (!containerRef.value) return

    const idx = targetIndex !== -1 ? targetIndex : activeIndex.value
    if (idx === -1 || !lyrics.value.length) return

    const container = containerRef.value
    // 按索引取元素,比 querySelector(".active") 可靠
    const lyricElements = container.querySelectorAll<HTMLElement>('.lyrics')
    if (!lyricElements || !lyricElements[idx]) return

    const activeEl = lyricElements[idx]
    const computeTargetScroll = (): number => computeCenteredScroll(container, activeEl)

    // 见 handleScroll: 先置位,本次程序化滚动才算自动
    isAutoScrolling.value = true

    // 双阶段滚动:active 行 font-size 过渡 (0.15s,见 FONT_SIZE_TRANSITION_MS) 期间 offsetTop/clientHeight
    // 是中间值,直接读会定偏 (多行多句时偏差更大);先按当前尺寸预估,过渡完成后再用稳定尺寸修正
    // 用户点击的目标行原本不是 active 行,尺寸同样要等过渡到 32px 才稳定,故只走"等一下再滚"
    if (immediate || isUserClick) {
      trackTimeout(() => {
        const targetScroll = computeTargetScroll()
        container.style.scrollBehavior = 'auto'
        container.scrollTop = targetScroll
        trackFrame(() => {
          container.style.scrollBehavior = 'smooth'
          trackTimeout(() => (isAutoScrolling.value = false), 100)
        })
      }, FONT_SIZE_TRANSITION_MS)
    } else {
      void nextTick(() => {
        // 阶段 1: 用过渡中的尺寸预估
        const estimatedScroll = computeTargetScroll()
        container.style.scrollBehavior = 'smooth'
        container.scrollTop = estimatedScroll

        // 阶段 2: 稳定尺寸修正;平滑滚动的余波会连续触发 scroll,故 500ms 后才释放锁
        trackTimeout(() => {
          const correctedScroll = computeTargetScroll()
          container.scrollTop = correctedScroll
          trackTimeout(() => (isAutoScrolling.value = false), 500)
        }, FONT_SIZE_TRANSITION_MS)
      })
    }
  }

  const breakUserScrollLock = (): void => {
    isUserScroll.value = false
    if (scrollTimeout) clearTimeout(scrollTimeout)
  }

  const dispose = (): void => {
    disposed.value = true
    if (scrollTimeout) {
      clearTimeout(scrollTimeout)
      scrollTimeout = null
    }
    pendingTimers.forEach((id) => clearTimeout(id))
    pendingTimers.clear()
    pendingFrames.forEach((id) => cancelAnimationFrame(id))
    pendingFrames.clear()
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
