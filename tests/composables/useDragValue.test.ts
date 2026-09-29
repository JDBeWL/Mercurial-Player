// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import type { UseDragValueOptions } from '@/composables/useDragValue'

const { useDragValue } = await import('@/composables/useDragValue')

const PERCENT_SCALE = 100

const mounted: ReturnType<typeof mount>[] = []

const mountDrag = (options: Partial<UseDragValueOptions> = {}) => {
  let api!: ReturnType<typeof useDragValue>
  const wrapper = mount(
    defineComponent({
      setup() {
        api = useDragValue({
          getPercent: (event: MouseEvent) => event.clientX / PERCENT_SCALE,
          ...options,
        })
        return () => null
      },
    }),
  )
  mounted.push(wrapper)
  return {
    wrapper,
    get api() {
      return api
    },
  }
}

/** 构造一个指针事件；鼠标/触摸/触控笔共用同一种事件类型 */
const pointer = (type: string, clientX: number, init: PointerEventInit = {}) =>
  document.dispatchEvent(new PointerEvent(type, { clientX, bubbles: true, ...init }))

beforeEach(() => {
  mounted.length = 0
})

afterEach(() => {
  while (mounted.length > 0) mounted.pop()?.unmount()
})

describe('useDragValue', () => {
  it('starts idle at zero percent', () => {
    const { api } = mountDrag()
    expect(api.isDragging.value).toBe(false)
    expect(api.percent.value).toBe(0)
  })

  it('enters the dragging state on pointerdown', () => {
    const onStart = vi.fn()
    const { api } = mountDrag({ onStart })

    api.startDrag(new PointerEvent('pointerdown', { clientX: 25 }))

    expect(api.isDragging.value).toBe(true)
    expect(api.percent.value).toBe(0.25)
    expect(onStart).toHaveBeenCalledWith(0.25, expect.any(MouseEvent))
  })

  it('aborts the drag when onStart returns false', () => {
    const { api } = mountDrag({ onStart: () => false })

    api.startDrag(new PointerEvent('pointerdown', { clientX: 25 }))

    expect(api.isDragging.value).toBe(false)

    pointer('pointermove', 80)
    // 监听器未挂载,移动不应被处理
    expect(api.percent.value).toBe(0.25)
  })

  it('tracks the pointer while dragging', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    pointer('pointermove', 60)

    expect(api.percent.value).toBe(0.6)
    expect(onMove).toHaveBeenCalledWith(0.6)
  })

  it('ignores pointer movement when the drag never started', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })

    pointer('pointermove', 60)

    expect(api.percent.value).toBe(0)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('keeps tracking after the pointer leaves the slider element', () => {
    // document 级监听:鼠标移出滑块范围也应继续拖拽
    const { api } = mountDrag()
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    const outside = document.createElement('div')
    document.body.appendChild(outside)
    outside.dispatchEvent(new PointerEvent('pointermove', { clientX: 90, bubbles: true }))

    expect(api.percent.value).toBe(0.9)
  })

  it('finishes the drag on pointerup', () => {
    const onEnd = vi.fn()
    const { api } = mountDrag({ onEnd })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    pointer('pointerup', 75)

    expect(api.isDragging.value).toBe(false)
    expect(api.percent.value).toBe(0.75)
    expect(onEnd).toHaveBeenCalledWith(0.75)
  })

  it('removes the document listeners after pointerup', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))
    pointer('pointerup', 75)
    onMove.mockClear()

    pointer('pointermove', 20)

    expect(onMove).not.toHaveBeenCalled()
  })

  it('ignores a pointerup that was not preceded by a drag', () => {
    const onEnd = vi.fn()
    const { api } = mountDrag({ onEnd })

    pointer('pointerup', 75)

    expect(onEnd).not.toHaveBeenCalled()
    expect(api.isDragging.value).toBe(false)
  })

  it('does not stack duplicate listeners when started twice', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))
    api.startDrag(new PointerEvent('pointerdown', { clientX: 20 }))

    pointer('pointermove', 50)

    expect(onMove).toHaveBeenCalledTimes(1)
  })

  it('stopDrag ends the drag without firing onEnd', () => {
    const onEnd = vi.fn()
    const onMove = vi.fn()
    const { api } = mountDrag({ onEnd, onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    api.stopDrag()

    expect(api.isDragging.value).toBe(false)
    expect(onEnd).not.toHaveBeenCalled()

    pointer('pointermove', 50)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('exposes the caller-supplied getPercent for non-drag paths', () => {
    const getPercent = vi.fn((event: MouseEvent) => event.clientX / PERCENT_SCALE)
    const { api } = mountDrag({ getPercent })

    expect(api.getPercent(new PointerEvent('click', { clientX: 40 }))).toBe(0.4)
    expect(getPercent).toHaveBeenCalled()
  })

  it('cleans up automatically when the component unmounts', () => {
    const onMove = vi.fn()
    const { api, wrapper } = mountDrag({ onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    wrapper.unmount()

    expect(api.isDragging.value).toBe(false)
    pointer('pointermove', 50)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('reports a clamped-out-of-range percent straight from the caller', () => {
    // 百分比计算完全由调用方决定,这里验证原样透传(含越界值)
    const { api } = mountDrag({
      getPercent: (event: MouseEvent) => event.clientX / PERCENT_SCALE,
    })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 250 }))

    expect(api.percent.value).toBe(2.5)
  })

  it('aborts cleanly on pointercancel without applying the value', () => {
    // 安卓上手指手势被系统接管（回滚滚动/来电）会抛 pointercancel，
    // 此时既不能提交取值，也必须退出拖拽态
    const onEnd = vi.fn()
    const onMove = vi.fn()
    const { api } = mountDrag({ onEnd, onMove })
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    pointer('pointercancel', 90)

    expect(api.isDragging.value).toBe(false)
    expect(onEnd).not.toHaveBeenCalled()

    pointer('pointermove', 40)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('ignores a secondary touch finger on the same slider', () => {
    const { api } = mountDrag()
    api.startDrag(new PointerEvent('pointerdown', { clientX: 10 }))

    // 第二根手指：isPrimary 为 false
    const second = new PointerEvent('pointerdown', {
      clientX: 90,
      pointerType: 'touch',
      isPrimary: false,
    })
    api.startDrag(second)

    expect(api.percent.value).toBe(0.1)
  })
})
