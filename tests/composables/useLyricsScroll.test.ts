// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'
import type { LyricLine } from '@/types'

const { useLyricsScroll } = await import('@/composables/useLyricsScroll')

const CONTAINER_HEIGHT = 400
const LINE_HEIGHT = 40
const USER_SCROLL_COOLDOWN = 2500
const FONT_SIZE_TRANSITION_MS = 160

let frameCallbacks: Array<() => void>
const disposers: Array<() => void> = []

/** 构造带 .lyrics 子行的滚动容器,行高与容器高度决定居中位置 */
const buildContainer = (lineCount: number) => {
  const container = document.createElement('div')
  Object.defineProperty(container, 'clientHeight', { value: CONTAINER_HEIGHT, configurable: true })

  for (let i = 0; i < lineCount; i++) {
    const line = document.createElement('div')
    line.className = 'lyrics'
    Object.defineProperty(line, 'offsetTop', { value: i * LINE_HEIGHT, configurable: true })
    Object.defineProperty(line, 'clientHeight', { value: LINE_HEIGHT, configurable: true })
    container.appendChild(line)
  }

  document.body.appendChild(container)
  return container
}

/** 居中定位:offsetTop - 容器高度一半 + 行高一半 */
const expectedScroll = (index: number) =>
  Math.max(0, index * LINE_HEIGHT - CONTAINER_HEIGHT * 0.5 + LINE_HEIGHT / 2)

const mount = (lineCount: number, activeIndex = 0) => {
  const containerRef = ref<HTMLElement | null>(buildContainer(lineCount))
  const activeIndexRef = ref(activeIndex)
  const lyrics = ref<LyricLine[]>(
    Array.from({ length: lineCount }, (_, i) => ({ time: i, text: `line ${i}` })) as LyricLine[],
  )
  const api = useLyricsScroll({ containerRef, activeIndex: activeIndexRef, lyrics })
  disposers.push(api.dispose)
  return { containerRef, activeIndexRef, lyrics, ...api }
}

const flushFrames = () => {
  const callbacks = frameCallbacks
  frameCallbacks = []
  for (const cb of callbacks) cb()
}

const scroll = () => window.dispatchEvent(new Event('scroll'))

beforeEach(() => {
  vi.useFakeTimers()
  frameCallbacks = []
  disposers.length = 0
  document.body.innerHTML = ''

  vi.stubGlobal('requestAnimationFrame', (cb: () => void) => {
    frameCallbacks.push(cb)
    return frameCallbacks.length
  })
  vi.stubGlobal('cancelAnimationFrame', () => {})
})

afterEach(() => {
  for (const dispose of disposers) dispose()
  disposers.length = 0
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('useLyricsScroll > user scroll detection', () => {
  it('ignores scroll events triggered by the automatic scrolling', () => {
    const { isAutoScrolling, isHovering, isUserScroll, handleScroll } = mount(10)
    isAutoScrolling.value = true
    isHovering.value = true

    handleScroll()

    expect(isUserScroll.value).toBe(false)
  })

  it('ignores scroll events while the pointer is outside the lyric area', () => {
    const { isHovering, isUserScroll, handleScroll } = mount(10)
    isHovering.value = false

    handleScroll()

    expect(isUserScroll.value).toBe(false)
  })

  it('marks a manual scroll while hovering', () => {
    const { isHovering, isUserScroll, handleScroll } = mount(10)
    isHovering.value = true

    handleScroll()

    expect(isUserScroll.value).toBe(true)
  })

  it('releases the lock after the cooldown', () => {
    const { isHovering, isUserScroll, handleScroll } = mount(10)
    isHovering.value = true

    handleScroll()
    vi.advanceTimersByTime(USER_SCROLL_COOLDOWN - 1)
    expect(isUserScroll.value).toBe(true)

    vi.advanceTimersByTime(1)
    expect(isUserScroll.value).toBe(false)
  })

  it('restarts the cooldown on every scroll event', () => {
    const { isHovering, isUserScroll, handleScroll } = mount(10)
    isHovering.value = true

    handleScroll()
    vi.advanceTimersByTime(USER_SCROLL_COOLDOWN - 500)
    handleScroll()
    vi.advanceTimersByTime(USER_SCROLL_COOLDOWN - 500)

    expect(isUserScroll.value).toBe(true)

    vi.advanceTimersByTime(500)
    expect(isUserScroll.value).toBe(false)
  })

  it('breaks the lock immediately on demand', () => {
    const { isHovering, isUserScroll, handleScroll, breakUserScrollLock } = mount(10)
    isHovering.value = true
    handleScroll()
    expect(isUserScroll.value).toBe(true)

    breakUserScrollLock()

    expect(isUserScroll.value).toBe(false)
    // 残留的冷却定时器不应再把状态改回去
    vi.advanceTimersByTime(USER_SCROLL_COOLDOWN)
    expect(isUserScroll.value).toBe(false)
  })

  it('survives a scroll event dispatched on the window', () => {
    const { isHovering, isUserScroll, handleScroll } = mount(10)
    isHovering.value = true
    // 组件通过模板绑定 handleScroll,这里直接调用等价入口
    scroll()
    handleScroll()

    expect(isUserScroll.value).toBe(true)
  })
})

describe('useLyricsScroll > jumpToActiveLyric', () => {
  it('centres the active line instantly', () => {
    const { containerRef, isAutoScrolling, jumpToActiveLyric } = mount(10, 3)

    jumpToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(expectedScroll(3))
    expect(containerRef.value!.style.scrollBehavior).toBe('auto')
    expect(isAutoScrolling.value).toBe(true)
  })

  it('restores smooth scrolling on the next frame', () => {
    const { containerRef, jumpToActiveLyric } = mount(10, 3)

    jumpToActiveLyric()
    flushFrames()

    expect(containerRef.value!.style.scrollBehavior).toBe('smooth')
  })

  it('clears the auto-scrolling flag shortly after the jump', () => {
    const { containerRef, isAutoScrolling, jumpToActiveLyric } = mount(10, 3)

    jumpToActiveLyric()
    flushFrames()
    expect(isAutoScrolling.value).toBe(true)

    vi.advanceTimersByTime(100)
    expect(isAutoScrolling.value).toBe(false)
    expect(containerRef.value!.style.scrollBehavior).toBe('smooth')
  })

  it('does nothing when there is no container', () => {
    const { containerRef, jumpToActiveLyric } = mount(10, 3)
    containerRef.value = null

    expect(() => jumpToActiveLyric()).not.toThrow()
  })

  it('does nothing when no line is active', () => {
    const { containerRef, jumpToActiveLyric } = mount(10, -1)

    jumpToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(0)
  })

  it('does nothing when the lyric list is empty', () => {
    const { containerRef, lyrics, jumpToActiveLyric } = mount(0, 0)
    lyrics.value = []

    jumpToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(0)
  })

  it('does nothing when the active line has no element', () => {
    const { containerRef, activeIndexRef, jumpToActiveLyric } = mount(10, 0)
    activeIndexRef.value = 99

    jumpToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(0)
  })
})

describe('useLyricsScroll > scrollToActiveLyric', () => {
  it('does nothing when there is no container', () => {
    const { containerRef, scrollToActiveLyric } = mount(10, 3)
    containerRef.value = null

    expect(() => scrollToActiveLyric()).not.toThrow()
  })

  it('does nothing when no line is active', () => {
    const { containerRef, scrollToActiveLyric } = mount(10, -1)

    scrollToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(0)
  })

  it('does nothing when the lyric list is empty', () => {
    const { containerRef, lyrics, scrollToActiveLyric } = mount(0, 0)
    lyrics.value = []

    scrollToActiveLyric()

    expect(containerRef.value!.scrollTop).toBe(0)
  })

  it('does nothing when the target line has no element', () => {
    const { containerRef, scrollToActiveLyric } = mount(10, 0)

    scrollToActiveLyric(false, false, 99)

    expect(containerRef.value!.scrollTop).toBe(0)
  })

  it('scrolls to an explicit target index', async () => {
    const { containerRef, scrollToActiveLyric } = mount(10, 0)

    scrollToActiveLyric(false, false, 6)
    await nextTick()

    expect(containerRef.value!.scrollTop).toBe(expectedScroll(6))
  })

  it('estimates first and corrects after the font-size transition', async () => {
    const { containerRef, isAutoScrolling, scrollToActiveLyric } = mount(10, 3)

    scrollToActiveLyric()
    await nextTick()

    // 阶段 1:立即用当前尺寸预估,并切到 smooth
    expect(containerRef.value!.scrollTop).toBe(expectedScroll(3))
    expect(containerRef.value!.style.scrollBehavior).toBe('smooth')
    expect(isAutoScrolling.value).toBe(true)

    // 阶段 2:过渡完成后按稳定尺寸修正
    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS)
    expect(containerRef.value!.scrollTop).toBe(expectedScroll(3))

    vi.advanceTimersByTime(500)
    expect(isAutoScrolling.value).toBe(false)
  })

  it('waits for the transition before scrolling on a user click', async () => {
    const { containerRef, scrollToActiveLyric } = mount(10, 3)

    scrollToActiveLyric(false, true)
    await nextTick()
    expect(containerRef.value!.scrollTop).toBe(0)

    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS)
    expect(containerRef.value!.scrollTop).toBe(expectedScroll(3))
    expect(containerRef.value!.style.scrollBehavior).toBe('auto')

    flushFrames()
    expect(containerRef.value!.style.scrollBehavior).toBe('smooth')
  })

  it('waits for the transition on an immediate scroll too', async () => {
    const { containerRef, scrollToActiveLyric } = mount(10, 5)

    scrollToActiveLyric(true)
    await nextTick()
    expect(containerRef.value!.scrollTop).toBe(0)

    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS)
    expect(containerRef.value!.scrollTop).toBe(expectedScroll(5))
  })

  it('uses a longer settle window for the auto-follow correction', async () => {
    const { isAutoScrolling, scrollToActiveLyric } = mount(10, 3)

    scrollToActiveLyric()
    await nextTick()
    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS)
    expect(isAutoScrolling.value).toBe(true)

    vi.advanceTimersByTime(499)
    expect(isAutoScrolling.value).toBe(true)

    vi.advanceTimersByTime(1)
    expect(isAutoScrolling.value).toBe(false)
  })

  it('clamps the target at zero for the very first lines', async () => {
    const { containerRef, activeIndexRef, scrollToActiveLyric } = mount(10, 0)
    activeIndexRef.value = 0

    scrollToActiveLyric()
    await nextTick()

    // 前几行的居中值是负数,必须被夹到 0
    expect(expectedScroll(0)).toBe(0)
    expect(containerRef.value!.scrollTop).toBe(0)
  })
})

describe('useLyricsScroll > dispose', () => {
  it('clears the user scroll cooldown', () => {
    const { isHovering, isUserScroll, handleScroll, dispose } = mount(10)
    isHovering.value = true
    handleScroll()

    dispose()

    expect(isUserScroll.value).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('resets the auto-scrolling flag', () => {
    const { isAutoScrolling, jumpToActiveLyric, dispose } = mount(10, 3)
    jumpToActiveLyric()
    expect(isAutoScrolling.value).toBe(true)

    dispose()

    expect(isAutoScrolling.value).toBe(false)
  })

  it('drops pending timers and frames scheduled by a scroll', async () => {
    const { containerRef, isAutoScrolling, scrollToActiveLyric, dispose } = mount(10, 3)
    const scrollTopBefore = containerRef.value!.scrollTop

    scrollToActiveLyric()
    await nextTick()
    dispose()

    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS + 500)
    flushFrames()

    // dispose 之后不应再有任何滚动修正
    expect(containerRef.value!.scrollTop).toBe(
      scrollTopBefore === 0 ? expectedScroll(3) : scrollTopBefore,
    )
    expect(isAutoScrolling.value).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('ignores scroll requests issued after disposal', async () => {
    const { containerRef, jumpToActiveLyric, scrollToActiveLyric, dispose } = mount(10, 3)
    dispose()

    jumpToActiveLyric()
    scrollToActiveLyric()
    await nextTick()
    vi.advanceTimersByTime(FONT_SIZE_TRANSITION_MS + 500)
    flushFrames()

    expect(containerRef.value!.scrollTop).toBe(0)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('is safe to call twice', () => {
    const { isHovering, handleScroll, dispose } = mount(10)
    isHovering.value = true
    handleScroll()

    dispose()
    expect(() => dispose()).not.toThrow()
  })
})
