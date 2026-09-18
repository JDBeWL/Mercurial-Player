// 播放统计插件的统计口径测试:
// 覆盖计次门槛、完播判定、分段提交、范围统计、每日序列与旧数据迁移
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { playCountPlugin } from '@/plugins/builtins/playCount'
import type { PluginAPI } from '@/plugins/pluginTypes'
import type { Track } from '@/types'

const POLL_MS = 3000
const NOMINAL_NOW = '2026-09-17T10:00:00'

interface FakeState {
  currentTrack: Track | null
  isPlaying: boolean
  currentTime: number
  duration: number
}

interface PluginExposed {
  activate(): Promise<void>
  deactivate(): void
  getStats(rangeDays?: number | null): {
    totalTracks: number
    totalPlays: number
    totalPlayTime: number
    completedPlays: number
    judgedPlays: number
    completionRate: number | null
    averageSecondsFormatted: string
    activeDays: number
    currentStreak: number
    longestStreak: number
    todayPlays: number
    topArtist: { name: string; plays: number } | null
    detailTruncated: boolean
  }
  getMostPlayed(query?: { limit?: number; sortBy?: string; days?: number | null }): Array<{
    path: string
    title: string
    artist: string
    plays: number
    seconds: number
    completed: number
    judged: number
    completionRate: number | null
  }>
  getPlayHistory(limit?: number): Array<{
    path: string
    title: string
    artist: string
    timestamp: number
    listenedSeconds: number
    completed: boolean
    counted: boolean
  }>
  getDailySeries(
    days?: number,
  ): Array<{ date: string; plays: number; seconds: number; completed: number }>
  clearAllData(): void
}

const createHarness = (initialStorage: Record<string, unknown> = {}) => {
  const storageData: Record<string, unknown> = { ...initialStorage }
  const listeners: Record<string, Set<(data: unknown) => void>> = {}
  const state: FakeState = { currentTrack: null, isPlaying: false, currentTime: 0, duration: 0 }

  const api = {
    storage: {
      get(key: string, fallback: unknown = null): unknown {
        const value = storageData[key]
        return value === undefined ? fallback : value
      },
      set(key: string, value: unknown): void {
        storageData[key] = value
      },
      remove(key: string): void {
        delete storageData[key]
      },
    },
    player: {
      getState: () => ({
        currentTrack: state.currentTrack ? { ...state.currentTrack } : null,
        isPlaying: state.isPlaying,
        currentTime: state.currentTime,
        duration: state.duration,
        volume: 1,
        repeatMode: 'off',
        isShuffle: false,
      }),
    },
    events: {
      on(event: string, callback: (data: unknown) => void): void {
        const bucket = listeners[event] ?? new Set<(data: unknown) => void>()
        bucket.add(callback)
        listeners[event] = bucket
      },
      off(event: string, callback: (data: unknown) => void): void {
        listeners[event]?.delete(callback)
      },
    },
    log: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
  }

  const instance = playCountPlugin.main(api as unknown as PluginAPI) as unknown as PluginExposed
  return { api, storageData, state, instance, listeners }
}

type Harness = ReturnType<typeof createHarness>

const makeTrack = (path: string, duration: number, title = path, artist = 'Artist'): Track => ({
  path,
  title,
  artist,
  duration,
})

const poll = async (): Promise<void> => {
  await vi.advanceTimersByTimeAsync(POLL_MS)
}

/** 触发插件订阅的事件(真实应用里切歌/暂停会立刻派发,不被轮询间隔推迟) */
const fireEvent = async (harness: Harness, event: string): Promise<void> => {
  for (const callback of harness.listeners[event] ?? []) {
    callback({})
  }
  // pollPlayerState 内部同步读取状态,冲一次微任务即可完成结算
  await Promise.resolve()
}

/** 切换到指定曲目并开始播放(与 pluginManager 的 watcher 一致,切歌事件即时派发) */
const startTrack = async (harness: Harness, track: Track, duration: number): Promise<void> => {
  harness.state.currentTrack = track
  harness.state.duration = duration
  harness.state.currentTime = 0
  harness.state.isPlaying = true
  await fireEvent(harness, 'player:trackChanged')
}

/** 从当前位置继续播放 seconds 秒(位置随之推进,不触发回退判定) */
const playSeconds = async (harness: Harness, seconds: number): Promise<void> => {
  let elapsed = 0
  while (elapsed < seconds) {
    elapsed = Math.min(elapsed + POLL_MS / 1000, seconds)
    harness.state.currentTime = Math.min(elapsed, harness.state.duration)
    await poll()
  }
}

/** 停止播放:与真实链路一致,暂停会立即派发状态事件 */
const stopPlayback = async (harness: Harness): Promise<void> => {
  harness.state.isPlaying = false
  await fireEvent(harness, 'player:stateChanged')
}

describe('playCount 插件统计口径', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(NOMINAL_NOW))
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('完整听完一首歌:计一次播放并记为完播', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/a.mp3', 180, 'Song A'), 180)
    await playSeconds(harness, 180)
    await stopPlayback(harness)

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(1)
    expect(stats.totalTracks).toBe(1)
    expect(stats.completedPlays).toBe(1)
    expect(stats.judgedPlays).toBe(1)
    expect(stats.completionRate).toBe(1)
    expect(stats.totalPlayTime).toBeCloseTo(180, 1)

    const history = harness.instance.getPlayHistory(10)
    expect(history).toHaveLength(1)
    expect(history[0]?.counted).toBe(true)
    expect(history[0]?.completed).toBe(true)
    expect(history[0]?.title).toBe('Song A')
    expect(history[0]?.listenedSeconds).toBeCloseTo(180, 1)
  })

  it('试听不足 30 秒:不计播放次数,但计入收听时长', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/skip.mp3', 240, 'Skipped'), 240)
    await playSeconds(harness, 12)
    // 切歌触发上一首结算
    await startTrack(harness, makeTrack('/music/next.mp3', 240, 'Next'), 240)

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(0)
    expect(stats.totalTracks).toBe(0)
    expect(stats.completedPlays).toBe(0)
    expect(stats.completionRate).toBeNull()
    expect(stats.averageSecondsFormatted).toBe('--')
    expect(stats.totalPlayTime).toBeCloseTo(12, 1)

    const history = harness.instance.getPlayHistory(10)
    expect(history[0]?.counted).toBe(false)
    expect(history[0]?.completed).toBe(false)
  })

  it('短于 30 秒的曲目完整听完也应计次并完播', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/jingle.mp3', 20, 'Jingle'), 20)
    await playSeconds(harness, 21)
    await stopPlayback(harness)

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(1)
    expect(stats.completedPlays).toBe(1)
    expect(stats.completionRate).toBe(1)
  })

  it('播放位置回退视为新一轮播放,完播率按可判定次数计算', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/loop.mp3', 200, 'Loop'), 200)
    await playSeconds(harness, 60)

    // 位置回到开头(单曲循环或手动重播)
    harness.state.currentTime = 0
    await poll()
    await playSeconds(harness, 200)
    await stopPlayback(harness)

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(2)
    expect(stats.completedPlays).toBe(1)
    expect(stats.judgedPlays).toBe(2)
    expect(stats.completionRate).toBe(0.5)
  })

  it('长时间播放会分段落盘,不依赖切歌', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/long.mp3', 900, 'Long'), 900)
    await playSeconds(harness, 360)

    // 未切歌,但已满一个 5 分钟检查点,收听时长应已持久化
    expect(Number(harness.storageData.totalPlayTime ?? 0)).toBeGreaterThanOrEqual(300)

    await stopPlayback(harness)
    const paused = harness.instance.getStats()
    // 本次收听尚未结束(没播完也没切歌),时长照常计入,播放次数等到会话归档时才结算
    expect(paused.totalPlayTime).toBeCloseTo(360, 1)
    expect(paused.totalPlays).toBe(0)

    // 切歌即归档:计一次但未达 90%,因此不计完播
    await startTrack(harness, makeTrack('/music/next.mp3', 200, 'Next'), 200)
    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(1)
    expect(stats.completedPlays).toBe(0)
    expect(stats.completionRate).toBe(0)
  })

  it('范围统计与趋势序列按天聚合', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/today.mp3', 120, 'Today'), 120)
    await playSeconds(harness, 120)
    await stopPlayback(harness)

    const all = harness.instance.getStats()
    const last7 = harness.instance.getStats(7)
    const last1 = harness.instance.getStats(1)

    expect(all.totalPlays).toBe(1)
    expect(last7.totalPlays).toBe(1)
    expect(last1.totalPlays).toBe(1)
    expect(last1.completionRate).toBe(1)
    expect(all.detailTruncated).toBe(false)

    const series = harness.instance.getDailySeries(3)
    expect(series).toHaveLength(3)
    expect(series[2]?.plays).toBe(1)
    expect(series[2]?.seconds).toBeCloseTo(120, 1)
    expect(series[0]?.plays).toBe(0)
    expect(all.activeDays).toBe(1)
    expect(all.currentStreak).toBe(1)
    expect(all.longestStreak).toBe(1)
  })

  it('榜单支持按次数/时长排序并带出完播率', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    // A 听两轮,每轮 100 秒(共 200 秒),B 完整听完一次 60 秒
    await startTrack(harness, makeTrack('/music/a.mp3', 200, 'A', 'Artist A'), 200)
    await playSeconds(harness, 100)
    harness.state.currentTime = 0
    await poll()
    await playSeconds(harness, 200)
    await stopPlayback(harness)

    await startTrack(harness, makeTrack('/music/b.mp3', 60, 'B', 'Artist B'), 60)
    await playSeconds(harness, 60)
    await stopPlayback(harness)

    const byPlays = harness.instance.getMostPlayed({ limit: 10, sortBy: 'plays' })
    expect(byPlays[0]?.path).toBe('/music/a.mp3')
    expect(byPlays[0]?.plays).toBe(2)
    expect(byPlays[0]?.completed).toBe(1)
    expect(byPlays[0]?.completionRate).toBe(0.5)
    expect(byPlays[1]?.path).toBe('/music/b.mp3')

    const bySeconds = harness.instance.getMostPlayed({ limit: 10, sortBy: 'seconds' })
    expect(bySeconds[0]?.seconds).toBeGreaterThanOrEqual(bySeconds[1]?.seconds ?? 0)

    // 范围查询走播放明细聚合
    const ranged = harness.instance.getMostPlayed({ limit: 10, days: 7 })
    expect(ranged.find((item) => item.path === '/music/a.mp3')?.plays).toBe(2)

    const stats = harness.instance.getStats()
    expect(stats.topArtist?.name).toBe('Artist A')
  })

  it('旧版数据自动迁移且不会被算成 0% 完播率', async () => {
    const legacyTimestamp = new Date('2026-09-16T12:00:00').getTime()
    const harness = createHarness({
      playCounts: { '/music/old.mp3': 5 },
      playHistory: [
        {
          path: '/music/old.mp3',
          title: 'Old Song',
          artist: 'Old Artist',
          timestamp: legacyTimestamp,
        },
      ],
      totalPlayTime: 3600,
    })

    await harness.instance.activate()

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(5)
    expect(stats.totalTracks).toBe(1)
    expect(stats.totalPlayTime).toBe(3600)
    // 旧数据没有完播信息,分母为 0,应显示为"暂无判定样本"而不是 0%
    expect(stats.judgedPlays).toBe(0)
    expect(stats.completionRate).toBeNull()

    const top = harness.instance.getMostPlayed({ limit: 5 })
    expect(top[0]?.title).toBe('Old Song')
    expect(top[0]?.artist).toBe('Old Artist')
    expect(top[0]?.completionRate).toBeNull()

    // 迁移结果落盘,旧键被清理
    expect(harness.storageData.version).toBe(2)
    expect(harness.storageData.playCounts).toBeUndefined()
    expect(harness.storageData.playHistory).toBeUndefined()
    const daily = harness.storageData.daily as Record<string, { plays: number }>
    expect(daily['2026-09-16']?.plays).toBe(1)
  })

  it('清除数据后所有统计归零', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/a.mp3', 120, 'A'), 120)
    await playSeconds(harness, 120)
    await stopPlayback(harness)
    expect(harness.instance.getStats().totalPlays).toBe(1)

    harness.instance.clearAllData()

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(0)
    expect(stats.totalPlayTime).toBe(0)
    expect(stats.completionRate).toBeNull()
    expect(harness.instance.getPlayHistory(10)).toEqual([])
    expect(harness.instance.getMostPlayed({ limit: 10 })).toEqual([])
    expect(harness.instance.getDailySeries(3).every((point) => point.plays === 0)).toBe(true)
  })

  it('停用插件时会结算并归档当前播放', async () => {
    const harness = createHarness()
    await harness.instance.activate()

    await startTrack(harness, makeTrack('/music/a.mp3', 300, 'A'), 300)
    await playSeconds(harness, 90)

    harness.instance.deactivate()

    const stats = harness.instance.getStats()
    expect(stats.totalPlays).toBe(1)
    expect(stats.totalPlayTime).toBeCloseTo(90, 1)
    expect(harness.instance.getPlayHistory(10)).toHaveLength(1)
  })
})
