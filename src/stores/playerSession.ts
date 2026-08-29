import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { Track, ResumeResult, TrackSnapshot } from '@/types'
import type { usePlayerStore } from './player'
import { useMusicLibraryStore } from './musicLibrary'

/**
 * Player store 的会话持久化与启动恢复,从 player.ts 抽离以降低单文件复杂度。
 * 函数接收 store 实例参数,在运行时与 player store 共享同一 Pinia 实例。
 */
type PlayerStore = ReturnType<typeof usePlayerStore>

/**
 * 立即保存 last_session (无节流,用于 pause/切曲/关闭等关键节点)
 *
 * 不在 playback-position 事件中写入,避免播放期间频繁写盘。
 * 仅在以下场景触发:
 * - pause():暂停时立即保存
 * - playTrack():切曲前保存上一曲的最后位置
 * - cleanup():关闭窗口前 await 保存
 * 风险:程序崩溃/断电时丢失自上次关键节点以来的进度。
 */
export async function saveLastSessionNow(store: PlayerStore): Promise<void> {
  if (!store.currentTrack || store._isDestroyed) return
  const track = store.currentTrack
  // 从 musicLibraryStore 获取当前播放列表名 (用于启动恢复)
  let playlistName: string | null = null
  let trackIndexInPlaylist: number | null = null
  try {
    const musicLibraryStore = useMusicLibraryStore()
    if (musicLibraryStore.currentPlaylist) {
      playlistName = musicLibraryStore.currentPlaylist.name
      const idx = musicLibraryStore.currentPlaylist.files.findIndex((f) => f.path === track.path)
      if (idx >= 0) trackIndexInPlaylist = idx
    }
  } catch (err) {
    logger.debug('Failed to get current playlist name:', err)
  }
  // 提取 player.playlist 的元数据快照 (不依赖 musicLibrary 缓存)
  // 这样启动恢复时即使 musicLibrary 还没加载,也能直接重建 player.playlist
  const playlistTracks: TrackSnapshot[] = store.playlist.map((t) => ({
    path: t.path,
    title: t.title ?? null,
    artist: t.artist ?? null,
    album: t.album ?? null,
    duration: t.duration ?? null,
    bitrate: t.bitrate ?? null,
    sampleRate: t.sampleRate ?? null,
    channels: t.channels ?? null,
    bitDepth: t.bitDepth ?? null,
    format: t.format ?? null,
  }))
  try {
    await invoke('save_last_session', {
      trackPath: track.path,
      trackTitle: track.title || track.displayTitle || track.name || '',
      trackArtist: track.artist || track.displayArtist || '',
      durationSecs: store.duration || 0,
      positionSecs: store.currentTime,
      playlistName,
      trackIndexInPlaylist,
      playlistTracks,
    })
  } catch (err) {
    logger.debug('Failed to save last session:', err)
  }
}

/**
 * 启动时调用 - 尝试恢复上次播放会话
 *
 * 行为:
 * - resumed=true: 后端已加载文件并暂停在 position,前端设置 UI 状态
 *   (currentTrack/duration/currentTime/audioInfo/playlist),isPlaying=false 保持暂停
 * - resumed=false 且 status='not_found': 文件不存在,从播放列表移除该路径
 * - 其他 false 状态: 静默忽略,无需 UI 反馈
 */
export async function resumeLastSession(store: PlayerStore): Promise<ResumeResult | null> {
  try {
    const result = await invoke<ResumeResult>('resume_last_session')
    if (result.resumed && result.trackPath) {
      const trackPath = result.trackPath

      // 1. 用返回的 playlistTracks 直接构造 player.playlist
      //    不依赖 musicLibrary 缓存是否加载,保证恢复后播放列表完整
      const playlistTracks = result.playlistTracks ?? []
      const playlist: Track[] = playlistTracks.map((s) => ({
        path: s.path,
        title: s.title ?? undefined,
        artist: s.artist ?? undefined,
        album: s.album ?? undefined,
        displayTitle: s.title ?? undefined,
        displayArtist: s.artist ?? undefined,
        duration: s.duration ?? undefined,
        bitrate: s.bitrate ?? null,
        sampleRate: s.sampleRate ?? null,
        channels: s.channels ?? null,
        bitDepth: s.bitDepth ?? null,
        format: s.format ?? null,
      }))
      store.playlist = playlist

      // 2. 在 playlist 中查找当前曲目 (含完整元数据: bitrate/sampleRate 等)
      let matchedTrack: Track | null = playlist.find((t) => t.path === trackPath) ?? null

      // 3. 如果 playlistTracks 为空或没找到,用 lastSession 快照构造一个最小 Track
      if (!matchedTrack) {
        matchedTrack = {
          path: trackPath,
          title: result.trackTitle || undefined,
          artist: result.trackArtist || undefined,
          displayTitle: result.trackTitle || undefined,
          displayArtist: result.trackArtist || undefined,
          duration: result.durationSecs ?? undefined,
        }
        // 当前曲目不在 playlistTracks 中,把它加到 playlist 末尾
        store.playlist.push(matchedTrack)
      }

      // 4. 尝试同步 musicLibrary 的 currentPlaylist (让 UI 高亮,但不依赖它)
      if (result.playlistName) {
        try {
          const musicLibraryStore = useMusicLibraryStore()
          if (musicLibraryStore.playlists.length === 0) {
            await musicLibraryStore.loadPlaylistsFromCache()
          }
          const mlPlaylist = musicLibraryStore.playlists.find((p) => p.name === result.playlistName)
          if (mlPlaylist) {
            musicLibraryStore.selectPlaylist(mlPlaylist)
            // 如果 musicLibrary 中的曲目有更完整的元数据 (比如 coverPath 已加载),用它
            const mlTrack = mlPlaylist.files.find((t) => t.path === trackPath)
            if (mlTrack && mlTrack.bitrate && !matchedTrack.bitrate) {
              matchedTrack = mlTrack
            }
          }
        } catch (err) {
          logger.warn('Failed to sync musicLibrary playlist for resume:', err)
        }
      }

      // 5. 触发元数据缓存和封面预加载 (与 loadPlaylist 一致)
      if (store._cacheAbortController) {
        store._cacheAbortController.abort()
      }
      store._cachePlaylistMetadata(store.playlist)
      store._loadPlaylistCovers(store.playlist)

      // 6. 设置播放状态 (currentTrack/audioInfo/duration/currentTime)
      store.currentTrack = matchedTrack
      store.duration = matchedTrack.duration ?? result.durationSecs ?? 0
      store.currentTime = result.positionSecs ?? 0
      // 从 matchedTrack 提取完整音频元数据 (bitrate/sampleRate/channels/bitDepth/format)
      store.audioInfo = {
        bitrate: matchedTrack.bitrate || null,
        sampleRate: matchedTrack.sampleRate || null,
        channels: matchedTrack.channels || null,
        bitDepth: matchedTrack.bitDepth || null,
        format: matchedTrack.format || null,
      }
      // 保持暂停状态 - 用户主动点播放才会开始
      store.isPlaying = false
      store._updateTaskbarState()
      logger.info(
        `Resumed last session (paused): ${trackPath} @ ${result.positionSecs}s (${result.status}), playlist=${playlist.length} tracks`,
      )

      // 7. 加载当前曲目封面 (异步,不阻塞恢复)
      invoke<string | null>('get_track_cover_path', { path: trackPath })
        .then((coverPath) => {
          // 守卫:应用关闭后不再修改已销毁的 store state
          if (store._isDestroyed) return
          if (store.currentTrack && store.currentTrack.path === trackPath && coverPath) {
            store.currentTrack.coverPath = coverPath
          }
        })
        .catch((err) => logger.debug('Failed to load cover for resumed track:', err))
    } else if (result.status === 'not_found' && result.trackPath) {
      // 静默处理:从当前播放列表移除该文件 (用户选择)
      const idx = store.playlist.findIndex((t) => t.path === result.trackPath)
      if (idx >= 0) {
        store.playlist.splice(idx, 1)
        logger.info(`Removed missing track from playlist: ${result.trackPath}`)
      }
    }
    return result
  } catch (err) {
    logger.error('Failed to resume last session:', err)
    return null
  }
}
