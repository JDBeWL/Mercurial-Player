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

/**
 * 按 "pointerdown -> document pointermove/pointerup" 模式拖拽取值的骨架。
 * 统一了各滑块组件手写的监听器挂载/清理样板;
 * document 级监听保证指针移出滑块后拖拽不中断,并在组件卸载时自动清理。
 *
 * 之所以用 Pointer Events 而不是 mouse events：Android WebView 上手指拖动
 * **不会**产生 `mousemove`（触摸只在按下/抬起时合成少量鼠标事件），
 * 结果就是进度条、音量条、EQ 滑块在手机上"点一下能动、拖着走不动"。
 * PointerEvent 统一了鼠标/触摸/触控笔三种输入，且天然携带 `clientX/Y`
 * （签名仍按 `MouseEvent` 收，因为 PointerEvent 继承自 MouseEvent，
 * 这样 hover 路径传进来的真 MouseEvent 也能直接复用同一个测算函数）。
 *
 * 配套要求：调用方必须在滑块元素上写 `touch-action: none`，
 * 否则浏览器会把这次拖拽判成滚动手势，中途抛 pointercancel 打断拖拽。
 *
 * 注意: getPercent 收到的 document 级事件没有 currentTarget,
 * 需要通过元素 ref 或闭包定位滑块几何。
 */
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
