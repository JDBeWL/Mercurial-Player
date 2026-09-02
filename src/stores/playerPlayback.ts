/**
 * 播放控制模块:seek / 音量 / 静音。
 *
 * 从 stores/player.ts 拆出的纯逻辑层,避免 options store 越滚越大。
 * 函数接收最小化的 store 形状(而非 import player 造成循环依赖),
 * 通过 Pinia action 委托调用,行为与原实现完全一致。
 *
 * 注:play/pause/next/previous 与播放列表、结束后自动切歌逻辑强耦合,
 * 未随本模块迁出,留在 player.ts。
 */
import type { Track } from '@/types'
import { ErrorSeverity } from '../utils/errorHandler'
import { safeInvoke } from '../utils/safeInvoke'
import { useConfigStore } from './config'

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

  // rethrow:true + catch(() => {}) → 仅在成功后刷新状态,失败已由 safeInvoke 记入 errorHandler
  safeInvoke<void>('seek_track', { time: newTime }, { severity: ErrorSeverity.MEDIUM, rethrow: true })
    .then(() => {
      store.currentTime = newTime
      if (!wasPlaying) {
        // 后端 seek 总是 play，如果之前是暂停状态需要重新暂停
        safeInvoke('pause_track', undefined, { severity: ErrorSeverity.LOW })
      }
    })
    .catch(() => {})
}

/** 设置音量(0-1),成功后同步配置防抖落盘 */
export function setPlayerVolume(store: PlayerPlaybackTarget, volume: number): void {
  const newVolume = Math.max(0, Math.min(1, volume))
  store.volume = newVolume

  // 如果设置音量大于0，取消静音状态
  if (newVolume > 0 && store.isMuted) {
    store.isMuted = false
  }

  // 如果音量大于0，更新 previousVolume
  if (newVolume > 0) {
    store.previousVolume = newVolume
  }

  safeInvoke<void>(
    'set_volume',
    { volume: store.isMuted ? 0 : newVolume },
    { severity: ErrorSeverity.MEDIUM },
  ).then(() => {
    const configStore = useConfigStore()
    configStore.audio.volume = newVolume
    // 拖动音量条时每次 mousemove 都会走到这里：saveConfig 会对整个 config
    // (含 lastSession 的播放队列快照)做深比较 + 深拷贝 + 写盘,大队列下足以
    // 卡住主线程、让滑块掉帧;故用防抖保存(2s),关闭应用前有 flushPendingSave 兜底
    configStore.saveConfig()
  })
}

/** 静音/取消静音:静音保存当前音量,取消时恢复到 previousVolume */
export function togglePlayerMute(store: PlayerPlaybackTarget): void {
  if (store.isMuted) {
    // 取消静音，恢复之前的音量
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
        configStore.saveConfigNow()
      })
      .catch(() => {})
  } else {
    // 静音，保存当前音量
    store.previousVolume = store.volume > 0 ? store.volume : store.previousVolume
    store.isMuted = true
    safeInvoke('set_volume', { volume: 0 }, { severity: ErrorSeverity.MEDIUM })
  }
}
