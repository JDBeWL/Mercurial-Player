/** 播放队列同步。Android 上 App 进后台后 WebView 的 JS 会被节流/冻结，`track-ended`
 *  可能无人处理、播完即停，故把排好序的队列同步给 Rust 负责自然结束后的推进，
 *  前端在 Android 上退化为跟随者（监听 `queue-track-changed`）。
 *  桌面端仍由前端推进，但开启智能过渡时也要同步队列：Rust 需要提前知道下一首以预载并交叉淡化。 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { watch, type WatchStopHandle } from 'vue'
import type { TrackSnapshot } from '@/types'
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
    bpm: track.bpm ?? null,
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

/** 同步队列快照到 Rust。
 *
 *  仅 Android 发送：桌面端 Rust 侧队列没有任何消费者（自动推进不开，通知栏/
 *  MediaSession 同步是 Android 专属空实现），整份快照的序列化与 IPC 纯属浪费。
 *  切歌时的下标同步不走这里，由 play_track 命令（note_playback_started）在 Rust 侧完成。 */
let syncSeq = 0

/** 队列同步的可选依赖：桌面端只有在开启智能过渡时才需要把队列交给 Rust（用于预载下一首） */
export interface PlayQueueSyncOptions {
  smartTransitionsEnabled?: () => boolean
}

async function syncPlayQueue(store: PlayerStore, options?: PlayQueueSyncOptions): Promise<void> {
  if (store._isDestroyed) return
  // latest-wins：判平台的 await 期间又触发了新一次同步时，旧请求放弃发送，
  // 避免慢的旧快照后到、把新快照覆盖回去
  const seq = ++syncSeq
  try {
    const android = await isAndroid()
    if (seq !== syncSeq || store._isDestroyed) return
    const smartTransitions = options?.smartTransitionsEnabled?.() ?? false
    if (!android && !smartTransitions) return
    const tracks = orderedTracks(store)
      .map(toSnapshot)
      .filter((t): t is TrackSnapshot => t !== null)
    const currentPath = store.currentTrack?.path
    const index = currentPath ? tracks.findIndex((t) => t.path === currentPath) : -1
    await invoke('set_play_queue', {
      tracks,
      currentIndex: index >= 0 ? index : null,
      repeatMode: store.repeatMode ?? 'none',
      // 桌面端仍然由前端推进；Rust 只把它当"下一首是谁"的数据源
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
        // Rust 侧已经开始播放，前端只更新状态，不能再次 invoke play_track。
        // 走完整应用（封面/歌词/音频信息）而不是只换 currentTrack：智能过渡在旧曲
        // 结束前就切到新曲，UI 必须与真正出声的曲目一致。
        store._applyExternalTrack(track)
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

/** 监听队列结构/顺序变化，自动同步快照到 Rust。
 *  currentTrack 不在源里：每次切歌都重发整份队列没有意义，下标由 play_track 命令
 *  （note_playback_started → sync_index_by_path）在 Rust 侧同步。 */
export function watchPlayQueue(store: PlayerStore, options?: PlayQueueSyncOptions): void {
  if (queueWatchStop) return
  // watch 路径拼接而不是数组本身:playlist 原地变异不改引用,直接 watch 数组不会触发
  queueWatchStop = watch(
    () =>
      [
        store.playlist.map((t) => t.path).join('\n'),
        // 重新随机只换 _shuffleOrder：isShuffle 与列表都不动，必须单独盯住
        store._shuffleOrder.join(','),
        store.repeatMode,
        store.isShuffle,
        // 开关打开时才需要把队列交给 Rust；开关变化也要立刻补发一次
        options?.smartTransitionsEnabled?.() ?? false,
      ] as const,
    () => {
      void syncPlayQueue(store, options)
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
