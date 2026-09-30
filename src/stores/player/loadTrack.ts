import i18n from '@/i18n'
import { invoke } from '@tauri-apps/api/core'
import type { Track } from '@/types'
import type { usePlayerStore } from './index'
import type { TrackMetadata } from './cache'
import FileUtils from '@/utils/fileUtils'
import logger from '@/utils/logger'
import errorHandler, { ErrorSeverity } from '@/utils/errorHandler'
import { classifyAudioInvokeError } from '@/utils/audioErrorClassifier'

/**
 * 曲目装载：把播放列表里的一条 Track 解析成可播放状态并起播，从 index.ts 的 playTrack 抽离。
 * 四个阶段各自可测：解析路径 → 组装元数据 → 写入 store → 与后端交互起播。
 * playTrack 只保留编排与过期请求（_playRequestId）的守卫。
 */
type PlayerStore = ReturnType<typeof usePlayerStore>

/// play_track IPC 的超时(毫秒),超时视为后端无响应并报错
const PLAY_TRACK_TIMEOUT_MS = 5000
/// 播放失败后自动跳到下一首的延迟(毫秒),给 UI 留出状态刷新窗口
const AUTO_NEXT_TRACK_DELAY_MS = 100

export interface ResolvedTrackPath {
  /** 最终尝试的路径（分隔符互转后的结果，供日志与缓存键使用） */
  path: string
  exists: boolean
}

/** 解析结果:路径已确认存在,元数据与要显示的字段都已补齐 */
export interface PreparedTrack {
  resolvedPath: string
  resolvedTrack: Track
  metadata: TrackMetadata
  /** 是否「同一首歌重新播放」(单曲循环、重复点击当前曲目),决定后面是否重置歌词 */
  isSameTrackReplay: boolean
}

/** 解析曲目实际路径：Windows 与 Android 的分隔符互转后再查一次存在性 */
export async function resolveTrackPath(
  store: PlayerStore,
  track: Track,
): Promise<ResolvedTrackPath> {
  let resolvedPath = track.path
  if (await store._checkFileExists(resolvedPath)) {
    return { path: resolvedPath, exists: true }
  }
  if (!resolvedPath) {
    return { path: resolvedPath, exists: false }
  }

  const altPath = resolvedPath.includes('/')
    ? resolvedPath.replace(/\//g, '\\')
    : resolvedPath.replace(/\\/g, '/')
  if (altPath === resolvedPath) {
    return { path: resolvedPath, exists: false }
  }

  resolvedPath = altPath
  return { path: resolvedPath, exists: await store._checkFileExists(resolvedPath) }
}

/** 元数据兜底并组装出要显示与播放的曲目对象。必须在写入 currentTrack 之前调用 */
export function prepareTrack(
  store: PlayerStore,
  track: Track,
  resolvedPath: string,
): PreparedTrack {
  const metadataCache = store._getMetadataCache()
  const metadata: TrackMetadata = metadataCache.get(resolvedPath) ??
    metadataCache.get(track.path) ?? {
      title: track.title || FileUtils.getFileNameWithoutExtension(resolvedPath),
      artist: track.artist || '',
      album: track.album || '',
      duration: track.duration || 0,
      bitrate: track.bitrate || null,
      sampleRate: track.sampleRate || null,
      channels: track.channels || null,
      bitDepth: track.bitDepth || null,
      format: track.format || null,
    }

  const isSameTrackReplay = store.currentTrack?.path === resolvedPath

  const resolvedTrack: Track = {
    ...track,
    path: resolvedPath,
    title: metadata.title,
    artist: metadata.artist,
    album: metadata.album,
    duration: metadata.duration,
    coverPath: track.coverPath, // 保留原始的 coverPath
  }

  return { resolvedPath, resolvedTrack, metadata, isSameTrackReplay }
}

/** 把解析结果写进 store：当前曲目、shuffle 位置、封面懒加载与界面展示字段 */
export function applyPreparedTrack(store: PlayerStore, prepared: PreparedTrack): void {
  const { resolvedPath, resolvedTrack, metadata, isSameTrackReplay } = prepared
  const metadataCache = store._getMetadataCache()

  store.currentTrack = resolvedTrack

  // shuffle 模式下,用户手动切曲时同步 _shufflePosition 到新曲目在 _shuffleOrder 中的位置
  // 如果新曲目不在 _shuffleOrder 中 (顺序失效/外部触发),则作废顺序,下次 nextTrack 时重新生成
  if (store.isShuffle && store._shuffleOrder.length > 0) {
    const newIdx = store.currentTrackIndex
    const pos = store._shuffleOrder.indexOf(newIdx)
    if (pos >= 0) {
      store._shufflePosition = pos
    } else {
      // 顺序已失效,作废等待下次懒生成
      store._shuffleOrder = []
      store._shufflePosition = -1
    }
  }

  // 按需加载封面路径（如果还没有）
  if (!resolvedTrack.coverPath) {
    logger.debug('Loading cover for track:', resolvedPath)
    invoke<string | null>('get_track_cover_path', { path: resolvedPath })
      .then((coverPath) => {
        logger.debug('Cover path result:', coverPath)
        if (store.currentTrack?.path === resolvedPath && coverPath) {
          store.currentTrack.coverPath = coverPath
          // 同时更新元数据缓存中的封面路径
          const cachedMetadata = metadataCache.get(resolvedPath)
          if (cachedMetadata) {
            cachedMetadata.coverPath = coverPath
            metadataCache.set(resolvedPath, cachedMetadata)
          }
        }
      })
      .catch((err) => logger.error('Failed to load cover path:', err))
  } else {
    logger.debug('Track already has coverPath:', resolvedTrack.coverPath)
  }

  store.duration = metadata.duration || 0
  store.currentTime = 0
  // 同一首歌重新播放时 path 未变,useLyrics 的 watcher 不会重新触发,
  // 在这里清空就再也补不回来(表现为重播后歌词消失)
  if (!isSameTrackReplay) {
    store.lyrics = null
    store.currentLyricIndex = -1
  }
  store.audioInfo = {
    bitrate: metadata.bitrate || null,
    sampleRate: metadata.sampleRate || null,
    channels: metadata.channels || null,
    bitDepth: metadata.bitDepth || null,
    format: metadata.format || null,
  }
}

/**
 * 起播：串行等 pause 完成后再 play_track（带超时），避免 pause 晚于 play 返回把新曲目立即暂停。
 * 失败时上报错误并按需顺延下一首；过期请求（已被新的 playTrack 取代）直接放弃。
 */
export async function startPlayback(
  store: PlayerStore,
  track: Track,
  resolvedPath: string,
  requestId: number,
): Promise<void> {
  try {
    await invoke('pause_track')
  } catch (err) {
    logger.warn('pause before play:', err)
  }

  try {
    logger.info('Playing track:', resolvedPath)

    let timeoutId: ReturnType<typeof setTimeout> | null = null
    const playPromise = invoke('play_track', { path: resolvedPath })
    const timeoutPromise = new Promise((_, reject) => {
      timeoutId = setTimeout(
        () => reject(new Error(i18n.global.t('errors.playTimeout'))),
        PLAY_TRACK_TIMEOUT_MS,
      )
    })

    try {
      await Promise.race([playPromise, timeoutPromise])
    } finally {
      if (timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      }
    }

    if (store._activePlayRequestId !== requestId || store._isDestroyed) {
      return
    }

    store.isPlaying = true
    store._updateTaskbarState()

    // 歌词加载不再在此显式触发:useLyrics 的共享 watcher 监听 currentTrack.path
    // 变化后会统一加载 (缓存 → 本地文件 → 在线获取),单一入口避免双路径竞态
  } catch (err) {
    if (store._activePlayRequestId !== requestId || store._isDestroyed) {
      return
    }

    const type = classifyAudioInvokeError(err)
    const handled = errorHandler.handle(err instanceof Error ? err : new Error(String(err)), {
      type,
      severity: ErrorSeverity.HIGH,
      context: { trackPath: resolvedPath, trackName: track.name },
      showToUser: true,
    })

    logger.error('Failed to play track:', handled)
    store.isPlaying = false

    const currentIdx = store.playlist.findIndex(
      (t) => t.path === track.path || t.path === resolvedPath,
    )
    if (store.playlist.length > 1 && currentIdx >= 0 && currentIdx < store.playlist.length - 1) {
      const nextTrackTimeoutId = setTimeout(() => {
        if (!store._isDestroyed && store._activePlayRequestId === requestId) {
          void store.nextTrack()
        }
      }, AUTO_NEXT_TRACK_DELAY_MS)
      // 保存定时器ID以便在cleanup时清理
      store._nextTrackTimeoutId = nextTrackTimeoutId
    }
  } finally {
    if (store._activePlayRequestId === requestId) {
      store._isLoading = false
    }
  }
}
