import { onUnmounted, ref, type Ref } from 'vue'

export interface UseDragValueOptions {
  /** 由鼠标事件计算 0..1 的拖拽百分比 (方向与几何由调用方决定) */
  getPercent: (event: MouseEvent) => number
  /** mousedown 时调用;返回 false 取消本次拖拽 (不进入拖拽态、不挂监听器) */
  onStart?: (percent: number, event: MouseEvent) => boolean | void
  /** 拖拽移动时调用 */
  onMove?: (percent: number) => void
  /** mouseup 时调用 (仅当拖拽进行中) */
  onEnd?: (percent: number) => void
}

export interface UseDragValueResult {
  isDragging: Ref<boolean>
  /** 最近一次事件的拖拽百分比 (0..1) */
  percent: Ref<number>
  /** 用调用方的 getPercent 计算任意事件的百分比 (供 click 等非拖拽路径复用) */
  getPercent: (event: MouseEvent) => number
  /** 绑定到滑块元素的 @mousedown */
  startDrag: (event: MouseEvent) => void
  /** 强制结束拖拽并移除监听器 */
  stopDrag: () => void
}

/**
 * 按 "mousedown -> document mousemove/mouseup" 模式拖拽取值的骨架。
 * 统一了各滑块组件手写的监听器挂载/清理样板;
 * document 级监听保证鼠标移出滑块后拖拽不中断,并在组件卸载时自动清理。
 *
 * 注意: getPercent 收到的 document 级事件没有 currentTarget,
 * 需要通过元素 ref 或闭包定位滑块几何。
 */
export function useDragValue(options: UseDragValueOptions): UseDragValueResult {
  const isDragging = ref(false)
  const percent = ref(0)

  const removeListeners = (): void => {
    document.removeEventListener('mousemove', onDragMove)
    document.removeEventListener('mouseup', onDragEnd)
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

  const startDrag = (event: MouseEvent): void => {
    percent.value = options.getPercent(event)
    if (options.onStart?.(percent.value, event) === false) return
    isDragging.value = true
    removeListeners() // 防御性去重,避免重复挂载
    document.addEventListener('mousemove', onDragMove)
    document.addEventListener('mouseup', onDragEnd)
  }

  const stopDrag = (): void => {
    isDragging.value = false
    removeListeners()
  }

  onUnmounted(stopDrag)

  return { isDragging, percent, getPercent: options.getPercent, startDrag, stopDrag }
}
