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

const mouse = (type: string, clientX: number) =>
  document.dispatchEvent(new MouseEvent(type, { clientX, bubbles: true }))

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

  it('enters the dragging state on mousedown', () => {
    const onStart = vi.fn()
    const { api } = mountDrag({ onStart })

    api.startDrag(new MouseEvent('mousedown', { clientX: 25 }))

    expect(api.isDragging.value).toBe(true)
    expect(api.percent.value).toBe(0.25)
    expect(onStart).toHaveBeenCalledWith(0.25, expect.any(MouseEvent))
  })

  it('aborts the drag when onStart returns false', () => {
    const { api } = mountDrag({ onStart: () => false })

    api.startDrag(new MouseEvent('mousedown', { clientX: 25 }))

    expect(api.isDragging.value).toBe(false)

    mouse('mousemove', 80)
    // 监听器未挂载,移动不应被处理
    expect(api.percent.value).toBe(0.25)
  })

  it('tracks the pointer while dragging', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))

    mouse('mousemove', 60)

    expect(api.percent.value).toBe(0.6)
    expect(onMove).toHaveBeenCalledWith(0.6)
  })

  it('ignores pointer movement when the drag never started', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })

    mouse('mousemove', 60)

    expect(api.percent.value).toBe(0)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('keeps tracking after the pointer leaves the slider element', () => {
    // document 级监听:鼠标移出滑块范围也应继续拖拽
    const { api } = mountDrag()
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))

    const outside = document.createElement('div')
    document.body.appendChild(outside)
    outside.dispatchEvent(new MouseEvent('mousemove', { clientX: 90, bubbles: true }))

    expect(api.percent.value).toBe(0.9)
  })

  it('finishes the drag on mouseup', () => {
    const onEnd = vi.fn()
    const { api } = mountDrag({ onEnd })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))

    mouse('mouseup', 75)

    expect(api.isDragging.value).toBe(false)
    expect(api.percent.value).toBe(0.75)
    expect(onEnd).toHaveBeenCalledWith(0.75)
  })

  it('removes the document listeners after mouseup', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))
    mouse('mouseup', 75)
    onMove.mockClear()

    mouse('mousemove', 20)

    expect(onMove).not.toHaveBeenCalled()
  })

  it('ignores a mouseup that was not preceded by a drag', () => {
    const onEnd = vi.fn()
    const { api } = mountDrag({ onEnd })

    mouse('mouseup', 75)

    expect(onEnd).not.toHaveBeenCalled()
    expect(api.isDragging.value).toBe(false)
  })

  it('does not stack duplicate listeners when started twice', () => {
    const onMove = vi.fn()
    const { api } = mountDrag({ onMove })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))
    api.startDrag(new MouseEvent('mousedown', { clientX: 20 }))

    mouse('mousemove', 50)

    expect(onMove).toHaveBeenCalledTimes(1)
  })

  it('stopDrag ends the drag without firing onEnd', () => {
    const onEnd = vi.fn()
    const onMove = vi.fn()
    const { api } = mountDrag({ onEnd, onMove })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))

    api.stopDrag()

    expect(api.isDragging.value).toBe(false)
    expect(onEnd).not.toHaveBeenCalled()

    mouse('mousemove', 50)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('exposes the caller-supplied getPercent for non-drag paths', () => {
    const getPercent = vi.fn((event: MouseEvent) => event.clientX / PERCENT_SCALE)
    const { api } = mountDrag({ getPercent })

    expect(api.getPercent(new MouseEvent('click', { clientX: 40 }))).toBe(0.4)
    expect(getPercent).toHaveBeenCalled()
  })

  it('cleans up automatically when the component unmounts', () => {
    const onMove = vi.fn()
    const { api, wrapper } = mountDrag({ onMove })
    api.startDrag(new MouseEvent('mousedown', { clientX: 10 }))

    wrapper.unmount()

    expect(api.isDragging.value).toBe(false)
    mouse('mousemove', 50)
    expect(onMove).not.toHaveBeenCalled()
  })

  it('reports a clamped-out-of-range percent straight from the caller', () => {
    // 百分比计算完全由调用方决定,这里验证原样透传(含越界值)
    const { api } = mountDrag({
      getPercent: (event: MouseEvent) => event.clientX / PERCENT_SCALE,
    })
    api.startDrag(new MouseEvent('mousedown', { clientX: 250 }))

    expect(api.percent.value).toBe(2.5)
  })
})
