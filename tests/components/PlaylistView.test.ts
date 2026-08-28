// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { reactive, nextTick } from 'vue'
import PlaylistView from '@/components/PlaylistView.vue'
import type { Track } from '@/types'

// 使用 vi.hoisted 创建可在 mock 工厂中引用的可变引用
const mocks = vi.hoisted(() => ({
  playerStore: null as unknown,
}))

vi.mock('@/stores/player', () => ({
  usePlayerStore: () => mocks.playerStore,
}))

// mock config store
vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({
    titleExtraction: { hideFileExtension: true },
    general: { showQueueInfo: true },
  }),
}))

// mock vue-i18n（组件 setup 中调用 useI18n 获取 t 函数）
vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string, params?: Record<string, number | string>) => {
      if (!params) return key
      return Object.entries(params).reduce(
        (text, [name, value]) => text.replace(`{${name}}`, String(value)),
        key
      )
    },
  }),
}))

// mock pinia 的 storeToRefs，将 store 属性转为 ref
// 组件使用 storeToRefs 获取 playlist、currentTrack 和 currentTrackIndex，需要返回响应式 ref
vi.mock('pinia', async () => {
  const { toRef } = await import('vue')
  return {
    storeToRefs: (store: Record<string, unknown>) => ({
      playlist: toRef(store, 'playlist'),
      currentTrack: toRef(store, 'currentTrack'),
      currentTrackIndex: toRef(store, 'currentTrackIndex'),
    }),
  }
})

// 重新 mock @tauri-apps/api/core，添加 convertFileSrc（setup.ts 只 mock 了 invoke）
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `mock://cover/${path}`),
}))

// mock FileUtils
vi.mock('@/utils/fileUtils', () => ({
  default: {
    getFileName: vi.fn((path: string) => path.split(/[\\/]/).pop() || path),
    getFileNameWithoutExtension: vi.fn((path: string) => {
      const name = path.split(/[\\/]/).pop() || path
      const lastDot = name.lastIndexOf('.')
      return lastDot > 0 ? name.substring(0, lastDot) : name
    }),
    getTrackDisplayName: vi.fn((track: { displayTitle?: string; title?: string; name?: string; path: string }, hideExt: boolean) => {
      if (track.displayTitle) return track.displayTitle
      if (track.title) return track.title
      const name = track.name || (path => path.split(/[\\/]/).pop() || path)(track.path)
      if (!hideExt) return name
      const lastDot = name.lastIndexOf('.')
      return lastDot > 0 ? name.substring(0, lastDot) : name
    }),
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

/** 创建测试用曲目列表 */
function makeTracks(n: number): Track[] {
  return Array.from({ length: n }, (_, i) => ({
    path: `/music/track${i}.mp3`,
    name: `Track${i}`,
    title: `Track ${i}`,
    artist: `Artist ${i}`,
    duration: 180,
  }))
}

/** 创建带封面的曲目 */
function makeTrackWithCover(path: string, title: string): Track {
  return {
    path,
    title,
    artist: 'Test Artist',
    coverPath: '/covers/test.png',
    duration: 200,
  }
}

/** 创建 mock player store */
function createMockStore(overrides: Record<string, unknown> = {}) {
  return reactive({
    playlist: [] as Track[],
    currentTrack: null as Track | null,
    currentTrackIndex: -1,
    isPlaying: false,
    playlistCoverVersion: 0,
    takeCoverUpdates: vi.fn(() => new Map<string, string>()),
    recordCoverUpdate: vi.fn(),
    playTrack: vi.fn(),
    pause: vi.fn(),
    resume: vi.fn(),
    removeTrack: vi.fn(),
    ...overrides,
  })
}

/** 挂载组件，注入 $t mock */
function mountComponent() {
  return mount(PlaylistView, {
    global: {
      mocks: {
        $t: (key: string) => key,
      },
    },
  })
}

describe('PlaylistView.vue', () => {
  let wrapper: ReturnType<typeof mount>
  let store: ReturnType<typeof createMockStore>

  beforeEach(() => {
    vi.useFakeTimers()
    store = createMockStore()
    mocks.playerStore = store
  })

  afterEach(() => {
    if (wrapper) wrapper.unmount()
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  // ---------- 播放列表渲染 ----------

  describe('播放列表渲染', () => {
    it('空播放列表显示空状态', async () => {
      store.playlist = []
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.playlist-empty').exists()).toBe(true)
      expect(wrapper.find('.empty-state').exists()).toBe(true)
    })

    it('空播放列表不渲染歌曲列表', async () => {
      store.playlist = []
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.playlist-songs').exists()).toBe(false)
    })

    it('有曲目时渲染列表项', async () => {
      store.playlist = makeTracks(3)
      wrapper = mountComponent()
      await nextTick()
      const items = wrapper.findAll('.list-item')
      expect(items).toHaveLength(3)
    })

    it('列表项显示曲目标题', async () => {
      store.playlist = makeTracks(2)
      wrapper = mountComponent()
      await nextTick()
      const headlines = wrapper.findAll('.list-item-headline')
      expect(headlines[0].text()).toBe('Track 0')
      expect(headlines[1].text()).toBe('Track 1')
    })

    it('列表项显示艺术家', async () => {
      store.playlist = makeTracks(2)
      wrapper = mountComponent()
      await nextTick()
      const supporting = wrapper.findAll('.list-item-supporting')
      expect(supporting[0].text()).toBe('Artist 0')
      expect(supporting[1].text()).toBe('Artist 1')
    })

    it('标题缺失时回退到文件名', async () => {
      store.playlist = [{ path: '/music/my-song.mp3', duration: 100 }]
      wrapper = mountComponent()
      await nextTick()
      const headline = wrapper.find('.list-item-headline')
      // hideFileExtension 默认为 true,文件名不含扩展名
      expect(headline.text()).toBe('my-song')
    })

    it('有封面时显示封面图片', async () => {
      const track = makeTrackWithCover('/music/test.mp3', 'Test Song')
      store.playlist = [track]
      wrapper = mountComponent()
      await nextTick()
      const cover = wrapper.find('.track-cover')
      expect(cover.exists()).toBe(true)
      const img = cover.find('img')
      expect(img.exists()).toBe(true)
      expect(img.attributes('src')).toBe('mock://cover//covers/test.png')
    })

    it('无封面时显示占位符', async () => {
      store.playlist = [{ path: '/music/test.mp3', title: 'Test', duration: 100 }]
      wrapper = mountComponent()
      await nextTick()
      const placeholder = wrapper.find('.track-cover-placeholder')
      expect(placeholder.exists()).toBe(true)
      expect(placeholder.find('.material-symbols-rounded').text()).toBe('album')
    })
  })

  // ---------- 当前曲目高亮 ----------

  describe('当前曲目高亮', () => {
    it('当前播放曲目有 selected 类', async () => {
      const tracks = makeTracks(3)
      store.playlist = tracks
      store.currentTrack = tracks[1]
      wrapper = mountComponent()
      await nextTick()
      const items = wrapper.findAll('.list-item')
      expect(items[0].classes()).not.toContain('selected')
      expect(items[1].classes()).toContain('selected')
      expect(items[2].classes()).not.toContain('selected')
    })

    it('无当前曲目时无 selected 项', async () => {
      store.playlist = makeTracks(3)
      store.currentTrack = null
      wrapper = mountComponent()
      await nextTick()
      const items = wrapper.findAll('.list-item')
      items.forEach((item) => {
        expect(item.classes()).not.toContain('selected')
      })
    })
  })

  // ---------- 播放控制 ----------

  describe('播放控制', () => {
    it('点击列表项调用 playTrack', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      wrapper = mountComponent()
      await nextTick()
      const items = wrapper.findAll('.list-item')
      await items[1].trigger('click')
      // 组件传入的是 ProcessedTrack（含 cachedTitle 等额外字段），用 path 匹配
      expect(store.playTrack).toHaveBeenCalledWith(
        expect.objectContaining({ path: tracks[1].path }),
      )
    })

    it('点击当前暂停曲目的列表项调用 resume', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      store.currentTrack = tracks[0]
      store.isPlaying = false
      wrapper = mountComponent()
      await nextTick()
      const items = wrapper.findAll('.list-item')
      await items[0].trigger('click')
      // 当前曲目且未播放 → resume
      expect(store.resume).toHaveBeenCalledTimes(1)
      expect(store.playTrack).not.toHaveBeenCalled()
    })

    it('点击播放按钮调用 playTrack', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      wrapper = mountComponent()
      await nextTick()
      const playButtons = wrapper.findAll('.play-button')
      await playButtons[1].trigger('click')
      // 组件传入的是 ProcessedTrack（含 cachedTitle 等额外字段），用 path 匹配
      expect(store.playTrack).toHaveBeenCalledWith(
        expect.objectContaining({ path: tracks[1].path }),
      )
    })

    it('当前曲目播放中显示暂停按钮', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      store.currentTrack = tracks[0]
      store.isPlaying = true
      wrapper = mountComponent()
      await nextTick()
      // 第一项应有暂停按钮
      const pauseButtons = wrapper.findAll('.pause-button')
      expect(pauseButtons).toHaveLength(1)
    })

    it('点击暂停按钮调用 pause', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      store.currentTrack = tracks[0]
      store.isPlaying = true
      wrapper = mountComponent()
      await nextTick()
      const pauseButton = wrapper.find('.pause-button')
      await pauseButton.trigger('click')
      expect(store.pause).toHaveBeenCalledTimes(1)
    })

    it('非当前曲目不显示暂停按钮', async () => {
      const tracks = makeTracks(3)
      store.playlist = tracks
      store.currentTrack = tracks[0]
      store.isPlaying = true
      wrapper = mountComponent()
      await nextTick()
      const pauseButtons = wrapper.findAll('.pause-button')
      expect(pauseButtons).toHaveLength(1)
    })
  })

  // ---------- 删除曲目 ----------

  describe('删除曲目', () => {
    it('每项都有删除按钮', async () => {
      store.playlist = makeTracks(3)
      wrapper = mountComponent()
      await nextTick()
      const removeButtons = wrapper.findAll('.remove-button')
      expect(removeButtons).toHaveLength(3)
    })

    it('点击删除按钮调用 removeTrack', async () => {
      const tracks = makeTracks(3)
      store.playlist = tracks
      wrapper = mountComponent()
      await nextTick()
      const removeButtons = wrapper.findAll('.remove-button')
      await removeButtons[1].trigger('click')
      expect(store.removeTrack).toHaveBeenCalledWith(tracks[1].path)
    })

    it('删除按钮点击不触发列表项点击（stop 修饰符）', async () => {
      const tracks = makeTracks(2)
      store.playlist = tracks
      wrapper = mountComponent()
      await nextTick()
      const removeButton = wrapper.findAll('.remove-button')[0]
      await removeButton.trigger('click')
      // removeTrack 被调用，但 playTrack 不应被调用
      expect(store.removeTrack).toHaveBeenCalledTimes(1)
      expect(store.playTrack).not.toHaveBeenCalled()
    })
  })

  // ---------- 关闭按钮 ----------

  describe('关闭按钮', () => {
    it('点击关闭按钮触发 close 事件', async () => {
      store.playlist = []
      wrapper = mountComponent()
      await nextTick()
      // 头部的关闭按钮（icon-button）
      const closeBtn = wrapper.find('.playlist-header .icon-button')
      await closeBtn.trigger('click')
      expect(wrapper.emitted('close')).toBeTruthy()
      expect(wrapper.emitted('close')).toHaveLength(1)
    })
  })

  // ---------- 标题右侧队列信息 ----------

  describe('队列信息', () => {
    it('有曲目时标题右侧显示队列信息（含总时长）', async () => {
      store.playlist = makeTracks(3) // 3 首 × 180s = 9:00
      store.currentTrackIndex = 1
      wrapper = mountComponent()
      await nextTick()
      const info = wrapper.find('.playlist-queue-info')
      expect(info.exists()).toBe(true)
      // t mock 返回 key 本身，后缀为格式化的总时长
      expect(info.text()).toBe('player.queueInfo · 9:00')
    })

    it('空播放列表时不显示队列信息', async () => {
      store.playlist = []
      wrapper = mountComponent()
      await nextTick()
      expect(wrapper.find('.playlist-queue-info').exists()).toBe(false)
    })
  })

  // ---------- 滚动状态 ----------

  describe('滚动状态', () => {
    it('滚动时添加 is-scrolling 类', async () => {
      store.playlist = makeTracks(5)
      wrapper = mountComponent()
      await nextTick()
      const content = wrapper.find('.playlist-content')
      expect(content.classes()).not.toContain('is-scrolling')

      await content.trigger('scroll')
      expect(content.classes()).toContain('is-scrolling')
    })

    it('停止滚动后 is-scrolling 类移除', async () => {
      store.playlist = makeTracks(5)
      wrapper = mountComponent()
      await nextTick()
      const content = wrapper.find('.playlist-content')

      await content.trigger('scroll')
      expect(content.classes()).toContain('is-scrolling')

      // 快进超过 scrollTimeout (150ms)
      vi.advanceTimersByTime(200)
      await nextTick()
      expect(content.classes()).not.toContain('is-scrolling')
    })
  })

  // ---------- 注意：未实现的功能 ----------
  // 任务描述中提到的"双击播放"、"右键菜单"、"搜索过滤"功能在当前组件源码中未实现：
  // - 组件使用单击播放 (@click="playTrack(track)")，而非双击
  // - 没有 @contextmenu 事件处理，无右键菜单
  // - 没有搜索输入框和过滤逻辑
  // 因此这些测试用例被跳过。
})
