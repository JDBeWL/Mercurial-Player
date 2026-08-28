import { onMounted, onUnmounted } from 'vue'
import { usePlayerStore } from '@/stores/player'

/**
 * 全局键盘事件 Composable
 *
 * 监听全局 keydown 事件，处理以下快捷键：
 * - 空格：暂停/恢复播放
 * - 左/右方向键：上一首/下一首
 * - 上/下方向键：音量增减（步长 0.05）
 *
 * 自动在 onMounted 时注册监听器，在 onUnmounted 时注销。
 * 当焦点位于 INPUT/TEXTAREA 或可编辑元素上时跳过处理，避免与输入冲突。
 */
export function useGlobalKeyboard(): void {
  const playerStore = usePlayerStore()

  // 音量键节流：按住方向键时 keydown 以系统重复率触发（可达 ~30Hz），
  // 每次都会跨 IPC 调 set_volume 并触发配置保存，节流到最高 ~12 次/秒
  const VOLUME_REPEAT_MIN_INTERVAL_MS = 80
  let lastVolumeChangeAt = 0

  const handleKeyDown = (event: KeyboardEvent): void => {
    // 只在用户没有在输入框等元素中编辑时响应键盘事件
    const activeEl = document.activeElement
    if (!activeEl) return

    const isInputFocused =
      activeEl.tagName === 'INPUT' ||
      activeEl.tagName === 'TEXTAREA' ||
      (activeEl as HTMLElement).isContentEditable

    if (isInputFocused) return

    // 空格键暂停/恢复播放
    if (event.code === 'Space') {
      event.preventDefault()
      playerStore.togglePlay()
    }

    // 方向键控制
    switch (event.code) {
      case 'ArrowLeft':
        event.preventDefault()
        if (playerStore.hasPreviousTrack) {
          playerStore.previousTrack()
        }
        break
      case 'ArrowRight':
        event.preventDefault()
        if (playerStore.hasNextTrack) {
          playerStore.nextTrack()
        }
        break
      case 'ArrowUp': {
        event.preventDefault()
        const now = performance.now()
        if (event.repeat && now - lastVolumeChangeAt < VOLUME_REPEAT_MIN_INTERVAL_MS) break
        lastVolumeChangeAt = now
        const newVolume = Math.min(1, playerStore.volume + 0.05)
        playerStore.setVolume(newVolume)
        break
      }
      case 'ArrowDown': {
        event.preventDefault()
        const now = performance.now()
        if (event.repeat && now - lastVolumeChangeAt < VOLUME_REPEAT_MIN_INTERVAL_MS) break
        lastVolumeChangeAt = now
        const newVolume = Math.max(0, playerStore.volume - 0.05)
        playerStore.setVolume(newVolume)
        break
      }
    }
  }

  onMounted(() => {
    document.addEventListener('keydown', handleKeyDown)
  })

  onUnmounted(() => {
    document.removeEventListener('keydown', handleKeyDown)
  })
}
