/**
 * 沉浸式封面模式的控制栏自动隐藏状态机
 *
 * 从 App.vue 抽离:鼠标热区判定 + 空闲计时 + 窗口聚焦/失焦处理,
 * 进入/退出沉浸模式时自动挂载/卸载窗口监听。
 */
import { ref, watch, type Ref } from 'vue'

const IMMERSIVE_IDLE_DELAY = 3000 // 无操作隐藏延时（ms）
const IMMERSIVE_TOP_AREA = 96 // 顶部控制栏热区高度（覆盖顶栏及其下方缓冲区）
const IMMERSIVE_BOTTOM_AREA = 140 // 底部控制栏热区高度（覆盖磨砂玻璃底栏及其上方缓冲区）

export function useImmersiveAutoHide(immersiveCover: Ref<boolean>): {
  /** 控制栏是否可见(模板绑定) */
  immersiveControlsVisible: Ref<boolean>
  /** 用户活动:显示控制栏并重置隐藏计时(键盘等其他输入源也调用此方法) */
  markImmersiveActivity: () => void
  /** 组件卸载时调用:移除窗口监听并清理计时器 */
  cleanup: () => void
} {
  const immersiveControlsVisible = ref(true)
  let immersiveHideTimer: ReturnType<typeof setTimeout> | null = null
  let immersivePointerY = -1

  // 鼠标是否位于顶/底控制栏热区（悬停时不隐藏，保证可点击）
  const immersivePointerInControls = (): boolean => {
    if (immersivePointerY < 0) return false
    return (
      immersivePointerY <= IMMERSIVE_TOP_AREA ||
      immersivePointerY >= window.innerHeight - IMMERSIVE_BOTTOM_AREA
    )
  }

  const markImmersiveActivity = (): void => {
    immersiveControlsVisible.value = true
    if (immersiveHideTimer) clearTimeout(immersiveHideTimer)
    immersiveHideTimer = setTimeout(() => {
      immersiveHideTimer = null
      // 鼠标仍悬停在控制栏区域时继续等待，否则隐藏
      if (immersivePointerInControls()) {
        markImmersiveActivity()
        return
      }
      immersiveControlsVisible.value = false
    }, IMMERSIVE_IDLE_DELAY)
  }

  const handleImmersivePointerMove = (e: MouseEvent): void => {
    immersivePointerY = e.clientY
    markImmersiveActivity()
  }

  // 鼠标移出窗口：清空热区判定。
  const handleImmersivePointerLeave = (): void => {
    immersivePointerY = -1
    markImmersiveActivity()
  }

  // 窗口失焦：用户已转向其他窗口（如点击桌面），立即隐藏控制栏
  const handleImmersiveBlur = (): void => {
    immersivePointerY = -1
    if (immersiveHideTimer) {
      clearTimeout(immersiveHideTimer)
      immersiveHideTimer = null
    }
    immersiveControlsVisible.value = false
  }

  // 窗口重新聚焦：恢复控制栏并重新进入空闲计时
  const handleImmersiveFocus = (): void => {
    markImmersiveActivity()
  }

  // 进入/退出沉浸模式时挂载/卸载自动隐藏逻辑
  watch(immersiveCover, (active) => {
    if (active) {
      immersiveControlsVisible.value = true
      immersivePointerY = -1
      window.addEventListener('mousemove', handleImmersivePointerMove, { passive: true })
      document.addEventListener('mouseleave', handleImmersivePointerLeave)
      window.addEventListener('blur', handleImmersiveBlur)
      window.addEventListener('focus', handleImmersiveFocus)
      markImmersiveActivity()
    } else {
      window.removeEventListener('mousemove', handleImmersivePointerMove)
      document.removeEventListener('mouseleave', handleImmersivePointerLeave)
      window.removeEventListener('blur', handleImmersiveBlur)
      window.removeEventListener('focus', handleImmersiveFocus)
      if (immersiveHideTimer) {
        clearTimeout(immersiveHideTimer)
        immersiveHideTimer = null
      }
      immersiveControlsVisible.value = true
      immersivePointerY = -1
    }
  })

  // 组件卸载时移除监听(与 watch(false) 分支同样彻底,避免泄漏)
  const cleanup = (): void => {
    window.removeEventListener('mousemove', handleImmersivePointerMove)
    document.removeEventListener('mouseleave', handleImmersivePointerLeave)
    window.removeEventListener('blur', handleImmersiveBlur)
    window.removeEventListener('focus', handleImmersiveFocus)
    if (immersiveHideTimer) {
      clearTimeout(immersiveHideTimer)
      immersiveHideTimer = null
    }
  }

  return { immersiveControlsVisible, markImmersiveActivity, cleanup }
}
