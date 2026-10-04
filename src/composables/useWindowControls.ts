import { ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import logger from '@/utils/logger'

/**
 * 窗口控制: 最小化 / 全屏切换 / 关闭, 并维护 isFullscreen、isMaximized 两个本地状态。
 * 进入全屏前先取消最大化, 否则 Windows 下两种状态会冲突。
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

  /** 查询窗口真实状态刷新两个 ref, 通常在 onMounted 调用一次 (标题栏按钮图标直接绑这两个 ref) */
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
