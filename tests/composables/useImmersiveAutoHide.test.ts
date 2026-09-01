// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'

const { useImmersiveAutoHide } = await import('@/composables/useImmersiveAutoHide')

// 与源码保持一致的热区常量
const IDLE_DELAY = 3000
const TOP_AREA = 96
const BOTTOM_AREA = 140

let instances: ReturnType<typeof useImmersiveAutoHide>[] = []

const mount = (initial = false) => {
  const immersiveCover = ref(initial)
  const api = useImmersiveAutoHide(immersiveCover)
  instances.push(api)
  return { immersiveCover, ...api }
}

const moveTo = (clientY: number) => window.dispatchEvent(new MouseEvent('mousemove', { clientY }))

const enterImmersive = async (immersiveCover: { value: boolean }) => {
  immersiveCover.value = true
  await nextTick()
}

beforeEach(() => {
  vi.useFakeTimers()
  instances = []
})

afterEach(() => {
  for (const api of instances) api.cleanup()
  instances = []
  vi.useRealTimers()
})

describe('useImmersiveAutoHide', () => {
  it('starts with the controls visible', () => {
    const { immersiveControlsVisible } = mount()
    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('hides the controls after the idle delay', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    vi.advanceTimersByTime(IDLE_DELAY - 1)
    expect(immersiveControlsVisible.value).toBe(true)

    vi.advanceTimersByTime(1)
    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('stays idle until immersive mode is actually entered', () => {
    const { immersiveControlsVisible } = mount(false)

    vi.advanceTimersByTime(IDLE_DELAY * 2)

    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('shows the controls again on user activity', async () => {
    const { immersiveCover, immersiveControlsVisible, markImmersiveActivity } = mount()
    await enterImmersive(immersiveCover)
    vi.advanceTimersByTime(IDLE_DELAY)
    expect(immersiveControlsVisible.value).toBe(false)

    markImmersiveActivity()

    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('keeps the controls visible while the pointer rests in the top hot zone', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    moveTo(TOP_AREA - 10)
    vi.advanceTimersByTime(IDLE_DELAY * 3)

    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('keeps the controls visible while the pointer rests in the bottom hot zone', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    moveTo(window.innerHeight - BOTTOM_AREA + 5)
    vi.advanceTimersByTime(IDLE_DELAY * 3)

    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('hides the controls when the pointer sits in the middle of the window', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    moveTo(window.innerHeight / 2)
    vi.advanceTimersByTime(IDLE_DELAY)

    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('resets the idle timer on every pointer move', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    moveTo(window.innerHeight / 2)
    vi.advanceTimersByTime(IDLE_DELAY - 500)
    moveTo(window.innerHeight / 2)
    vi.advanceTimersByTime(IDLE_DELAY - 500)

    expect(immersiveControlsVisible.value).toBe(true)

    vi.advanceTimersByTime(500)
    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('hides after the pointer leaves the window', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)
    // 先停在下热区,确保离开后不再命中热区
    moveTo(window.innerHeight - BOTTOM_AREA + 5)

    document.dispatchEvent(new MouseEvent('mouseleave'))
    vi.advanceTimersByTime(IDLE_DELAY)

    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('hides immediately when the window loses focus', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)
    moveTo(window.innerHeight / 2)

    window.dispatchEvent(new Event('blur'))

    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('cancels the pending hide timer on blur', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    window.dispatchEvent(new Event('blur'))
    // 若 blur 未清理计时器,这次推进不会改变状态;关键是此后不再有多余的定时器触发
    vi.advanceTimersByTime(IDLE_DELAY)

    expect(immersiveControlsVisible.value).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('restores the controls when the window regains focus', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)
    window.dispatchEvent(new Event('blur'))
    expect(immersiveControlsVisible.value).toBe(false)

    window.dispatchEvent(new Event('focus'))

    expect(immersiveControlsVisible.value).toBe(true)
  })

  it('drops all window listeners when leaving immersive mode', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)

    immersiveCover.value = false
    await nextTick()

    // 退出后不应再有任何待触发的定时器
    expect(vi.getTimerCount()).toBe(0)
    expect(immersiveControlsVisible.value).toBe(true)

    // 事件也不再影响状态
    immersiveControlsVisible.value = false
    window.dispatchEvent(new Event('focus'))
    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('re-arms the idle timer when immersive mode is re-entered', async () => {
    const { immersiveCover, immersiveControlsVisible } = mount()
    await enterImmersive(immersiveCover)
    immersiveCover.value = false
    await nextTick()

    await enterImmersive(immersiveCover)
    vi.advanceTimersByTime(IDLE_DELAY)

    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('cleanup removes the listeners and pending timers', async () => {
    const { immersiveCover, immersiveControlsVisible, cleanup } = mount()
    await enterImmersive(immersiveCover)

    cleanup()
    expect(vi.getTimerCount()).toBe(0)

    immersiveControlsVisible.value = false
    window.dispatchEvent(new Event('focus'))
    expect(immersiveControlsVisible.value).toBe(false)
  })

  it('cleanup is safe to call twice', async () => {
    const { immersiveCover, cleanup } = mount()
    await enterImmersive(immersiveCover)

    cleanup()
    expect(() => cleanup()).not.toThrow()
  })
})
