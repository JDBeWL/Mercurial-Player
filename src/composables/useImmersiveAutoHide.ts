/** 沉浸式控制栏自动隐藏状态机: 热区判定、空闲计时、窗口聚焦处理 */
import { ref, watch, type Ref } from 'vue'

const IMMERSIVE_IDLE_DELAY = 3000 // 无操作隐藏延时 (ms)
const IMMERSIVE_TOP_AREA = 96 // 顶部热区高度 (px): 除顶栏本身外还要盖住其下方的缓冲带
const IMMERSIVE_BOTTOM_AREA = 140 // 底部热区高度 (px), 缓冲带同理

export function useImmersiveAutoHide(immersiveCover: Ref<boolean>): {
  /** 控制栏是否可见 (模板绑定) */
  immersiveControlsVisible: Ref<boolean>
  /** 用户活动: 显示控制栏并重置隐藏计时 (键盘等其他输入源也走这里) */
  markImmersiveActivity: () => void
  /** 组件卸载时调用: 移除窗口监听并清理计时器 */
  cleanup: () => void
} {
  const immersiveControlsVisible = ref(true)
  let immersiveHideTimer: ReturnType<typeof setTimeout> | null = null
  let immersivePointerY = -1

  // 指针 (鼠标/手指) 是否停在顶/底控制栏热区:停在这两处时不隐藏,保证还能点得到
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
      // 鼠标仍悬停在控制栏区域时重新计时, 否则隐藏
      if (immersivePointerInControls()) {
        markImmersiveActivity()
        return
      }
      immersiveControlsVisible.value = false
    }, IMMERSIVE_IDLE_DELAY)
  }

  const handleImmersivePointerMove = (e: PointerEvent): void => {
    immersivePointerY = e.clientY
    markImmersiveActivity()
  }

  // 只标记活动、不更新热区 Y:单次 pointerdown 不足以说明手指停在这里,写进去反而让悬浮判定失真
  const handleImmersivePointerDown = (): void => {
    markImmersiveActivity()
  }

  // 鼠标移出窗口:把 Y 复位成 -1,即"不在热区"
  const handleImmersivePointerLeave = (): void => {
    immersivePointerY = -1
    markImmersiveActivity()
  }

  // 窗口失焦:用户已转向其它窗口,立即隐藏控制栏
  const handleImmersiveBlur = (): void => {
    immersivePointerY = -1
    if (immersiveHideTimer) {
      clearTimeout(immersiveHideTimer)
      immersiveHideTimer = null
    }
    immersiveControlsVisible.value = false
  }

  const handleImmersiveFocus = (): void => {
    markImmersiveActivity()
  }

  watch(immersiveCover, (active) => {
    if (active) {
      immersiveControlsVisible.value = true
      immersivePointerY = -1
      // 用 Pointer Events 而非 mouse events (原因见 useDragValue 头注释); pointerdown 管"点一下唤出", pointermove 管拖拽
      window.addEventListener('pointermove', handleImmersivePointerMove, { passive: true })
      window.addEventListener('pointerdown', handleImmersivePointerDown, { passive: true })
      document.addEventListener('mouseleave', handleImmersivePointerLeave)
      window.addEventListener('blur', handleImmersiveBlur)
      window.addEventListener('focus', handleImmersiveFocus)
      markImmersiveActivity()
    } else {
      window.removeEventListener('pointermove', handleImmersivePointerMove)
      window.removeEventListener('pointerdown', handleImmersivePointerDown)
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

  // 组件卸载时移除监听,与 watch(false) 分支同样彻底,避免泄漏
  const cleanup = (): void => {
    window.removeEventListener('pointermove', handleImmersivePointerMove)
    window.removeEventListener('pointerdown', handleImmersivePointerDown)
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
