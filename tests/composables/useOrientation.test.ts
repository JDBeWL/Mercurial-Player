// @vitest-environment happy-dom
import { describe, it, beforeEach, afterEach, expect, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'

import { useOrientation, type OrientationInfo } from '@/composables/useOrientation'

/** 当前"屏幕"方向；matchMedia 会按这个值回答 portrait 查询 */
let portrait = true

/**
 * 替换 window.matchMedia。
 * 必须让 `(orientation: portrait)` 与真实语义一致：竖屏为 true、其余查询为 false，
 * 否则测的就不是组件实际使用的判定路径了。
 */
function installMatchMedia(): void {
  window.matchMedia = vi.fn((query: string) => {
    return {
      matches: query.includes('portrait') ? portrait : false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }
  }) as unknown as typeof window.matchMedia
}

/** 模拟系统方向变化：浏览器会因此在 window 上派发 resize */
function rotate(next: boolean): void {
  portrait = next
  window.dispatchEvent(new Event('resize'))
}

interface Harness {
  wrapper: VueWrapper
  info: OrientationInfo
}

function mountHarness(): Harness {
  const harness = {} as Harness
  harness.wrapper = mount(
    defineComponent({
      setup() {
        harness.info = useOrientation()
        return () => h('div')
      },
    }),
  )
  return harness
}

describe('useOrientation', () => {
  let harness: Harness | undefined

  beforeEach(() => {
    portrait = true
    installMatchMedia()
  })

  afterEach(() => {
    harness?.wrapper.unmount()
    harness = undefined
    vi.restoreAllMocks()
  })

  it('竖屏时 isPortrait 为 true、isLandscape 为 false', () => {
    harness = mountHarness()
    expect(harness.info.isPortrait.value).toBe(true)
    expect(harness.info.isLandscape.value).toBe(false)
  })

  it('横屏时 isPortrait 为 false、isLandscape 为 true', () => {
    portrait = false
    harness = mountHarness()
    expect(harness.info.isPortrait.value).toBe(false)
    expect(harness.info.isLandscape.value).toBe(true)
  })

  it('方向变化派发 resize 后状态翻转', () => {
    harness = mountHarness()
    expect(harness.info.isPortrait.value).toBe(true)

    rotate(false)
    expect(harness.info.isPortrait.value).toBe(false)

    rotate(true)
    expect(harness.info.isPortrait.value).toBe(true)
  })

  it('orientationchange 也能触发同步（老 WebView 的主信号）', () => {
    harness = mountHarness()
    portrait = false
    window.dispatchEvent(new Event('orientationchange'))
    expect(harness.info.isPortrait.value).toBe(false)
  })

  it('挂载时注册 resize / orientationchange 监听', () => {
    const addSpy = vi.spyOn(window, 'addEventListener')
    harness = mountHarness()
    expect(addSpy).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(addSpy).toHaveBeenCalledWith('orientationchange', expect.any(Function))
  })

  it('卸载时移除监听', () => {
    const removeSpy = vi.spyOn(window, 'removeEventListener')
    harness = mountHarness()
    harness.wrapper.unmount()
    expect(removeSpy).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(removeSpy).toHaveBeenCalledWith('orientationchange', expect.any(Function))
    harness = undefined
  })

  it('多个使用方共享同一份状态，逐个卸载不会提前摘掉监听', () => {
    const first = mountHarness()
    const second = mountHarness()
    expect(first.info.isPortrait.value).toBe(true)

    // 只卸载其中一个：监听仍在，状态继续跟随方向
    first.wrapper.unmount()
    rotate(false)
    expect(second.info.isPortrait.value).toBe(false)

    harness = second
  })
})
