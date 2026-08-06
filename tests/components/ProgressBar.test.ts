// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { reactive, nextTick } from 'vue'
import ProgressBar from '@/components/ProgressBar.vue'

// 使用 vi.hoisted 创建可在 mock 工厂中引用的可变引用
// mock 工厂在模块导入前执行，必须用 hoisted 才能访问外部变量
const storeRef = vi.hoisted(() => ({ current: null as unknown }))

vi.mock('@/stores/player', () => ({
  usePlayerStore: () => storeRef.current,
}))

/** 创建一个响应式 mock store，模拟 playerStore 的状态和方法 */
function createMockStore(overrides: Record<string, unknown> = {}) {
  return reactive({
    currentTime: 0,
    duration: 100,
    seek: vi.fn(),
    ...overrides,
  })
}

/** 模拟 getBoundingClientRect，让进度条有可计算的尺寸 */
function mockBoundingClientRect(el: HTMLElement, rect: { left: number; width: number }) {
  el.getBoundingClientRect = vi.fn(() => ({
    left: rect.left,
    top: 0,
    width: rect.width,
    height: 16,
    right: rect.left + rect.width,
    bottom: 16,
    x: rect.left,
    y: 0,
    toJSON: () => ({}),
  })) as unknown as typeof el.getBoundingClientRect
}

/** 在 document 上派发鼠标事件 */
function dispatchMouseEvent(type: string, clientX: number) {
  document.dispatchEvent(new MouseEvent(type, { clientX, bubbles: true }))
}

describe('ProgressBar.vue', () => {
  let wrapper: ReturnType<typeof mount>
  let store: ReturnType<typeof createMockStore>

  beforeEach(() => {
    vi.useFakeTimers()
    store = createMockStore()
    storeRef.current = store
  })

  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
    if (wrapper) wrapper.unmount()
  })

  // ---------- 进度条渲染 ----------

  describe('渲染', () => {
    it('根据 currentTime / duration 计算填充宽度百分比', async () => {
      store.duration = 100
      store.currentTime = 50
      wrapper = mount(ProgressBar)
      await nextTick()
      const fill = wrapper.find('.progress-bar-fill')
      // 50% 进度
      expect(fill.attributes('style')).toContain('width: 50%')
    })

    it('duration=0 时填充宽度为 0%', async () => {
      store.duration = 0
      store.currentTime = 10
      wrapper = mount(ProgressBar)
      await nextTick()
      const fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 0%')
    })

    it('currentTime > duration 时百分比可超过 100%（不截断）', async () => {
      store.duration = 100
      store.currentTime = 150
      wrapper = mount(ProgressBar)
      await nextTick()
      const fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 150%')
    })

    it('currentTime=0 时填充宽度为 0%', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()
      const fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 0%')
    })
  })

  // ---------- 时间格式化 (mm:ss) ----------

  describe('时间格式化', () => {
    it('悬停时显示 tooltip，格式为 "当前时间 / 总时长"', async () => {
      store.duration = 100 // 1:40
      store.currentTime = 50 // 0:50
      wrapper = mount(ProgressBar)
      await nextTick()

      // 初始无 tooltip
      expect(wrapper.find('.hover-time-tooltip').exists()).toBe(false)

      // 触发鼠标进入
      await wrapper.find('.progress-bar-wrapper').trigger('mouseenter')
      await nextTick()

      const tooltip = wrapper.find('.hover-time-tooltip')
      expect(tooltip.exists()).toBe(true)
      // 50 秒 => "0:50", 100 秒 => "1:40"
      expect(tooltip.text()).toContain('0:50')
      expect(tooltip.text()).toContain('1:40')
    })

    it('整分钟时间格式化为 "m:00"', async () => {
      store.duration = 120 // 2:00
      store.currentTime = 60 // 1:00
      wrapper = mount(ProgressBar)
      await nextTick()
      await wrapper.find('.progress-bar-wrapper').trigger('mouseenter')
      await nextTick()
      const tooltip = wrapper.find('.hover-time-tooltip')
      expect(tooltip.text()).toContain('1:00')
      expect(tooltip.text()).toContain('2:00')
    })

    it('鼠标离开后 tooltip 消失', async () => {
      store.duration = 100
      store.currentTime = 50
      wrapper = mount(ProgressBar)
      await nextTick()

      await wrapper.find('.progress-bar-wrapper').trigger('mouseenter')
      await nextTick()
      expect(wrapper.find('.hover-time-tooltip').exists()).toBe(true)

      await wrapper.find('.progress-bar-wrapper').trigger('mouseleave')
      await nextTick()
      expect(wrapper.find('.hover-time-tooltip').exists()).toBe(false)
    })
  })

  // ---------- 点击/拖拽 seek ----------

  describe('点击 seek', () => {
    it('点击进度条触发 seek 到对应位置', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      // 模拟在 50% 位置按下并释放鼠标
      await wrapperEl.trigger('mousedown', { clientX: 50 })
      dispatchMouseEvent('mouseup', 50)
      vi.advanceTimersByTime(1100)
      await nextTick()

      expect(store.seek).toHaveBeenCalledWith(50)
    })

    it('duration=0 时点击不触发 seek', async () => {
      store.duration = 0
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      await wrapperEl.trigger('mousedown', { clientX: 50 })
      dispatchMouseEvent('mouseup', 50)
      vi.advanceTimersByTime(1100)
      await nextTick()

      expect(store.seek).not.toHaveBeenCalled()
    })

    it('点击最左端 seek 到 0', async () => {
      store.duration = 100
      store.currentTime = 50
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      await wrapperEl.trigger('mousedown', { clientX: 0 })
      dispatchMouseEvent('mouseup', 0)
      vi.advanceTimersByTime(1100)
      await nextTick()

      expect(store.seek).toHaveBeenCalledWith(0)
    })

    it('点击最右端 seek 到 duration', async () => {
      store.duration = 200
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      await wrapperEl.trigger('mousedown', { clientX: 100 })
      dispatchMouseEvent('mouseup', 100)
      vi.advanceTimersByTime(1100)
      await nextTick()

      expect(store.seek).toHaveBeenCalledWith(200)
    })
  })

  describe('拖拽 seek', () => {
    it('拖拽到新位置后 seek 到拖拽位置', async () => {
      store.duration = 100
      store.currentTime = 10
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      // 按下 -> 拖动 -> 释放
      await wrapperEl.trigger('mousedown', { clientX: 20 })
      dispatchMouseEvent('mousemove', 80)
      dispatchMouseEvent('mouseup', 80)
      vi.advanceTimersByTime(1100)
      await nextTick()

      // seek 到 80% 位置 = 80 秒
      expect(store.seek).toHaveBeenCalledWith(80)
    })

    it('拖拽过程中显示拖拽位置的时间而非当前播放时间', async () => {
      store.duration = 100
      store.currentTime = 10 // 0:10
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      // 按下在 50% 位置
      await wrapperEl.trigger('mousedown', { clientX: 50 })
      await nextTick()

      // 拖拽 tooltip 应该显示拖拽位置的时间 (50秒 = 0:50)
      const tooltip = wrapper.find('.hover-time-tooltip')
      expect(tooltip.exists()).toBe(true)
      expect(tooltip.text()).toContain('0:50')
      // 不应显示原始 currentTime (0:10)
      expect(tooltip.text()).not.toContain('0:10')
    })

    it('拖拽时进度条填充跟随拖拽位置', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      await wrapperEl.trigger('mousedown', { clientX: 70 })
      await nextTick()

      const fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 70%')
    })

    it('拖拽百分比被限制在 0-100 范围内', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      // 拖到超出左边界
      await wrapperEl.trigger('mousedown', { clientX: -50 })
      await nextTick()
      const fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 0%')
    })
  })

  // ---------- pendingSeek 状态 ----------

  describe('pendingSeek 状态', () => {
    it('seek 后 currentTime 接近目标时清除 pendingSeek', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      // seek 到 80
      await wrapperEl.trigger('mousedown', { clientX: 80 })
      dispatchMouseEvent('mouseup', 80)
      await nextTick()

      // 此时处于 pendingSeek 状态，显示位置应保持在 80%
      let fill = wrapper.find('.progress-bar-fill')
      expect(fill.attributes('style')).toContain('width: 80%')

      // 模拟播放器更新 currentTime 到接近目标 (误差 < 0.5s)
      store.currentTime = 79.8
      await nextTick()

      // pendingSeek 被清除，显示位置跟随真实 currentTime
      fill = wrapper.find('.progress-bar-fill')
      // 79.8 / 100 * 100 = 79.8%
      expect(fill.attributes('style')).toContain('width: 79.8%')
    })

    it('seek 后 1 秒超时自动清除 pendingSeek', async () => {
      store.duration = 100
      store.currentTime = 0
      wrapper = mount(ProgressBar)
      await nextTick()

      const wrapperEl = wrapper.find('.progress-bar-wrapper')
      mockBoundingClientRect(wrapperEl.element as HTMLElement, { left: 0, width: 100 })

      await wrapperEl.trigger('mousedown', { clientX: 80 })
      dispatchMouseEvent('mouseup', 80)
      await nextTick()

      // pendingSeek 中，显示 80%
      expect(wrapper.find('.progress-bar-fill').attributes('style')).toContain('width: 80%')

      // 快进 1 秒后超时
      vi.advanceTimersByTime(1100)
      await nextTick()

      // pendingSeek 清除后跟随真实 currentTime (仍为 0)
      expect(wrapper.find('.progress-bar-fill').attributes('style')).toContain('width: 0%')
    })
  })
})
