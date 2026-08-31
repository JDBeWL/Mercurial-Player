import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { Track } from '@/types'
import type { usePlayerStore } from './player'
import { adjustShuffleAfterRemove } from './shuffle'

/**
 * Player store 的播放列表增删管理,从 player.ts 抽离以降低单文件复杂度。
 * 函数接收 store 实例参数,在运行时与 player store 共享同一 Pinia 实例。
 */
type PlayerStore = ReturnType<typeof usePlayerStore>

export function removeTrackFromPlaylist(store: PlayerStore, path: string): void {
  const index = store.playlist.findIndex((t) => t.path === path)
  if (index === -1) return

  // 先从播放列表中移除
  store.playlist.splice(index, 1)

  // 如果播放列表为空，重置状态
  if (store.playlist.length === 0) {
    store.resetPlayerState(false)
    return
  }

  // 如果删除的是当前播放的歌曲
  if (store.currentTrack?.path === path) {
    const nextIndex = index >= store.playlist.length ? 0 : index
    const wasPlaying = store.isPlaying

    // 暂停当前播放，避免音频状态不一致
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
    // 如果删除的不是当前播放的歌曲，但删除了当前歌曲前面的歌曲，
    // 我们不需要更新 currentTrack，但需要处理 vue 响应式带来的潜在问题
    // 虽然在目前的设计中 currentTrackIndex 是一个 getter，所以它会自动更新

    // 同步更新 shuffle 顺序，避免 _shuffleOrder.length 与 playlist.length 不一致
    // 导致 _isShuffleOrderValid() 返回 false，进而造成 shuffle 模式下单曲列表无限重播
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

  // 如果没有当前曲目或播放列表为空，直接添加到开头
  if (currentIndex === -1 || store.playlist.length === 0) {
    store.playlist.unshift(track)
    logger.info('Added track to beginning of playlist:', track.path)
    return
  }

  // 检查曲目是否已经在播放列表中
  const existingIndex = store.playlist.findIndex((t) => t.path === track.path)

  if (existingIndex !== -1) {
    // 如果曲目已存在，先移除它
    store.playlist.splice(existingIndex, 1)

    // 移除后当前曲目的实际索引可能已偏移，需要重新计算
    const adjustedCurrentIndex = existingIndex < currentIndex ? currentIndex - 1 : currentIndex
    // 插入到当前曲目之后
    const adjustedIndex = adjustedCurrentIndex + 1
    store.playlist.splice(adjustedIndex, 0, track)
    logger.info('Moved existing track to next position:', track.path)
  } else {
    // 如果曲目不存在，直接插入到当前曲目后面
    store.playlist.splice(currentIndex + 1, 0, track)
    logger.info('Added new track to next position:', track.path)
  }
}
