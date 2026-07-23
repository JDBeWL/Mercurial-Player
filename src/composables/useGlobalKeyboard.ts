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
        const newVolume = Math.min(1, playerStore.volume + 0.05)
        playerStore.setVolume(newVolume)
        break
      }
      case 'ArrowDown': {
        event.preventDefault()
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
