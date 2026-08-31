import { invoke } from '@tauri-apps/api/core'
import FileUtils from '@/utils/fileUtils'
import logger from '@/utils/logger'
import type { Track } from '@/types'
import type { usePlayerStore } from './player'

/**
 * Player store 的元数据缓存与封面批量加载,从 player.ts 抽离以降低单文件复杂度。
 * 函数接收 store 实例参数,在运行时与 player store 共享同一 Pinia 实例。
 */
type PlayerStore = ReturnType<typeof usePlayerStore>

// 待通知的封面更新 (path -> coverPath):非响应式,由 takeCoverUpdates 一次性取走。
// 放在 store 实例外,避免 Pinia 深度代理整个 Map。
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

  // 创建新的 AbortController，使此次缓存任务可被后续调用取消
  const abortController = new AbortController()

  store._cacheAbortController = abortController

  const cache = store._getMetadataCache()
  const CHUNK_SIZE = 200
  let cached = 0

  for (let i = 0; i < playlist.length; i++) {
    // 检查是否已被取消
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

export async function loadPlaylistCovers(store: PlayerStore, playlist: Track[]): Promise<void> {
  if (!playlist || playlist.length === 0) return

  const metadataCache = store._getMetadataCache()

  // 封面加载后不再依赖响应式 mutation 传播:逐条修改 track.coverPath 仅用于
  // currentTrack 同步与元数据缓存,列表 UI 通过 pendingCoverUpdates +
  // playlistCoverVersion 每批一次增量通知 (PlaylistView 以 O(变更数) 应用)。

  // 批量加载封面路径，每次处理 10 首歌曲
  const BATCH_SIZE = 10
  for (let i = 0; i < playlist.length; i += BATCH_SIZE) {
    const batch = playlist.slice(i, i + BATCH_SIZE)
    let foundInBatch = 0

    // 并行加载这一批的封面
    await Promise.all(
      batch.map(async (track) => {
        if (!track.coverPath) {
          try {
            const coverPath = await invoke<string | null>('get_track_cover_path', {
              path: track.path,
            })
            if (coverPath) {
              track.coverPath = coverPath
              pendingCoverUpdates.set(track.path, coverPath)
              foundInBatch++
              // 同步 currentTrack: playTrack 会创建 resolvedTrack 浅拷贝,
              // 导致 currentTrack 与 playlist 内对象脱钩,
              // 主界面/MiniPlayer 封面基于 currentTrack.coverPath,
              // 不同步会导致切歌时封面丢失
              if (store.currentTrack?.path === track.path) {
                store.currentTrack.coverPath = coverPath
              }
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

    // 本批有新封面时通知一次,列表组件增量应用
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
