import { invoke } from '@tauri-apps/api/core'
import FileUtils from '@/utils/fileUtils'
import logger from '@/utils/logger'
import type { Track } from '@/types'
import type { usePlayerStore } from './index'

/** Player store 的播放列表元数据批量缓存与封面加载。 */
type PlayerStore = ReturnType<typeof usePlayerStore>

// 待通知的封面更新 (path -> coverPath):放在 store 实例外,避免 Pinia 深度代理整个 Map
const pendingCoverUpdates = new Map<string, string>()

/** 记录一条封面更新 (由 player store 的 recordCoverUpdate action 转发) */
export function recordCoverUpdate(path: string, coverPath: string): void {
  pendingCoverUpdates.set(path, coverPath)
}

/** 取走并清空自上次调用以来累计的封面更新 (由 player store 的 takeCoverUpdates action 转发) */
export function takeCoverUpdates(): Map<string, string> {
  if (pendingCoverUpdates.size === 0) return new Map()
  const taken = new Map(pendingCoverUpdates)
  pendingCoverUpdates.clear()
  return taken
}

export async function cachePlaylistMetadata(store: PlayerStore, playlist: Track[]): Promise<void> {
  if (!playlist || playlist.length === 0) return

  // 每次调用换一个新 controller,后续调用可通过 _cacheAbortController 取消本次缓存
  const abortController = new AbortController()

  store._cacheAbortController = abortController

  const cache = store._getMetadataCache()
  const CHUNK_SIZE = 200
  let cached = 0

  for (let i = 0; i < playlist.length; i++) {
    if (abortController.signal.aborted) {
      logger.debug(`Metadata caching aborted after ${cached} tracks`)
      return
    }

    const track = playlist[i]!
    if (!track.path || cache.has(track.path)) continue

    cache.set(track.path, {
      title: track.displayTitle || track.title || FileUtils.getFileNameWithoutExtension(track.path),
      artist: track.displayArtist || track.artist || '',
      album: track.album || '',
      duration: track.duration || 0,
      coverPath: track.coverPath,
      bitrate: track.bitrate || null,
      sampleRate: track.sampleRate || null,
      channels: track.channels || null,
      bitDepth: track.bitDepth || null,
      format: track.format || null,
    })
    cached++

    if (cached > 0 && cached % CHUNK_SIZE === 0) {
      await new Promise((resolve) => setTimeout(resolve, 0))
    }
  }

  logger.debug(`Cached metadata for ${cached} tracks`)
}

/**
 * 背景批量任务的取消令牌,与 cachePlaylistMetadata 共用同一个 controller:
 * setPlaylist / 恢复会话 / cleanup 处一次 abort 同时停掉元数据缓存与封面加载。
 */
function ensureAbortController(store: PlayerStore): AbortController {
  const existing = store._cacheAbortController
  if (existing && !existing.signal.aborted) return existing
  const controller = new AbortController()
  store._cacheAbortController = controller
  return controller
}

export async function loadPlaylistCovers(store: PlayerStore, playlist: Track[]): Promise<void> {
  if (!playlist || playlist.length === 0) return

  const metadataCache = store._getMetadataCache()
  const { signal } = ensureAbortController(store)

  // 列表 UI 不靠响应式 mutation 更新,用 pendingCoverUpdates + playlistCoverVersion 每批增量通知
  const BATCH_SIZE = 10
  for (let i = 0; i < playlist.length; i += BATCH_SIZE) {
    if (signal.aborted || store._isDestroyed) {
      logger.debug(`Cover loading aborted before index ${i}`)
      return
    }
    const batch = playlist.slice(i, i + BATCH_SIZE)
    let foundInBatch = 0

    await Promise.all(
      batch.map(async (track) => {
        if (signal.aborted || store._isDestroyed) return
        if (!track.coverPath) {
          try {
            const coverPath = await invoke<string | null>('get_track_cover_path', {
              path: track.path,
            })
            // 回写前再判一次：这一批进行中途可能已经换列表或 cleanup
            if (signal.aborted || store._isDestroyed) return
            if (coverPath) {
              // 先同步当前曲目、再写列表条目,顺序不能反(恢复会话时两者常是同一对象)。
              // playlist 已 markRaw(约束见 index.ts 的 _setPlaylist),就地写不触发渲染,
              // 所以这里整体重新赋值让 currentTrack 属性本身发生变化。
              const current = store.currentTrack
              if (current?.path === track.path && current.coverPath !== coverPath) {
                store.currentTrack = { ...current, coverPath }
              }
              track.coverPath = coverPath
              pendingCoverUpdates.set(track.path, coverPath)
              foundInBatch++
              // 同时更新元数据缓存中的封面路径
              const cachedMetadata = metadataCache.get(track.path)
              if (cachedMetadata) {
                cachedMetadata.coverPath = coverPath
                metadataCache.set(track.path, cachedMetadata)
              }
            }
          } catch (err) {
            logger.debug(`Failed to load cover for ${track.path}:`, err)
          }
        }
      }),
    )

    if (foundInBatch > 0) {
      store.playlistCoverVersion++
    }

    // 让出主线程，避免阻塞 UI
    if (i + BATCH_SIZE < playlist.length) {
      await new Promise((resolve) => setTimeout(resolve, 0))
    }
  }

  logger.debug(`Loaded covers for playlist`)
}
