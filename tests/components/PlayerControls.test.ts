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

// mock vue-i18n（组件 setup 中调用 useI18n 获取 t 函数）
vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}))

/** 创建一个响应式 mock store，模拟 playerStore 的状态和方法 */
function createMockStore(overrides: Record<string, unknown> = {}) {
  return reactive({
    isPlaying: false,
    isShuffle: false,
    repeatMode: 'none',
    hasNextTrack: true,
    hasPreviousTrack: true,
    playlist: [] as unknown[],
    togglePlay: vi.fn(),
    toggleShuffle: vi.fn(),
    toggleRepeat: vi.fn(),
    cyclePlayMode: vi.fn(),
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
      const prevBtn = buttons[1]!
      expect(prevBtn.attributes('disabled')).toBeDefined()
    })

    it('hasPreviousTrack=true 时上一首按钮可点击', async () => {
      store.hasPreviousTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const prevBtn = buttons[1]!
      expect(prevBtn.attributes('disabled')).toBeUndefined()
    })

    it('点击上一首按钮调用 previousTrack', async () => {
      store.hasPreviousTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[1]!.trigger('click')
      expect(store.previousTrack).toHaveBeenCalledTimes(1)
    })

    it('hasNextTrack=false 时下一首按钮 disabled', async () => {
      store.hasNextTrack = false
      wrapper = mountComponent(store)
      await nextTick()
      // 下一首按钮是第四个 icon-button (shuffle, prev, play, next)
      const buttons = wrapper.findAll('.icon-button')
      const nextBtn = buttons[3]!
      expect(nextBtn.attributes('disabled')).toBeDefined()
    })

    it('hasNextTrack=true 时下一首按钮可点击', async () => {
      store.hasNextTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      const nextBtn = buttons[3]!
      expect(nextBtn.attributes('disabled')).toBeUndefined()
    })

    it('点击下一首按钮调用 nextTrack', async () => {
      store.hasNextTrack = true
      wrapper = mountComponent(store)
      await nextTick()
      const buttons = wrapper.findAll('.icon-button')
      await buttons[3]!.trigger('click')
      expect(store.nextTrack).toHaveBeenCalledTimes(1)
    })
  })

  // ---------- 播放模式（随机 / 循环合并成一颗按钮） ----------

  describe('播放模式按钮', () => {
    /** 播放模式按钮是 .controls-row 里的第 0 颗（原本的随机按钮位置） */
    const modeBtnOf = (w: ReturnType<typeof mount>) => w.findAll('.icon-button')[0]!

    /** 顺序态的判定靠图标上的 .is-order（那一道斜线），不再靠 active 高亮 */
    const modeIconOf = (w: ReturnType<typeof mount>) => w.find('.play-mode-icon')

    it('顺序播放：显示 repeat 且带 is-order 斜线，且不加 active 高亮', async () => {
      store.isShuffle = false
      store.repeatMode = 'none'
      wrapper = mountComponent(store)
      await nextTick()
      expect(modeBtnOf(wrapper).classes()).not.toContain('active')
      const icon = modeIconOf(wrapper)
      expect(icon.text()).toContain('repeat')
      expect(icon.text()).not.toContain('repeat_one')
      expect(icon.classes()).toContain('is-order')
    })

    it('列表循环：显示 repeat 且没有斜线，不加 active 高亮', async () => {
      store.repeatMode = 'list'
      wrapper = mountComponent(store)
      await nextTick()
      expect(modeBtnOf(wrapper).classes()).not.toContain('active')
      const icon = modeIconOf(wrapper)
      expect(icon.text()).toContain('repeat')
      expect(icon.text()).not.toContain('repeat_one')
      expect(icon.classes()).not.toContain('is-order')
    })

    it('单曲循环：显示 repeat_one，不加 active 高亮', async () => {
      store.repeatMode = 'track'
      wrapper = mountComponent(store)
      await nextTick()
      expect(modeBtnOf(wrapper).classes()).not.toContain('active')
      const icon = modeIconOf(wrapper)
      expect(icon.text()).toContain('repeat_one')
      expect(icon.classes()).not.toContain('is-order')
    })

    it('随机播放：显示 shuffle，不加 active 高亮', async () => {
      store.isShuffle = true
      wrapper = mountComponent(store)
      await nextTick()
      expect(modeBtnOf(wrapper).classes()).not.toContain('active')
      const icon = modeIconOf(wrapper)
      expect(icon.text()).toContain('shuffle')
      expect(icon.classes()).not.toContain('is-order')
    })

    it('点击播放模式按钮调用 cyclePlayMode', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      await modeBtnOf(wrapper).trigger('click')
      expect(store.cyclePlayMode).toHaveBeenCalledTimes(1)
    })

    it('底部一行只有 4 颗主按钮（随机与循环已合并）', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      expect(wrapper.findAll('.controls-row .icon-button')).toHaveLength(4)
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

    it('播放模式切换时图标与斜线标记同步更新', async () => {
      wrapper = mountComponent(store)
      await nextTick()
      const icon = wrapper.find('.play-mode-icon')
      expect(icon.classes()).toContain('is-order')
      expect(icon.text()).toContain('repeat')

      store.isShuffle = true
      await nextTick()
      expect(icon.classes()).not.toContain('is-order')
      expect(icon.text()).toContain('shuffle')
    })
  })
})
