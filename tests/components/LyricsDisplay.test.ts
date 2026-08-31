// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { ref, reactive, nextTick } from 'vue'
import LyricsDisplay from '@/components/LyricsDisplay.vue'
import type { LyricLine, Track } from '@/types'

// 使用 vi.hoisted 创建可在 mock 工厂中引用的可变引用
// mock 工厂在模块导入前执行，必须用 hoisted 才能访问外部变量
const mocks = vi.hoisted(() => ({
  playerStore: null as unknown,
  configStore: null as unknown,
  lyricsState: null as unknown as Record<string, unknown>,
}))

vi.mock('@/stores/player', () => ({
  usePlayerStore: () => mocks.playerStore,
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: () => mocks.configStore,
}))

// mock useLyrics composable，返回可控的 lyrics/loading refs
vi.mock('@/composables/useLyrics', () => ({
  useLyrics: () => mocks.lyricsState,
}))

// mock 插件管理器
vi.mock('@/plugins', () => ({
  pluginManager: {
    getExtensions: vi.fn(() => []),
  },
}))

// mock logger
vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

// mock 动态 CSS 导入
vi.mock('@/assets/css/lyrics-modern.css', () => ({}))
vi.mock('@/assets/css/lyrics-classic.css', () => ({}))

/** 创建测试用歌词数据 */
function makeLyrics(): LyricLine[] {
  return [
    { time: 0, texts: ['第一行歌词'] },
    { time: 5, texts: ['第二行歌词'] },
    { time: 10, texts: ['第三行歌词'] },
    { time: 15, texts: ['第四行歌词'] },
  ]
}

/** 创建测试用曲目 */
function makeTrack(): Track {
  return { path: '/music/test.mp3', name: 'Test', title: 'Test', duration: 60 }
}

/** 初始化所有 mock store/composable */
function setupMocks(
  overrides: {
    lyrics?: LyricLine[]
    loading?: boolean
    currentTrack?: Track | null
    currentTime?: number
    isPlaying?: boolean
    lyricsOffset?: number
    lyricsConfig?: Record<string, unknown>
  } = {},
) {
  mocks.playerStore = reactive({
    currentTrack: overrides.currentTrack ?? null,
    currentTime: overrides.currentTime ?? 0,
    isPlaying: overrides.isPlaying ?? false,
    lyricsOffset: overrides.lyricsOffset ?? 0,
    currentLyricIndex: -1,
    seek: vi.fn(),
    adjustLyricsOffset: vi.fn(),
    resetLyricsOffset: vi.fn(),
  })

  mocks.configStore = reactive({
    lyrics: {
      lyricsStyle: 'modern',
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Roboto',
      enableOnlineFetch: false,
      ...overrides.lyricsConfig,
    },
  })

  const lyricsRef = ref<LyricLine[]>(overrides.lyrics ?? [])
  const loadingRef = ref(overrides.loading ?? false)

  mocks.lyricsState = {
    lyrics: lyricsRef,
    loading: loadingRef,
    lyricsSource: ref('local'),
    fetchAndSaveLyrics: vi.fn(),
  }

  return { lyricsRef, loadingRef }
}

/** 挂载组件，注入 $t mock */
function mountComponent() {
  return mount(LyricsDisplay, {
    global: {
      mocks: {
        $t: (key: string) => key,
      },
    },
  })
}

describe('LyricsDisplay.vue', () => {
  let wrapper: ReturnType<typeof mount>

  beforeEach(() => {
    // mock requestAnimationFrame 防止动画循环运行
    vi.stubGlobal(
      'requestAnimationFrame',
      vi.fn(() => 0),
    )
    vi.stubGlobal('cancelAnimationFrame', vi.fn())
  })

  afterEach(() => {
    if (wrapper) wrapper.unmount()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  // ---------- 空状态 ----------

  describe('空状态', () => {
    it('loading=true 时显示加载中状态', async () => {
      setupMocks({ loading: true })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.loading').exists()).toBe(true)
    })

    it('没有当前曲目时显示空闲状态', async () => {
      setupMocks({ currentTrack: null, loading: false })
      wrapper = mountComponent()
      await nextTick()
      const idle = wrapper.find('.idle-state')
      expect(idle.exists()).toBe(true)
    })

    it('有曲目但无歌词时显示"未找到歌词"', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [], loading: false })
      wrapper = mountComponent()
      await nextTick()
      const noLyrics = wrapper.find('.no-lyrics')
      expect(noLyrics.exists()).toBe(true)
      // 不应包含 idle-state
      expect(wrapper.find('.idle-state').exists()).toBe(false)
    })

    it('有曲目但无歌词时显示获取歌词按钮', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [], loading: false })
      wrapper = mountComponent()
      await nextTick()
      const fetchBtn = wrapper.find('.fetch-lyrics-btn')
      expect(fetchBtn.exists()).toBe(true)
    })

    it('showNoLyricsHint=false 时隐藏"未找到歌词"提示但保留按钮', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: [],
        loading: false,
        lyricsConfig: { showNoLyricsHint: false },
      })
      wrapper = mountComponent()
      await nextTick()
      const noLyrics = wrapper.find('.no-lyrics')
      expect(noLyrics.exists()).toBe(true)
      expect(noLyrics.text()).not.toContain('lyrics.notFound')
      expect(wrapper.find('.fetch-lyrics-btn').exists()).toBe(true)
    })

    it('showFetchLyricsButton=false 时隐藏获取歌词按钮但保留提示', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: [],
        loading: false,
        lyricsConfig: { showFetchLyricsButton: false },
      })
      wrapper = mountComponent()
      await nextTick()
      const noLyrics = wrapper.find('.no-lyrics')
      expect(noLyrics.exists()).toBe(true)
      expect(noLyrics.text()).toContain('lyrics.notFound')
      expect(wrapper.find('.fetch-lyrics-btn').exists()).toBe(false)
    })

    it('两个开关都关闭时整个无歌词区域不渲染', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: [],
        loading: false,
        lyricsConfig: { showNoLyricsHint: false, showFetchLyricsButton: false },
      })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.no-lyrics').exists()).toBe(false)
      expect(wrapper.find('.fetch-lyrics-btn').exists()).toBe(false)
    })

    it('配置缺省（字段为 undefined）时提示与按钮默认显示', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [], loading: false })
      wrapper = mountComponent()
      await nextTick()
      const noLyrics = wrapper.find('.no-lyrics')
      expect(noLyrics.exists()).toBe(true)
      expect(noLyrics.text()).toContain('lyrics.notFound')
      expect(wrapper.find('.fetch-lyrics-btn').exists()).toBe(true)
    })

    it('点击获取歌词按钮调用 fetchAndSaveLyrics', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [], loading: false })
      wrapper = mountComponent()
      await nextTick()
      await wrapper.find('.fetch-lyrics-btn').trigger('click')
      expect(mocks.lyricsState.fetchAndSaveLyrics).toHaveBeenCalledTimes(1)
    })
  })

  // ---------- 歌词列表渲染 ----------

  describe('歌词列表渲染', () => {
    it('渲染所有歌词行', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      const lines = wrapper.findAll('.lyrics')
      expect(lines).toHaveLength(4)
    })

    it('歌词行显示正确文本', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      const lines = wrapper.findAll('.lyrics .first-line')
      expect(lines[0]!.text()).toBe('第一行歌词')
      expect(lines[1]!.text()).toBe('第二行歌词')
      expect(lines[2]!.text()).toBe('第三行歌词')
    })

    it('空歌词列表不渲染歌词行', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [] })
      wrapper = mountComponent()
      await nextTick()
      const lines = wrapper.findAll('.lyrics')
      expect(lines).toHaveLength(0)
    })

    it('有歌词时不显示空状态', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.no-lyrics').exists()).toBe(false)
      expect(wrapper.find('.loading').exists()).toBe(false)
    })
  })

  // ---------- 当前歌词高亮 ----------
  // 组件 onMounted 会根据当前 currentTime 立即计算 activeIndex（覆盖重新挂载场景），
  // 播放中的高亮切换则由 visualTime watcher 驱动。

  describe('当前歌词高亮', () => {
    it('currentTime=2 时高亮第一行 (time=0)', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, currentTime: 0 })
      wrapper = mountComponent()
      await nextTick()

      // 改变 currentTime 触发 watch(currentTime) → 设置 visualTime → 触发 visualTime watcher
      const store = mocks.playerStore as { currentTime: number }
      store.currentTime = 2
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[0]!.classes()).toContain('active')
      expect(lines[1]!.classes()).not.toContain('active')
    })

    it('currentTime=6 时高亮第二行 (time=5)', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, currentTime: 0 })
      wrapper = mountComponent()
      await nextTick()

      const store = mocks.playerStore as { currentTime: number }
      store.currentTime = 6
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[0]!.classes()).not.toContain('active')
      expect(lines[1]!.classes()).toContain('active')
      expect(lines[2]!.classes()).not.toContain('active')
    })

    it('currentTime=12 时高亮第三行 (time=10)', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, currentTime: 0 })
      wrapper = mountComponent()
      await nextTick()

      const store = mocks.playerStore as { currentTime: number }
      store.currentTime = 12
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[1]!.classes()).not.toContain('active')
      expect(lines[2]!.classes()).toContain('active')
      expect(lines[3]!.classes()).not.toContain('active')
    })

    it('currentTime 超过最后一行时高亮最后一行', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, currentTime: 0 })
      wrapper = mountComponent()
      await nextTick()

      const store = mocks.playerStore as { currentTime: number }
      store.currentTime = 100
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[3]!.classes()).toContain('active')
    })

    it('currentTime 在第一行之前时无高亮行', async () => {
      const lyrics: LyricLine[] = [
        { time: 10, texts: ['第一行'] },
        { time: 20, texts: ['第二行'] },
      ]
      setupMocks({ currentTrack: makeTrack(), lyrics, currentTime: 0 })
      wrapper = mountComponent()
      await nextTick()

      const store = mocks.playerStore as { currentTime: number }
      store.currentTime = 5
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[0]!.classes()).not.toContain('active')
      expect(lines[1]!.classes()).not.toContain('active')
    })
  })

  // ---------- 重新挂载恢复高亮 ----------

  describe('重新挂载恢复高亮', () => {
    it('暂停状态下挂载即高亮 currentTime 对应歌词行', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: makeLyrics(),
        loading: false,
        currentTime: 12, // 对应 time=10 的第三行
        isPlaying: false,
      })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[2]!.classes()).toContain('active')
      expect(lines[1]!.classes()).not.toContain('active')
      expect(lines[3]!.classes()).not.toContain('active')
      // 同步到 store
      const store = mocks.playerStore as { currentLyricIndex: number }
      expect(store.currentLyricIndex).toBe(2)
    })

    it('暂停状态下挂载高亮应应用歌词偏移', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: makeLyrics(),
        loading: false,
        currentTime: 12,
        lyricsOffset: 3, // 12 - 3 = 9 → 对应 time=5 的第二行
        isPlaying: false,
      })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[1]!.classes()).toContain('active')
    })

    it('卸载后重新挂载（模拟切换视图返回）仍恢复高亮', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: makeLyrics(),
        loading: false,
        currentTime: 6, // 对应 time=5 的第二行
        isPlaying: false,
      })
      wrapper = mountComponent()
      await nextTick()
      wrapper.unmount()
      wrapper = mountComponent() // 模拟从波形模式切回，组件重新挂载
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[1]!.classes()).toContain('active')
      const store = mocks.playerStore as { currentLyricIndex: number }
      expect(store.currentLyricIndex).toBe(1)
    })

    it('重新挂载时走首帧瞬时定位（无平滑滚动、无延迟跳转）', async () => {
      setupMocks({
        currentTrack: makeTrack(),
        lyrics: makeLyrics(),
        loading: false,
        currentTime: 12,
        isPlaying: false,
      })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()
      const container = wrapper.find('.lyrics-display')
      expect((container.element as HTMLElement).style.scrollBehavior).toBe('auto')
    })

    it('currentTime 在第一行之前时挂载无高亮行', async () => {
      const lyrics: LyricLine[] = [
        { time: 10, texts: ['第一行'] },
        { time: 20, texts: ['第二行'] },
      ]
      setupMocks({
        currentTrack: makeTrack(),
        lyrics,
        loading: false,
        currentTime: 5,
        isPlaying: false,
      })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      expect(lines[0]!.classes()).not.toContain('active')
      expect(lines[1]!.classes()).not.toContain('active')
    })
  })

  // ---------- 歌词偏移调整 ----------

  describe('歌词偏移调整', () => {
    it('显示偏移控制栏（有歌词时）', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.lyrics-offset-control').exists()).toBe(true)
    })

    it('偏移值显示为 "0s"（初始状态）', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, lyricsOffset: 0 })
      wrapper = mountComponent()
      await nextTick()
      const offsetValue = wrapper.find('.offset-value')
      expect(offsetValue.text()).toBe('0s')
    })

    it('正偏移显示为 "+X.Xs"', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, lyricsOffset: 1.5 })
      wrapper = mountComponent()
      await nextTick()
      const offsetValue = wrapper.find('.offset-value')
      expect(offsetValue.text()).toBe('+1.5s')
    })

    it('负偏移显示为 "-X.Xs"', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics, lyricsOffset: -0.5 })
      wrapper = mountComponent()
      await nextTick()
      const offsetValue = wrapper.find('.offset-value')
      expect(offsetValue.text()).toBe('-0.5s')
    })

    it('点击 "+" 按钮调用 adjustLyricsOffset(0.5)', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      // 最后一个 offset-btn 是 "+"
      const buttons = wrapper.findAll('.offset-btn')
      await buttons[1]!.trigger('click')
      const store = mocks.playerStore as { adjustLyricsOffset: ReturnType<typeof vi.fn> }
      expect(store.adjustLyricsOffset).toHaveBeenCalledWith(0.5)
    })

    it('点击 "-" 按钮调用 adjustLyricsOffset(-0.5)', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      // 第一个 offset-btn 是 "-"
      const buttons = wrapper.findAll('.offset-btn')
      await buttons[0]!.trigger('click')
      const store = mocks.playerStore as { adjustLyricsOffset: ReturnType<typeof vi.fn> }
      expect(store.adjustLyricsOffset).toHaveBeenCalledWith(-0.5)
    })

    it('点击偏移值文本调用 resetLyricsOffset', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      await wrapper.find('.offset-value').trigger('click')
      const store = mocks.playerStore as { resetLyricsOffset: ReturnType<typeof vi.fn> }
      expect(store.resetLyricsOffset).toHaveBeenCalledTimes(1)
    })

    it('无歌词时不显示偏移控制栏', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [] })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.lyrics-offset-control').exists()).toBe(false)
    })
  })

  // ---------- 点击歌词跳转 ----------

  describe('点击歌词跳转', () => {
    it('点击歌词行调用 seek 到该行时间', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      // 点击第二行 (time=5)
      await lines[1]!.trigger('click')
      await nextTick()

      const store = mocks.playerStore as { seek: ReturnType<typeof vi.fn> }
      expect(store.seek).toHaveBeenCalledWith(5)
    })

    it('点击歌词行调用 seek 到正确时间', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      await nextTick()

      const lines = wrapper.findAll('.lyrics')
      // 点击第三行 (time=10)
      await lines[2]!.trigger('click')
      await nextTick()

      const store = mocks.playerStore as { seek: ReturnType<typeof vi.fn> }
      expect(store.seek).toHaveBeenCalledWith(10)
    })
  })

  // ---------- 底部控制栏 ----------

  describe('底部控制栏', () => {
    it('有歌词时显示底部控制栏', async () => {
      const lyrics = makeLyrics()
      setupMocks({ currentTrack: makeTrack(), lyrics })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.lyrics-bottom-bar').exists()).toBe(true)
    })

    it('无歌词且无操作按钮时不显示底部控制栏', async () => {
      setupMocks({ currentTrack: makeTrack(), lyrics: [] })
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.lyrics-bottom-bar').exists()).toBe(false)
    })
  })
})
