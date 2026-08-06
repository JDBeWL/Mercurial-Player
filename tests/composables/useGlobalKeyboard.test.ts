// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'

// Mock player store
const mockPlayerStore = {
  togglePlay: vi.fn(),
  previousTrack: vi.fn(),
  nextTrack: vi.fn(),
  setVolume: vi.fn(),
  volume: 0.5,
  hasPreviousTrack: true,
  hasNextTrack: true,
}
vi.mock('@/stores/player', () => ({
  usePlayerStore: vi.fn(() => mockPlayerStore),
}))

import { useGlobalKeyboard } from '@/composables/useGlobalKeyboard'

/** 创建并挂载一个使用 useGlobalKeyboard 的测试组件 */
function mountWithKeyboard(): VueWrapper {
  const TestComponent = defineComponent({
    setup() {
      useGlobalKeyboard()
      return () => h('div', { id: 'test-root' })
    },
  })
  return mount(TestComponent, { attachTo: document.body })
}

/** 派发 keydown 事件 */
function dispatchKey(code: string): KeyboardEvent {
  const event = new KeyboardEvent('keydown', { code, bubbles: true })
  vi.spyOn(event, 'preventDefault')
  document.dispatchEvent(event)
  return event
}

describe('useGlobalKeyboard', () => {
  let wrapper: VueWrapper | undefined

  beforeEach(() => {
    vi.clearAllMocks()
    mockPlayerStore.volume = 0.5
    mockPlayerStore.hasPreviousTrack = true
    mockPlayerStore.hasNextTrack = true
    // 重置 DOM：清空 body 并创建一个可聚焦的 div 作为 activeElement
    document.body.innerHTML = '<div tabindex="0" id="focus-target"></div>'
    const focusTarget = document.getElementById('focus-target') as HTMLElement | null
    focusTarget?.focus?.()
  })

  afterEach(() => {
    wrapper?.unmount()
    wrapper = undefined
    vi.clearAllMocks()
    document.body.innerHTML = ''
  })

  // ---------- 空格键 ----------

  describe('Space 键', () => {
    it('空格键触发 togglePlay', () => {
      wrapper = mountWithKeyboard()
      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).toHaveBeenCalledTimes(1)
    })

    it('空格键调用 preventDefault', () => {
      wrapper = mountWithKeyboard()
      const event = dispatchKey('Space')
      expect(event.preventDefault).toHaveBeenCalled()
    })
  })

  // ---------- 方向键：上一首/下一首 ----------

  describe('ArrowLeft / ArrowRight', () => {
    it('左方向键触发 previousTrack', () => {
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowLeft')
      expect(mockPlayerStore.previousTrack).toHaveBeenCalledTimes(1)
    })

    it('右方向键触发 nextTrack', () => {
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowRight')
      expect(mockPlayerStore.nextTrack).toHaveBeenCalledTimes(1)
    })

    it('hasPreviousTrack 为 false 时不触发 previousTrack', () => {
      mockPlayerStore.hasPreviousTrack = false
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowLeft')
      expect(mockPlayerStore.previousTrack).not.toHaveBeenCalled()
    })

    it('hasNextTrack 为 false 时不触发 nextTrack', () => {
      mockPlayerStore.hasNextTrack = false
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowRight')
      expect(mockPlayerStore.nextTrack).not.toHaveBeenCalled()
    })
  })

  // ---------- 方向键：音量 ----------

  describe('ArrowUp / ArrowDown 音量控制', () => {
    it('上方向键增加音量 (0.5 + 0.05 = 0.55)', () => {
      mockPlayerStore.volume = 0.5
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowUp')
      expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0.55)
    })

    it('下方向键减少音量 (0.5 - 0.05 = 0.45)', () => {
      mockPlayerStore.volume = 0.5
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowDown')
      expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0.45)
    })

    it('音量上限为 1 (0.98 + 0.05 → 1)', () => {
      mockPlayerStore.volume = 0.98
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowUp')
      expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(1)
    })

    it('音量下限为 0 (0.02 - 0.05 → 0)', () => {
      mockPlayerStore.volume = 0.02
      wrapper = mountWithKeyboard()
      dispatchKey('ArrowDown')
      expect(mockPlayerStore.setVolume).toHaveBeenCalledWith(0)
    })
  })

  // ---------- 焦点在 INPUT/TEXTAREA 时跳过 ----------

  describe('焦点在输入框时跳过', () => {
    it('焦点在 INPUT 时不触发任何操作', () => {
      wrapper = mountWithKeyboard()
      // 创建 input 并聚焦
      const input = document.createElement('input')
      document.body.appendChild(input)
      input.focus()

      dispatchKey('Space')
      dispatchKey('ArrowLeft')
      dispatchKey('ArrowRight')
      dispatchKey('ArrowUp')
      dispatchKey('ArrowDown')

      expect(mockPlayerStore.togglePlay).not.toHaveBeenCalled()
      expect(mockPlayerStore.previousTrack).not.toHaveBeenCalled()
      expect(mockPlayerStore.nextTrack).not.toHaveBeenCalled()
      expect(mockPlayerStore.setVolume).not.toHaveBeenCalled()
    })

    it('焦点在 TEXTAREA 时不触发任何操作', () => {
      wrapper = mountWithKeyboard()
      const textarea = document.createElement('textarea')
      document.body.appendChild(textarea)
      textarea.focus()

      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).not.toHaveBeenCalled()
    })

    it('焦点在 contentEditable 元素时不触发任何操作', () => {
      wrapper = mountWithKeyboard()
      const div = document.createElement('div')
      div.contentEditable = 'true'
      document.body.appendChild(div)
      div.focus()

      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).not.toHaveBeenCalled()
    })

    it('从输入框移出焦点后恢复正常响应', () => {
      wrapper = mountWithKeyboard()
      const input = document.createElement('input')
      document.body.appendChild(input)
      input.focus()

      // 输入框聚焦时不响应
      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).not.toHaveBeenCalled()

      // 移回焦点到普通 div
      const focusTarget = document.getElementById('focus-target') as HTMLElement
      focusTarget?.focus?.()

      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).toHaveBeenCalledTimes(1)
    })
  })

  // ---------- 生命周期 ----------

  describe('onMounted / onUnmounted 生命周期', () => {
    it('挂载时注册 keydown 事件监听', () => {
      const spy = vi.spyOn(document, 'addEventListener')
      wrapper = mountWithKeyboard()
      expect(spy).toHaveBeenCalledWith('keydown', expect.any(Function))
      spy.mockRestore()
    })

    it('卸载时移除 keydown 事件监听', () => {
      const spy = vi.spyOn(document, 'removeEventListener')
      wrapper = mountWithKeyboard()
      wrapper.unmount()
      expect(spy).toHaveBeenCalledWith('keydown', expect.any(Function))
      spy.mockRestore()
    })

    it('卸载后键盘事件不再触发操作', async () => {
      wrapper = mountWithKeyboard()
      wrapper.unmount()
      wrapper = undefined

      dispatchKey('Space')
      expect(mockPlayerStore.togglePlay).not.toHaveBeenCalled()
    })
  })
})
