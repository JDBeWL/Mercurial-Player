import { onUnmounted, ref, type Ref } from 'vue'

export interface UseDragValueOptions {
  /** 由指针事件计算 0..1 的拖拽百分比 (方向与几何由调用方决定) */
  getPercent: (event: MouseEvent) => number
  /** pointerdown 时调用;返回 false 取消本次拖拽 (不进入拖拽态、不挂监听器) */
  onStart?: (percent: number, event: MouseEvent) => boolean | void
  /** 拖拽移动时调用 */
  onMove?: (percent: number) => void
  /** pointerup 时调用 (仅当拖拽进行中) */
  onEnd?: (percent: number) => void
}

export interface UseDragValueResult {
  isDragging: Ref<boolean>
  /** 最近一次事件的拖拽百分比 (0..1) */
  percent: Ref<number>
  /** 用调用方的 getPercent 计算任意事件的百分比 (供 click 等非拖拽路径复用) */
  getPercent: (event: MouseEvent) => number
  /** 绑定到滑块元素的 @pointerdown */
  startDrag: (event: MouseEvent) => void
  /** 强制结束拖拽并移除监听器 */
  stopDrag: () => void
}

/** 拖拽取值的骨架：pointerdown 后把 pointermove/pointerup 挂到 document 上，指针移出滑块也不会中断，
 *  组件卸载时自动清理。用 Pointer Events 而不用 mouse events，因为 Android WebView 上手指拖动不产生
 *  mousemove。调用方必须给滑块写 touch-action: none，否则被判成滚动手势、中途抛 pointercancel 打断。 */
export function useDragValue(options: UseDragValueOptions): UseDragValueResult {
  const isDragging = ref(false)
  const percent = ref(0)

  const removeListeners = (): void => {
    document.removeEventListener('pointermove', onDragMove)
    document.removeEventListener('pointerup', onDragEnd)
    document.removeEventListener('pointercancel', onDragCancel)
  }

  const onDragMove = (event: MouseEvent): void => {
    if (!isDragging.value) return
    percent.value = options.getPercent(event)
    options.onMove?.(percent.value)
  }

  const onDragEnd = (event: MouseEvent): void => {
    if (!isDragging.value) return
    isDragging.value = false
    percent.value = options.getPercent(event)
    options.onEnd?.(percent.value)
    removeListeners()
  }

  // 指针取消（来电、手势区滑动、系统接管滚动等）：不应用最终取值，
  // 但要干净地退出拖拽态，否则下一次点击会被当成"正在拖拽"
  const onDragCancel = (): void => {
    if (!isDragging.value) return
    isDragging.value = false
    removeListeners()
  }

  const startDrag = (event: MouseEvent): void => {
    // 多指同时按住同一个滑块时只认第一根手指，避免两指互相拉扯把取值拽来拽去。
    // （只针对 touch：程序合成的 pointerdown 不带 pointerType，不能被误伤）
    const pointer = event as PointerEvent
    if (pointer.pointerType === 'touch' && pointer.isPrimary === false) return

    percent.value = options.getPercent(event)
    if (options.onStart?.(percent.value, event) === false) return
    isDragging.value = true
    removeListeners() // 防御性去重,避免重复挂载
    document.addEventListener('pointermove', onDragMove)
    document.addEventListener('pointerup', onDragEnd)
    document.addEventListener('pointercancel', onDragCancel)
  }

  const stopDrag = (): void => {
    isDragging.value = false
    removeListeners()
  }

  onUnmounted(stopDrag)

  return { isDragging, percent, getPercent: options.getPercent, startDrag, stopDrag }
}
