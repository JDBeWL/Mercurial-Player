// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { reactive, nextTick } from 'vue'
import VolumeControl from '@/components/VolumeControl.vue'

// 使用 vi.hoisted 创建可在 mock 工厂中引用的可变引用
const storeRef = vi.hoisted(() => ({ current: null as unknown }))

vi.mock('@/stores/player', () => ({
  usePlayerStore: () => storeRef.current,
}))

/** 创建一个响应式 mock store，模拟 playerStore 的状态和方法 */
function createMockStore(overrides: Record<string, unknown> = {}) {
  return reactive({
    volume: 0.5,
    isMuted: false,
    toggleMute: vi.fn(),
    setVolume: vi.fn(),
    ...overrides,
  })
}

/** 挂载组件，注入 $t mock 返回 key 本身 */
function mountComponent(store: ReturnType<typeof createMockStore>) {
  storeRef.current = store
  return mount(VolumeControl, {
    global: {
      mocks: {
        $t: (key: string) => key,
      },
    },
  })
}

describe('VolumeControl.vue', () => {
  let wrapper: ReturnType<typeof mount>
  let store: ReturnType<typeof createMockStore>

  beforeEach(() => {
    store = createMockStore()
  })

  afterEach(() => {
    if (wrapper) wrapper.unmount()
    vi.restoreAllMocks()
  })

  it('点击音量按钮调用 toggleMute', async () => {
    wrapper = mountComponent(store)
    await nextTick()
    const volumeBtn = wrapper.find('.volume-button')
    await volumeBtn.trigger('click')
    expect(store.toggleMute).toHaveBeenCalledTimes(1)
  })

  it('volume=0 时显示 volume_off 图标', async () => {
    store.volume = 0
    store.isMuted = false
    wrapper = mountComponent(store)
    await nextTick()
    const volumeBtn = wrapper.find('.volume-button')
    expect(volumeBtn.text()).toContain('volume_off')
  })

  it('volume<0.5 时显示 volume_down 图标', async () => {
    store.volume = 0.3
    store.isMuted = false
    wrapper = mountComponent(store)
    await nextTick()
    const volumeBtn = wrapper.find('.volume-button')
    expect(volumeBtn.text()).toContain('volume_down')
  })

  it('volume>=0.5 时显示 volume_up 图标', async () => {
    store.volume = 0.7
    store.isMuted = false
    wrapper = mountComponent(store)
    await nextTick()
    const volumeBtn = wrapper.find('.volume-button')
    expect(volumeBtn.text()).toContain('volume_up')
  })

  it('isMuted=true 时显示 volume_off 图标（无论音量值）', async () => {
    store.volume = 0.8
    store.isMuted = true
    wrapper = mountComponent(store)
    await nextTick()
    const volumeBtn = wrapper.find('.volume-button')
    expect(volumeBtn.text()).toContain('volume_off')
  })

  it('音量值显示为百分比', async () => {
    store.volume = 0.65
    wrapper = mountComponent(store)
    await nextTick()
    // 音量弹出框默认隐藏 (v-show)，但 DOM 仍存在
    const volumeValue = wrapper.find('.volume-value')
    expect(volumeValue.text()).toContain('65%')
  })

  it('音量弹出框默认隐藏', async () => {
    wrapper = mountComponent(store)
    await nextTick()
    const popup = wrapper.find('.volume-slider-popup')
    // v-show 控制可见性，元素存在但 display:none
    expect(popup.exists()).toBe(true)
    expect((popup.element as HTMLElement).style.display).toBe('none')
  })

  it('鼠标悬停时显示音量弹出框', async () => {
    wrapper = mountComponent(store)
    await nextTick()
    const container = wrapper.find('.volume-control-container')
    await container.trigger('mouseenter')
    await nextTick()
    const popup = wrapper.find('.volume-slider-popup')
    expect((popup.element as HTMLElement).style.display).not.toBe('none')
  })

  it('音量滑块填充高度随 volume 变化', async () => {
    store.volume = 0.4
    wrapper = mountComponent(store)
    await nextTick()
    const fill = wrapper.find('.slider-fill')
    expect(fill.attributes('style')).toContain('height: 40%')

    store.volume = 0.8
    await nextTick()
    expect(fill.attributes('style')).toContain('height: 80%')
  })
})
