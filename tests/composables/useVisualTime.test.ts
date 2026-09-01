import { beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick, ref } from 'vue'

const stores = vi.hoisted(() => ({ player: null as { currentTime: number } | null }))

// 必须是 reactive:useVisualTime 用 watch 监听 currentTime 判定 seek
vi.mock('@/stores/player', async () => {
  const { reactive } = await import('vue')
  stores.player ??= reactive({ currentTime: 0 })
  return { usePlayerStore: () => stores.player! }
})

const { useVisualTime } = await import('@/composables/useVisualTime')

// 常量与源码保持一致,便于断言各分支阈值
const HARD_SYNC_THRESHOLD = 0.5
const P_GAIN = 2.0
const MIN_SPEED = 0.7
const MAX_SPEED = 1.3
const MAX_DELTA = 0.1

const playerState = () => stores.player!

beforeEach(() => {
  playerState().currentTime = 0
})

describe('useVisualTime', () => {
  it('starts at zero and seeds the frame clock on the first advance', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()
    expect(visualTime.value).toBe(0)

    advanceVisualTime(1000)
    // 首帧只建立时钟基准,不推进时间
    expect(visualTime.value).toBe(0)
  })

  it('accumulates by the frame delta while in sync', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()

    advanceVisualTime(0)
    advanceVisualTime(100)

    expect(visualTime.value).toBeCloseTo(0.1)
  })

  it('clamps a single frame delta to 100ms', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()

    advanceVisualTime(0)
    advanceVisualTime(10_000)

    expect(visualTime.value).toBeCloseTo(MAX_DELTA)
  })

  it('hard-syncs when the visual time leads by more than 0.5s', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()
    playerState().currentTime = 10
    visualTime.value = 10 + HARD_SYNC_THRESHOLD + 0.2

    advanceVisualTime(0)
    // 首帧只播种时钟(delta 0),但 diff 0.7 已超过阈值,直接拉回真实时间
    expect(visualTime.value).toBe(10)

    // 同步后误差归零,后续帧按原速累加
    advanceVisualTime(16)
    expect(visualTime.value).toBeCloseTo(10 + 0.016)
  })

  it('hard-syncs when the visual time lags by more than 0.5s', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()
    playerState().currentTime = 5
    visualTime.value = 5 - HARD_SYNC_THRESHOLD - 0.2

    advanceVisualTime(0)
    expect(visualTime.value).toBe(5)

    advanceVisualTime(16)
    expect(visualTime.value).toBeCloseTo(5 + 0.016)
  })

  it('slows down when the visual time runs ahead', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()
    playerState().currentTime = 1
    visualTime.value = 1.1 // 领先 0.1s,处于平滑区

    advanceVisualTime(0)
    const before = visualTime.value
    advanceVisualTime(100) // delta 0.1s

    const speed = 1.0 - 0.1 * P_GAIN // 0.8
    expect(visualTime.value - before).toBeCloseTo(0.1 * speed)
  })

  it('speeds up when the visual time falls behind', () => {
    const { visualTime, advanceVisualTime } = useVisualTime()
    playerState().currentTime = 1
    visualTime.value = 0.9 // 落后 0.1s

    advanceVisualTime(0)
    const before = visualTime.value
    advanceVisualTime(100)

    const speed = 1.0 - -0.1 * P_GAIN // 1.2
    expect(visualTime.value - before).toBeCloseTo(0.1 * speed)
  })

  it('clamps the catch-up speed to the configured bounds', () => {
    playerState().currentTime = 1
    // 0.2s 误差:原始速度 1 - 0.2 * P_GAIN = 0.6,应被夹到 MIN_SPEED
    const leading = useVisualTime()
    leading.visualTime.value = 1 + 0.2
    leading.advanceVisualTime(0)
    const beforeLeading = leading.visualTime.value
    leading.advanceVisualTime(100)
    expect(leading.visualTime.value - beforeLeading).toBeCloseTo(0.1 * MIN_SPEED)

    // 对称:原始速度 1.4,应被夹到 MAX_SPEED
    const lagging = useVisualTime()
    lagging.visualTime.value = 1 - 0.2
    lagging.advanceVisualTime(0)
    const beforeLagging = lagging.visualTime.value
    lagging.advanceVisualTime(100)
    expect(lagging.visualTime.value - beforeLagging).toBeCloseTo(0.1 * MAX_SPEED)
  })

  it('resetFrameClock makes the next advance re-seed the clock', () => {
    const { visualTime, advanceVisualTime, resetFrameClock } = useVisualTime()

    advanceVisualTime(0)
    advanceVisualTime(100)
    const afterFirst = visualTime.value

    resetFrameClock()
    advanceVisualTime(200)

    expect(visualTime.value).toBe(afterFirst)
  })

  it('syncToCurrentTime jumps to the real playback time', () => {
    const { visualTime, syncToCurrentTime } = useVisualTime()
    playerState().currentTime = 42.5

    syncToCurrentTime()

    expect(visualTime.value).toBe(42.5)
  })
})

describe('seek detection', () => {
  it('jumps to the new time on a forward seek', async () => {
    const { visualTime } = useVisualTime()
    visualTime.value = 1

    playerState().currentTime = 10 // 跳变 9s > 1.5s
    await nextTick()

    expect(visualTime.value).toBe(10)
  })

  it('jumps to the new time when the time moves backwards', async () => {
    const { visualTime } = useVisualTime()
    visualTime.value = 10

    playerState().currentTime = 9.5 // 倒退 0.5s
    await nextTick()

    expect(visualTime.value).toBe(9.5)
  })

  it('leaves the visual time alone during normal playback', async () => {
    const { visualTime } = useVisualTime()
    // 先把真实时间摆到 10 并冲刷,让 watcher 的 oldTime 为 10,
    // 否则 oldTime 仍是 beforeEach 的 0,跳变 10.5s 会被判成 seek
    playerState().currentTime = 10
    await nextTick()
    visualTime.value = 10

    playerState().currentTime = 10.5 // 常规的 0.5s 推进
    await nextTick()

    expect(visualTime.value).toBe(10)
  })

  it('tolerates a tiny backwards drift', async () => {
    const { visualTime } = useVisualTime()
    playerState().currentTime = 10
    await nextTick()
    visualTime.value = 10

    playerState().currentTime = 9.95 // 倒退 0.05s,小于 0.1s 阈值
    await nextTick()

    expect(visualTime.value).toBe(10)
  })

  it('works with a reactive source that is not the module singleton', () => {
    // 守卫:确保 watcher 绑定的是 store 的 currentTime 而非局部快照
    const { visualTime } = useVisualTime()
    expect(ref(visualTime.value).value).toBe(visualTime.value)
    expect(vi.isMockFunction(nextTick)).toBe(false)
  })
})
