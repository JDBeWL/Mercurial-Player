/**
 * 播放队列同步（阶段 3.0 · Android 后台播放）
 *
 * 背景：桌面端「播完切下一首」由前端 `_onEnded()` 驱动；Android 上 App 进入
 * 后台后 WebView 的 JS 会被节流/冻结，`track-ended` 可能无人处理，播完即停。
 * 因此这里把「已排好播放顺序」的队列同步给 Rust，由 Rust 负责自然结束后的推进，
 * 前端在 Android 上退化为跟随者（监听 `queue-track-changed`）。
 *
 * 桌面端 `auto_advance` 恒为 false，Rust 侧不会自动推进 —— 行为零改变。
 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { watch, type WatchStopHandle } from 'vue'
import type { TrackSnapshot } from '@/types'
import { getPlatform } from '@/services/appService'
import logger from '@/utils/logger'
import type { usePlayerStore } from './player'

type PlayerStore = ReturnType<typeof usePlayerStore>

let androidPromise: Promise<boolean> | null = null

/** 是否运行在 Android（结果缓存，避免每次切歌都查一遍） */
export function isAndroid(): Promise<boolean> {
  if (!androidPromise) {
    androidPromise = getPlatform()
      .then((p) => p === 'android')
      .catch((err) => {
        logger.warn('Failed to detect platform, assume desktop:', err)
        return false
      })
  }
  return androidPromise
}

/** 把前端 Track 转成 Rust 侧 TrackSnapshot */
function toSnapshot(track: TrackSnapshot | null | undefined): TrackSnapshot | null {
  if (!track?.path) return null
  return {
    path: track.path,
    title: track.title ?? null,
    artist: track.artist ?? null,
    album: track.album ?? null,
    duration: track.duration ?? null,
    bitrate: track.bitrate ?? null,
    sampleRate: track.sampleRate ?? null,
    channels: track.channels ?? null,
    bitDepth: track.bitDepth ?? null,
    format: track.format ?? null,
  }
}

/**
 * 计算「最终播放顺序」的曲目数组
 *
 * 随机模式下复用 store 已生成的 `_shuffleOrder`；顺序失效时按当前列表顺序输出，
 * Rust 侧不实现 shuffle，避免两端算法分歧。
 */
function orderedTracks(store: PlayerStore): TrackSnapshot[] {
  if (!store.isShuffle || store._shuffleOrder.length !== store.playlist.length) {
    return store.playlist
  }
  return store._shuffleOrder.map((i) => store.playlist[i]!).filter(Boolean)
}

/** 同步队列到 Rust（Android 开启自动推进，桌面端关闭） */
export async function syncPlayQueue(store: PlayerStore): Promise<void> {
  if (store._isDestroyed) return
  try {
    const android = await isAndroid()
    const tracks = orderedTracks(store)
      .map(toSnapshot)
      .filter((t): t is TrackSnapshot => t !== null)
    const currentPath = store.currentTrack?.path
    const index = currentPath ? tracks.findIndex((t) => t.path === currentPath) : -1
    await invoke('set_play_queue', {
      tracks,
      currentIndex: index >= 0 ? index : null,
      repeatMode: store.repeatMode ?? 'none',
      autoAdvance: android,
    })
  } catch (err) {
    logger.warn('Failed to sync play queue:', err)
  }
}

/** 监听 Rust 侧推进结果，同步 UI（不重复触发播放） */
export async function setupQueueListener(store: PlayerStore): Promise<UnlistenFn | null> {
  try {
    return await listen<{ index: number | null; track: TrackSnapshot | null; reason: string }>(
      'queue-track-changed',
      (event) => {
        if (store._isDestroyed) return
        const { track, reason } = event.payload ?? {}
        logger.debug(`queue-track-changed: reason=${reason}`)

        if (!track) {
          // 队列到底（非循环模式）
          store.isPlaying = false
          return
        }
        // Rust 侧已经开始播放，前端只更新状态，不能再次 invoke play_track
        const idx = store.playlist.findIndex((t) => t.path === track.path)
        if (idx >= 0) {
          store.currentTrack = { ...store.playlist[idx]! }
        } else {
          store.currentTrack = { ...track, name: track.title ?? track.path } as never
        }
        store.currentTime = 0
        store.duration = track.duration ?? 0
        store.isPlaying = true
      },
    )
  } catch (err) {
    logger.error('Failed to setup queue-track-changed listener:', err)
    return null
  }
}

let stateSyncUnlisten: UnlistenFn | null = null

/**
 * 回到前台时重新同步播放状态
 *
 * 实测（MuMu / Android 15）：App 退到后台约 1 分钟后 WebView 的 JS 会被完全冻结
 * （心跳探针停止、期间发出的事件丢失），因此回到前台必须让 Rust 推一次真实状态，
 * 否则 UI 会停留在后台前的旧曲目与进度。
 */
export async function setupStateSyncListener(store: PlayerStore): Promise<UnlistenFn | null> {
  try {
    stateSyncUnlisten = await listen<{
      index: number | null
      track: TrackSnapshot | null
      playing: boolean
      positionMs: number
    }>('playback-state-sync', (event) => {
      if (store._isDestroyed) return
      const { track, playing, positionMs } = event.payload ?? {}
      logger.debug('playback-state-sync: 回前台重新同步')

      if (track) {
        const idx = store.playlist.findIndex((t) => t.path === track.path)
        if (idx >= 0) {
          store.currentTrack = { ...store.playlist[idx]! }
        }
        store.duration = track.duration ?? store.duration
        // Rust 侧进度是权威值：后台期间前端计时已失效
        store.currentTime = (positionMs ?? 0) / 1000
      }
      store.isPlaying = Boolean(playing)
    })
    return stateSyncUnlisten
  } catch (err) {
    logger.error('Failed to setup playback-state-sync listener:', err)
    return null
  }
}

let queueWatchStop: WatchStopHandle | null = null

/** 监听播放列表/循环/随机变化，自动同步队列到 Rust */
export function watchPlayQueue(store: PlayerStore): void {
  if (queueWatchStop) return
  queueWatchStop = watch(
    () => [store.playlist, store.currentTrack?.path, store.repeatMode, store.isShuffle] as const,
    () => {
      void syncPlayQueue(store)
    },
    { immediate: true },
  )
}

export function stopPlayQueueWatch(): void {
  queueWatchStop?.()
  queueWatchStop = null
  stateSyncUnlisten?.()
  stateSyncUnlisten = null
}

let heartbeatTimer: ReturnType<typeof setInterval> | null = null
let heartbeatSeq = 0

/**
 * 后台心跳探针（阶段 3 实测用）
 *
 * 固定间隔调用 Rust 命令打日志，`adb logcat | grep background-heartbeat`
 * 即可观察 App 进入后台后 JS 是否仍被调度，用于验证「播放推进必须下沉到 Rust」
 * 这一判断。移动端专用，桌面端不会启动。
 */
export async function startBackgroundHeartbeat(intervalMs = 1000): Promise<void> {
  if (heartbeatTimer) return
  if (!(await isAndroid())) return
  heartbeatTimer = setInterval(() => {
    heartbeatSeq += 1
    invoke('background_heartbeat', { seq: heartbeatSeq }).catch(() => {
      /* 探针失败不影响播放 */
    })
  }, intervalMs)
  logger.info('Background heartbeat probe started')
}

export function stopBackgroundHeartbeat(): void {
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer)
    heartbeatTimer = null
  }
}
