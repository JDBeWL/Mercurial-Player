// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { reactive, nextTick } from 'vue'
import PlayerControls from '@/components/PlayerControls.vue'

// 使用 vi.hoisted 创建可在 mock 工厂中引用的可变引用
const storeRef = vi.hoisted(() => ({ current: null as unknown }))

vi.mock('@/stores/player', () => ({
  usePlayerStore: () => storeRef.current,
}))

/** 创建一个响应式 mock store，模拟 playerStore 的状态和方法 */
function createMockStore(overrides: Record<string, unknown> = {}) {
  return reactive({
    isPlaying: false,
    isShuffle: false,
    repeatMode: 'none',
    volume: 0.5,
    isMuted: false,
    hasNextTrack: true,
    hasPreviousTrack: true,
    togglePlay: vi.fn(),
    toggleShuffle: vi.fn(),
    toggleRepeat: vi.fn(),
    toggleMute: vi.fn(),
    setVolume: vi.fn(),
    nextTrack: vi.fn(),
    previousTrack: vi.fn(),
    ...overrides,
  })
}

/** 挂载组件，注入 $t mock 返回 key 本身 */
function mountComponent(store: ReturnType<typeof createMockStore>) {
  storeRef.current = store
  return mount(PlayerControls, {
    global: {
      mocks: {
        $t: (key: string) => key,
      },
    },
  })
}

describe('PlayerControls.vue', () => {
  let wrapper: ReturnType<typeof mount>
  let store: ReturnType<typeof createMockStore>

  beforeEach(() => {
    store = createMockStore()
  })

  afterEach(() => {
    if (wrapper) wrapper.unmount()
    vi.restoreAllMocks()
  })

  // ---------- 播放/暂停按钮 ----------

  describe('播放/暂停按钮', () => {
    it('未播放时显示 play_arrow 图标', async () => {
      store.isPlaying = false
      wrapper = mountComponent(store)
      await nextTick()
      const playBtn = wrapper.find('.play-button')
      expect(playBtn.text()).toContain('play_arrow')
    })

    it('播放中显示 pause 图标', async () => {
      store.isPlaying = true
      wrapper = mountComponent(store)
      await nextTick()
      const playBtn = wrapper.find('.play-button')
      expect(playBtn.text()).toContain('pause')
    })

    it('点击播放按钮调用 togglePlay', async () => {
      store.isPlaying = false
      wrapper = mountComponent(store)
      await nextTick()
      await wrapper.find('.play-button').trigger('click')
      expect(store.togglePlay).toHaveBeenCalledTimes(1)
    })

    it('点击暂停按钮也调用 togglePlay', async () => {
      store.isPlaying = true
      wrapper = mountComponent(store)
      await nextTick()
      await wrapper.find('.play-button').trigger('click')
      expect(store.togglePlay).toHaveBeenCalledTimes(1)
    })

    it('isPlaying 切换时图标实时更新', async () => {
      store.isPlaying = false
      wrapper = mountComponent(store)
      await nextTick()
      expect(wrapper.find('.play-button').text()).toContain('play_arrow')

      store.isPlaying = true
      await nextTick()
      expect(wrapper.find('.play-button').text()).toContain('pause')
    })
  })

  // ---------- 上一首/下一首按钮 ----------

  describe('上一首/下一首按钮', () => {
    it('hasPreviousTrack=false 时上一首按钮 disabled', async () => {
      store.hasPreviousTrack = false
      wrapper = mountComponent(store)
      await nextTick()
      // 上一首按钮是第二个 icon-button (shuffle 之后)
      const buttons = wrapper.findAll('.icon-button')
      const prevBtn = buttons[1]
      expect(prevBtn.attributes('disabled')).toBeDefined()
    })

    it('hasPreviousTrack=true 时上一首按钮可点击', async () => {
      store.hasPreviousTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const prevBtn = buttons[1]
      expect(prevBtn.attributes('disabled')).toBeUndefined()
    })

    it('点击上一首按钮调用 previousTrack', async () => {
      store.hasPreviousTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[1].trigger('click')
      expect(store.previousTrack).toHaveBeenCalledTimes(1)
    })

    it('hasNextTrack=false 时下一首按钮 disabled', async () => {
      store.hasNextTrack = false
      wrapper = mountComponent(store)
      await nextTick()
      // 下一首按钮是第四个 icon-button (shuffle, prev, play, next)
      const buttons = wrapper.findAll('.icon-button')
      const nextBtn = buttons[3]
      expect(nextBtn.attributes('disabled')).toBeDefined()
    })

    it('hasNextTrack=true 时下一首按钮可点击', async () => {
      store.hasNextTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const nextBtn = buttons[3]
      expect(nextBtn.attributes('disabled')).toBeUndefined()
    })

    it('点击下一首按钮调用 nextTrack', async () => {
      store.hasNextTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[3].trigger('click')
      expect(store.nextTrack).toHaveBeenCalledTimes(1)
    })
  })

  // ---------- 随机/循环模式 ----------

  describe('随机模式', () => {
    it('isShuffle=false 时 shuffle 按钮无 active 类', async () => {
      store.isShuffle = false
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const shuffleBtn = buttons[0]
      expect(shuffleBtn.classes()).not.toContain('active')
    })

    it('isShuffle=true 时 shuffle 按钮有 active 类', async () => {
      store.isShuffle = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const shuffleBtn = buttons[0]
      expect(shuffleBtn.classes()).toContain('active')
    })

    it('点击 shuffle 按钮调用 toggleShuffle', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[0].trigger('click')
      expect(store.toggleShuffle).toHaveBeenCalledTimes(1)
    })
  })

  describe('循环模式', () => {
    it('repeatMode=none 时循环按钮无 active 类', async () => {
      store.repeatMode = 'none'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.classes()).not.toContain('active')
    })

    it('repeatMode=list 时循环按钮有 active 类', async () => {
      store.repeatMode = 'list'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.classes()).toContain('active')
    })

    it('repeatMode=track 时循环按钮有 active 类', async () => {
      store.repeatMode = 'track'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.classes()).toContain('active')
    })

    it('repeatMode=track 时显示 repeat_one 图标', async () => {
      store.repeatMode = 'track'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.text()).toContain('repeat_one')
    })

    it('repeatMode=list 时显示 repeat 图标', async () => {
      store.repeatMode = 'list'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.text()).toContain('repeat')
      expect(repeatBtn.text()).not.toContain('repeat_one')
    })

    it('repeatMode=none 时显示 repeat 图标', async () => {
      store.repeatMode = 'none'
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const repeatBtn = buttons[4]
      expect(repeatBtn.text()).toContain('repeat')
      expect(repeatBtn.text()).not.toContain('repeat_one')
    })

    it('点击循环按钮调用 toggleRepeat', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[4].trigger('click')
      expect(store.toggleRepeat).toHaveBeenCalledTimes(1)
    })
  })

  // ---------- 音量控制 ----------

  describe('音量控制', () => {
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
      expect(popup.element.style.display).toBe('none')
    })

    it('鼠标悬停时显示音量弹出框', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      const container = wrapper.find('.volume-control-container')
      await container.trigger('mouseenter')
      await nextTick()
      const popup = wrapper.find('.volume-slider-popup')
      expect(popup.element.style.display).not.toBe('none')
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

  // ---------- 综合状态切换 ----------

  describe('状态切换实时反映', () => {
    it('从播放切换到暂停时图标更新', async () => {
      store.isPlaying = true
      wrapper = mountComponent(store)
      await nextTick()
      expect(wrapper.find('.play-button').text()).toContain('pause')

      store.isPlaying = false
      await nextTick()
      expect(wrapper.find('.play-button').text()).toContain('play_arrow')
    })

    it('shuffle 状态切换时 active 类更新', async () => {
      store.isShuffle = false
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      expect(buttons[0].classes()).not.toContain('active')

      store.isShuffle = true
      await nextTick()
      expect(buttons[0].classes()).toContain('active')
    })
  })
})
