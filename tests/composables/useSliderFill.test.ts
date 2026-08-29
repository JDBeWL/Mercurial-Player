import { describe, expect, it } from 'vitest'
import { ref } from 'vue'
import { useSliderFill } from '@/composables/useSliderFill'

describe('useSliderFill', () => {
  it('computes percentage from a plain value', () => {
    const fill = useSliderFill(0, 100, 42)
    expect(fill.value).toBe(42)
  })

  it('tracks reactive value changes', () => {
    const value = ref(0.5)
    const fill = useSliderFill(0, 1, value)
    expect(fill.value).toBe(50)
    value.value = 1
    expect(fill.value).toBe(100)
  })

  it('supports getter input', () => {
    const fill = useSliderFill(-8, 8, () => 0)
    expect(fill.value).toBe(50)
  })
})
