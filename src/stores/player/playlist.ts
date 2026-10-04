import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { Track } from '@/types'
import type { usePlayerStore } from './index'
import { adjustShuffleAfterRemove } from './shuffle'

/** Player store 的播放列表增删管理。 */
type PlayerStore = ReturnType<typeof usePlayerStore>

export function removeTrackFromPlaylist(store: PlayerStore, path: string): void {
  const index = store.playlist.findIndex((t) => t.path === path)
  if (index === -1) return

  // 整体重新赋值而非 splice(markRaw 约束见 index.ts 的 _setPlaylist)
  store._setPlaylist(store.playlist.filter((_, i) => i !== index))

  // 列表已空:重置状态但保留播放列表
  if (store.playlist.length === 0) {
    void store.resetPlayerState(false)
    return
  }

  if (store.currentTrack?.path === path) {
    const nextIndex = index >= store.playlist.length ? 0 : index
    const wasPlaying = store.isPlaying

    // 先暂停,避免新旧曲目音频状态不一致
    if (wasPlaying) {
      invoke('pause_track').catch((err) => logger.warn('pause before remove:', err))
    }

    store
      .playTrack(store.playlist[nextIndex]!)
      .then(() => {
        if (!wasPlaying) {
          store.pause()
        }
      })
      .catch((err) => logger.warn('play after remove failed:', err))
  } else {
    // currentTrackIndex 是 getter,删掉前面的曲目不需要修正当前曲目

    // 同步校正 shuffle 顺序,否则 _shuffleOrder 与 playlist 长度不一致会让
    // _isShuffleOrderValid() 返回 false,shuffle 模式下单曲列表会无限重播
    if (store._shuffleOrder.length > 0) {
      const adjusted = adjustShuffleAfterRemove(
        store._shuffleOrder,
        store._shufflePosition,
        store._shuffleHistory,
        index,
      )
      store._shuffleOrder = adjusted.order
      store._shufflePosition = adjusted.position
      store._shuffleHistory = adjusted.history
    }
  }
}

export function addTrackNextInPlaylist(store: PlayerStore, track: Track): void {
  if (!track) return

  const currentIndex = store.currentTrackIndex

  if (currentIndex === -1 || store.playlist.length === 0) {
    store._setPlaylist([track, ...store.playlist])
    logger.info('Added track to beginning of playlist:', track.path)
    return
  }

  const existingIndex = store.playlist.findIndex((t) => t.path === track.path)

  if (existingIndex !== -1) {
    // 已在列表中:先摘出,再插到当前曲目之后
    const rest = store.playlist.filter((_, i) => i !== existingIndex)

    // 移除后当前曲目的实际索引可能已偏移，需要重新计算
    const adjustedCurrentIndex = existingIndex < currentIndex ? currentIndex - 1 : currentIndex
    const at = adjustedCurrentIndex + 1
    store._setPlaylist([...rest.slice(0, at), track, ...rest.slice(at)])
    logger.info('Moved existing track to next position:', track.path)
  } else {
    const at = currentIndex + 1
    store._setPlaylist([...store.playlist.slice(0, at), track, ...store.playlist.slice(at)])
    logger.info('Added new track to next position:', track.path)
  }
}
