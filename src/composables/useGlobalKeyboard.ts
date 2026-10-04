import { onMounted, onUnmounted } from 'vue'
import { usePlayerStore } from '@/stores/player'

/**
 * 全局键盘快捷键: 空格 播放/暂停, 左/右方向键 上一首/下一首, 上/下方向键 音量 +/- 0.05 (音量域 0~1)。
 *
 * onMounted 注册 document keydown、onUnmounted 注销; 焦点在 INPUT/TEXTAREA/可编辑元素上时整体跳过。
 * 每个分支都 preventDefault: 否则空格会顺带激活当前聚焦的按钮、方向键会滚动页面。
 */
export function useGlobalKeyboard(): void {
  const playerStore = usePlayerStore()

  // 按住方向键时 keydown 以系统重复率触发 (~30Hz), 每次都跨 IPC 调 set_volume 并触发配置保存, 节流到最高 ~12 次/秒
  const VOLUME_REPEAT_MIN_INTERVAL_MS = 80
  let lastVolumeChangeAt = 0

  const handleKeyDown = (event: KeyboardEvent): void => {
    const activeEl = document.activeElement
    if (!activeEl) return

    const isInputFocused =
      activeEl.tagName === 'INPUT' ||
      activeEl.tagName === 'TEXTAREA' ||
      (activeEl as HTMLElement).isContentEditable

    if (isInputFocused) return

    if (event.code === 'Space') {
      event.preventDefault()
      playerStore.togglePlay()
    }

    switch (event.code) {
      case 'ArrowLeft':
        event.preventDefault()
        if (playerStore.hasPreviousTrack) {
          void playerStore.previousTrack()
        }
        break
      case 'ArrowRight':
        event.preventDefault()
        if (playerStore.hasNextTrack) {
          void playerStore.nextTrack()
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
