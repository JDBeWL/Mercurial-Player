import { ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import logger from '@/utils/logger'

/**
 * 窗口控制 Composable
 *
 * 封装窗口最小化、全屏切换、关闭等操作，并维护 isFullscreen/isMaximized 状态。
 * 进入全屏前会先取消最大化，避免 Windows 下两种状态冲突。
 */
export function useWindowControls() {
  const appWindow = getCurrentWindow()
  const isFullscreen = ref(false)
  const isMaximized = ref(false)

  const minimizeWindow = async (): Promise<void> => {
    try {
      await appWindow.minimize()
    } catch (error) {
      logger.error('Failed to minimize window:', error)
    }
  }

  const toggleFullscreen = async (): Promise<void> => {
    try {
      if (isFullscreen.value) {
        await appWindow.setFullscreen(false)
        isFullscreen.value = false
      } else {
        // 进入全屏前先检查并取消最大化状态
        const currentlyMaximized = await appWindow.isMaximized()
        if (currentlyMaximized) {
          await appWindow.unmaximize()
        }
        await appWindow.setFullscreen(true)
        isFullscreen.value = true
      }
    } catch (error) {
      logger.error('Failed to toggle fullscreen:', error)
    }
  }

  const closeWindow = async (): Promise<void> => {
    try {
      await appWindow.close()
    } catch (error) {
      logger.error('Failed to close window:', error)
    }
  }

  /**
   * 同步当前窗口的 isFullscreen/isMaximized 状态。
   * 通常在 onMounted 中调用一次，确保按钮图标与实际窗口状态一致。
   */
  const syncWindowState = async (): Promise<void> => {
    try {
      isFullscreen.value = await appWindow.isFullscreen()
      isMaximized.value = await appWindow.isMaximized()
    } catch (error) {
      logger.error('Failed to check window state:', error)
    }
  }

  return {
    isFullscreen,
    isMaximized,
    minimizeWindow,
    toggleFullscreen,
    closeWindow,
    syncWindowState,
  }
}
