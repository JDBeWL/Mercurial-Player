/**
 * 播放控制的纯逻辑层:接收最小 store 形状(与 Pinia player store 结构兼容)以避免循环依赖。
 * play/pause/next/previous 与播放列表强耦合,仍留在 index.ts。
 */
import type { Track } from '@/types'
import { ErrorSeverity } from '../../utils/errorHandler'
import { safeInvoke } from '../../utils/safeInvoke'
import { useConfigStore } from '../config'

/** 播放控制涉及的最小 store 状态(与 Pinia player store 结构兼容) */
export interface PlayerPlaybackTarget {
  currentTrack: Track | null
  isPlaying: boolean
  currentTime: number
  duration: number
  volume: number
  isMuted: boolean
  previousVolume: number
}

/**
 * 跳转进度。后端 seek 成功后刷新本地 currentTime;
 * 若 seek 前处于暂停态,后端 seek 总会播放,需要补一次 pause
 */
export function seekTrack(store: PlayerPlaybackTarget, time: number): void {
  if (!store.currentTrack) return

  const wasPlaying = store.isPlaying
  const newTime = Math.max(0, Math.min(time, store.duration))

  // rethrow:true + catch(() => {}) -> 仅在成功后刷新状态,失败已由 safeInvoke 记入 errorHandler
  safeInvoke<void>(
    'seek_track',
    { time: newTime },
    { severity: ErrorSeverity.MEDIUM, rethrow: true },
  )
    .then(() => {
      store.currentTime = newTime
      if (!wasPlaying) {
        void safeInvoke('pause_track', undefined, { severity: ErrorSeverity.LOW })
      }
    })
    .catch(() => {})
}

/** 设置音量(取值 0-1),成功后防抖落盘 */
export function setPlayerVolume(store: PlayerPlaybackTarget, volume: number): void {
  const newVolume = Math.max(0, Math.min(1, volume))
  store.volume = newVolume

  if (newVolume > 0 && store.isMuted) {
    store.isMuted = false
  }

  if (newVolume > 0) {
    store.previousVolume = newVolume
  }

  void safeInvoke<void>(
    'set_volume',
    { volume: store.isMuted ? 0 : newVolume },
    { severity: ErrorSeverity.MEDIUM },
  ).then(() => {
    const configStore = useConfigStore()
    configStore.audio.volume = newVolume
    // 拖动音量条每次 mousemove 都走到这里,必须走防抖 saveConfig(理由见 config.ts 的 saveConfig)
    configStore.saveConfig()
  })
}

/** 静音/取消静音:静音保存当前音量,取消时恢复到 previousVolume */
export function togglePlayerMute(store: PlayerPlaybackTarget): void {
  if (store.isMuted) {
    store.isMuted = false
    const volumeToRestore = store.previousVolume > 0 ? store.previousVolume : 0.5
    store.volume = volumeToRestore
    safeInvoke<void>(
      'set_volume',
      { volume: volumeToRestore },
      { severity: ErrorSeverity.MEDIUM, rethrow: true },
    )
      .then(() => {
        const configStore = useConfigStore()
        configStore.audio.volume = volumeToRestore
        void configStore.saveConfigNow()
      })
      .catch(() => {})
  } else {
    store.previousVolume = store.volume > 0 ? store.volume : store.previousVolume
    store.isMuted = true
    void safeInvoke('set_volume', { volume: 0 }, { severity: ErrorSeverity.MEDIUM })
  }
}
