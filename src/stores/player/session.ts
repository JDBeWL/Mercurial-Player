import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { Track, ResumeResult, TrackSnapshot } from '@/types'
import type { usePlayerStore } from './index'
import { useMusicLibraryStore } from '../musicLibrary'

/** Player store 的会话持久化与启动恢复。 */
type PlayerStore = ReturnType<typeof usePlayerStore>

/**
 * 立即保存 last_session(无节流)：仅在 pause、切曲前、cleanup 关闭前这几个关键节点触发，
 * 不在 playback-position 中写入，代价是崩溃/断电时丢失自上次关键节点以来的进度。
 */
export async function saveLastSessionNow(store: PlayerStore): Promise<void> {
  if (!store.currentTrack || store._isDestroyed) return
  const track = store.currentTrack
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
  // 随会话存一份 playlist 元数据快照,恢复时就不依赖 musicLibrary 是否已加载
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
 * resumed=true：后端已加载并暂停在 position，前端补 UI 状态且保持暂停。
 * not_found：文件已不存在，从播放列表移除该路径；其余 false 静默忽略。
 */
export async function resumeLastSession(store: PlayerStore): Promise<ResumeResult | null> {
  try {
    const result = await invoke<ResumeResult>('resume_last_session')
    if (result.resumed && result.trackPath) {
      const trackPath = result.trackPath

      // 1. 用快照里的 playlistTracks 重建播放列表(理由见 saveLastSessionNow)
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
      store._setPlaylist(playlist)

      // 2. 在 playlist 中找到含完整元数据(bitrate/sampleRate 等)的当前曲目
      let matchedTrack: Track | null = playlist.find((t) => t.path === trackPath) ?? null

      // 3. 快照里没有时,用 lastSession 的字段构造一个最小 Track 并追加到列表末尾
      if (!matchedTrack) {
        matchedTrack = {
          path: trackPath,
          title: result.trackTitle || undefined,
          artist: result.trackArtist || undefined,
          displayTitle: result.trackTitle || undefined,
          displayArtist: result.trackArtist || undefined,
          duration: result.durationSecs ?? undefined,
        }
        store._setPlaylist([...store.playlist, matchedTrack])
      }

      // 4. 同步 musicLibrary 的 currentPlaylist,只为 UI 高亮,不依赖它
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

      // 5. 与 loadPlaylist 一致:作废旧缓存任务后触发元数据缓存与封面预加载
      if (store._cacheAbortController) {
        store._cacheAbortController.abort()
      }
      void store._cachePlaylistMetadata(store.playlist)
      void store._loadPlaylistCovers(store.playlist)

      // 6. 写入播放状态,后端 positionSecs/durationSecs 的单位是秒
      store.currentTrack = matchedTrack
      store.duration = matchedTrack.duration ?? result.durationSecs ?? 0
      store.currentTime = result.positionSecs ?? 0
      store.audioInfo = {
        bitrate: matchedTrack.bitrate || null,
        sampleRate: matchedTrack.sampleRate || null,
        channels: matchedTrack.channels || null,
        bitDepth: matchedTrack.bitDepth || null,
        format: matchedTrack.format || null,
      }
      // 保持暂停:只有用户主动点播放才开始
      store.isPlaying = false
      store._updateTaskbarState()
      logger.info(
        `Resumed last session (paused): ${trackPath} @ ${result.positionSecs}s (${result.status}), playlist=${playlist.length} tracks`,
      )

      // 7. 异步加载当前曲目封面,不阻塞恢复
      invoke<string | null>('get_track_cover_path', { path: trackPath })
        .then((coverPath) => {
          // 守卫:应用关闭后不再修改已销毁的 store state
          if (store._isDestroyed) return
          const current = store.currentTrack
          // 整体重新赋值而非就地写字段(markRaw 约束见 index.ts 的 _setPlaylist)
          if (
            current &&
            current.path === trackPath &&
            coverPath &&
            current.coverPath !== coverPath
          ) {
            store.currentTrack = { ...current, coverPath }
          }
        })
        .catch((err) => logger.debug('Failed to load cover for resumed track:', err))
    } else if (result.status === 'not_found' && result.trackPath) {
      // not_found:静默从当前播放列表移除该文件
      const idx = store.playlist.findIndex((t) => t.path === result.trackPath)
      if (idx >= 0) {
        store._setPlaylist(store.playlist.filter((_, i) => i !== idx))
        logger.info(`Removed missing track from playlist: ${result.trackPath}`)
      }
    }
    return result
  } catch (err) {
    logger.error('Failed to resume last session:', err)
    return null
  }
}
