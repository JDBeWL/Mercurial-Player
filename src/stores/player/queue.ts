/** 播放队列同步。Android 上 App 进后台后 WebView 的 JS 会被节流/冻结，`track-ended`
 *  可能无人处理、播完即停，故把排好序的队列同步给 Rust 负责自然结束后的推进，
 *  前端在 Android 上退化为跟随者（监听 `queue-track-changed`）。桌面端行为零改变。 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { watch, type WatchStopHandle } from 'vue'
import type { Track, TrackSnapshot } from '@/types'
import { getPlatform } from '@/services/appService'
import logger from '@/utils/logger'
import type { usePlayerStore } from './index'

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

/** 计算「最终播放顺序」的曲目数组。Rust 侧不实现 shuffle，避免两端算法分歧，随机模式
 *  只复用 store 已生成的 `_shuffleOrder`；顺序失效时退回列表原顺序。 */
function orderedTracks(store: PlayerStore): TrackSnapshot[] {
  if (!store.isShuffle || store._shuffleOrder.length !== store.playlist.length) {
    return store.playlist
  }
  return store._shuffleOrder.map((i) => store.playlist[i]!).filter(Boolean)
}

/** 同步队列到 Rust（Android 开启自动推进，桌面端关闭） */
async function syncPlayQueue(store: PlayerStore): Promise<void> {
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

/** Rust 的 TrackSnapshot 用 null 表示"未知",前端 Track 用 undefined,这里做一次归一 */
function snapshotToTrack(snapshot: TrackSnapshot): Track {
  return {
    path: snapshot.path,
    name: snapshot.title ?? snapshot.path,
    title: snapshot.title ?? undefined,
    artist: snapshot.artist ?? undefined,
    album: snapshot.album ?? undefined,
    duration: snapshot.duration ?? undefined,
    bitrate: snapshot.bitrate ?? undefined,
    sampleRate: snapshot.sampleRate ?? undefined,
    channels: snapshot.channels ?? undefined,
    bitDepth: snapshot.bitDepth ?? undefined,
    format: snapshot.format ?? undefined,
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
          store.currentTrack = snapshotToTrack(track)
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

/** 回到前台时重新同步播放状态,否则 UI 会停在后台前的旧曲目与进度(Android 冻结见文件头) */
export async function setupStateSyncListener(store: PlayerStore): Promise<UnlistenFn | null> {
  // `_setupListeners` 可重入，先注销上一个再建新的，否则旧句柄被覆盖后再也拿不到
  stateSyncUnlisten?.()
  stateSyncUnlisten = null
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
  // watch 路径拼接而不是数组本身:playlist 原地变异不改引用,直接 watch 数组不会触发
  queueWatchStop = watch(
    () =>
      [
        store.playlist.map((t) => t.path).join('\n'),
        store.currentTrack?.path,
        store.repeatMode,
        store.isShuffle,
      ] as const,
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

/** 后台心跳探针(仅 Android,桌面端不启动):固定间隔调 Rust 命令打日志，
 *  在 logcat 看它是否停止即可判断 App 进后台后 JS 还在不被调度。 */
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
