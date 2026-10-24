/** 播放控制纯逻辑层 / 接收最小 store 形状以避免循环依赖；强耦合的 play/pause 仍在 index.ts。 */
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

/** seek 请求序号：完成回调落地前与最新值比较，只让最新请求改写状态（latest-wins） */
let seekRequestSeq = 0

/**
 * 跳转进度。后端 seek 成功后刷新本地 currentTime;
 * 若 seek 前处于暂停态,后端 seek 总会播放,需要补一次 pause
 *
 * 竞态防护：捕获发起时的曲目路径并携带单调递增的请求 ID，完成时若已切歌或已有更新的
 * seek 发起，本次结果作废——旧 seek 晚到不得覆盖新 seek 的位置，更不得改写新曲目的进度。
 * 返回真实 Promise 供调用方等待 IPC 完成。
 */
export function seekTrack(store: PlayerPlaybackTarget, time: number): Promise<void> {
  if (!store.currentTrack) return Promise.resolve()

  const trackPath = store.currentTrack.path
  const requestId = ++seekRequestSeq
  const wasPlaying = store.isPlaying
  const newTime = Math.max(0, Math.min(time, store.duration))

  // rethrow:true + catch(() => {}) -> 仅在成功后刷新状态,失败已由 safeInvoke 记入 errorHandler
  return safeInvoke<void>(
    'seek_track',
    { time: newTime },
    { severity: ErrorSeverity.MEDIUM, rethrow: true },
  )
    .then(() => {
      if (requestId !== seekRequestSeq || store.currentTrack?.path !== trackPath) {
        return
      }
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
