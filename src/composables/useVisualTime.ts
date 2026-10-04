/**
 * 视觉时间系统 composable
 *
 * 歌词显示与可视化器共用的卡拉OK时间源:rAF 内按帧间隔累加,再用 P 控制器调速消除与真实播放时间的漂移
 */
import { ref, watch, type Ref } from 'vue'
import { usePlayerStore } from '@/stores/player'

// 误差超过该值(秒)直接硬同步
const HARD_SYNC_THRESHOLD = 0.5
// 误差低于该值(秒)按原速累加
const SMOOTH_THRESHOLD = 0.05
// P 控制器增益,越大追赶越快
const P_GAIN = 2.0
// 追赶速度上下限,避免画面时间流速明显异常
const MIN_SPEED = 0.7
const MAX_SPEED = 1.3
// 单帧最大时间步长(秒),防止标签页休眠/掉帧后时间跳变
const MAX_DELTA = 0.1

/**
 * 检测 seek 操作:时间跳变超过 1.5s(正常播放每次只增加 0.5s)或时间倒退
 */
const isSeekJump = (newTime: number, oldTime: number): boolean => {
  const jump = newTime - oldTime
  return Math.abs(jump) > 1.5 || jump < -0.1
}

export function useVisualTime(): {
  visualTime: Ref<number>
  /** 在 rAF 回调中调用,按帧间隔推进视觉时间 */
  advanceVisualTime: (timestamp: number) => void
  /** 重置帧时钟(rAF 循环启动时调用) */
  resetFrameClock: () => void
  /** 立即同步到真实播放时间 */
  syncToCurrentTime: () => void
} {
  const playerStore = usePlayerStore()
  const visualTime = ref(0)
  // 用 null 表示"尚未播种":时间戳 0 是合法值,拿 0 当哨兵会让此后每帧都被视为首帧、delta 恒为 0
  let lastFrameTime: number | null = null

  const advanceVisualTime = (timestamp: number): void => {
    if (lastFrameTime === null) lastFrameTime = timestamp
    const deltaTime = Math.min((timestamp - lastFrameTime) / 1000, MAX_DELTA)
    lastFrameTime = timestamp

    const realTime = playerStore.currentTime
    const diff = visualTime.value - realTime // 正值表示视觉领先,负值表示落后

    if (Math.abs(diff) > HARD_SYNC_THRESHOLD) {
      visualTime.value = realTime
    } else if (Math.abs(diff) > SMOOTH_THRESHOLD) {
      const speed = 1.0 - diff * P_GAIN
      const clampedSpeed = Math.max(MIN_SPEED, Math.min(MAX_SPEED, speed))
      visualTime.value += deltaTime * clampedSpeed
    } else {
      visualTime.value += deltaTime
    }
  }

  const resetFrameClock = (): void => {
    lastFrameTime = null
  }

  const syncToCurrentTime = (): void => {
    visualTime.value = playerStore.currentTime
  }

  // seek(如拖拽进度条)立即同步:调用方的 rAF 循环只在播放时跑,暂停时没有 advanceVisualTime 来追平
  watch(
    () => playerStore.currentTime,
    (newTime, oldTime) => {
      if (isSeekJump(newTime, oldTime)) {
        visualTime.value = newTime
      }
    },
  )

  return { visualTime, advanceVisualTime, resetFrameClock, syncToCurrentTime }
}
