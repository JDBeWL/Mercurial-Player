import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  getContrastRatio,
  checkContrast,
  adjustColorForContrast,
  getColorFromCSSVar,
} from '@/utils/colorContrast'

describe('colorContrast', () => {
  describe('getContrastRatio', () => {
    it('should return 21 for black and white', () => {
      const ratio = getContrastRatio('#000000', '#ffffff')
      expect(ratio).toBeCloseTo(21, 0)
    })

    it('should return 1 for same colors', () => {
      const ratio = getContrastRatio('#ff0000', '#ff0000')
      expect(ratio).toBe(1)
    })

    it('should handle 3-digit hex colors', () => {
      const ratio = getContrastRatio('#000', '#fff')
      expect(ratio).toBeCloseTo(21, 0)
    })

    it('should handle colors with # prefix', () => {
      const ratio = getContrastRatio('#000000', '#ffffff')
      expect(ratio).toBeCloseTo(21, 0)
    })
  })

  describe('checkContrast', () => {
    it('should pass AA for black on white', () => {
      const result = checkContrast('#000000', '#ffffff', { level: 'AA' })
      expect(result.pass).toBe(true)
      expect(result.ratio).toBeCloseTo(21, 0)
    })

    it('should fail AA for low contrast colors', () => {
      const result = checkContrast('#777777', '#888888', { level: 'AA' })
      expect(result.pass).toBe(false)
    })

    it('should use different thresholds for large text', () => {
      const normalResult = checkContrast('#666666', '#ffffff', { level: 'AA', largeText: false })
      const largeResult = checkContrast('#666666', '#ffffff', { level: 'AA', largeText: true })

      // Large text has lower requirements
      expect(largeResult.requiredRatio).toBeLessThan(normalResult.requiredRatio)
    })

    it('should include descriptive message', () => {
      const result = checkContrast('#000000', '#ffffff')
      expect(result.message).toContain('WCAG')
    })
  })

  describe('adjustColorForContrast', () => {
    it('should return original color if already passing', () => {
      const result = adjustColorForContrast('#000000', '#ffffff')
      expect(result).toBe('#000000')
    })

    it('should darken color when foreground is lighter than background', () => {
      // Light gray on white - should darken to increase contrast
      const adjusted = adjustColorForContrast('#aaaaaa', '#ffffff', { level: 'AA' })
      const check = checkContrast(adjusted, '#ffffff', { level: 'AA' })
      expect(check.pass).toBe(true)
    })

    it('should lighten color when foreground is darker than background', () => {
      // Dark gray on black - should lighten to increase contrast
      const adjusted = adjustColorForContrast('#333333', '#000000', { level: 'AA' })
      const check = checkContrast(adjusted, '#000000', { level: 'AA' })
      expect(check.pass).toBe(true)
    })

    it('should handle AAA level requirement', () => {
      const adjusted = adjustColorForContrast('#888888', '#ffffff', { level: 'AAA' })
      const check = checkContrast(adjusted, '#ffffff', { level: 'AAA' })
      expect(check.pass).toBe(true)
    })

    it('should handle large text option', () => {
      const adjusted = adjustColorForContrast('#999999', '#ffffff', {
        level: 'AA',
        largeText: true,
      })
      const check = checkContrast(adjusted, '#ffffff', { level: 'AA', largeText: true })
      expect(check.pass).toBe(true)
    })

    it('should respect custom step size', () => {
      const adjusted = adjustColorForContrast('#888888', '#ffffff', { step: 0.1 })
      const check = checkContrast(adjusted, '#ffffff')
      expect(check.pass).toBe(true)
    })

    it('should handle dark background correctly', () => {
      // #2C2C2C on dark background should lighten
      const adjusted = adjustColorForContrast('#2c2c2c', '#1a1a1a', { level: 'AA' })
      const check = checkContrast(adjusted, '#1a1a1a', { level: 'AA' })
      expect(check.pass).toBe(true)
    })
  })
})

describe('getColorFromCSSVar', () => {
  beforeEach(() => {
    // Mock window and document for CSS variable tests
    vi.stubGlobal('window', {})
    vi.stubGlobal('document', {
      documentElement: {},
    })
  })

  it('should return null when window is undefined', () => {
    vi.stubGlobal('window', undefined)

    const result = getColorFromCSSVar('--test-color')

    expect(result).toBeNull()
  })

  it('should return null for empty CSS variable', () => {
    vi.stubGlobal('getComputedStyle', () => ({
      getPropertyValue: () => '',
    }))

    const result = getColorFromCSSVar('--empty-var')

    expect(result).toBeNull()
  })

  it('should return hex color directly', () => {
    vi.stubGlobal('getComputedStyle', () => ({
      getPropertyValue: () => '#ff0000',
    }))

    const result = getColorFromCSSVar('--red')

    expect(result).toBe('#ff0000')
  })

  it('should convert rgb to hex', () => {
    vi.stubGlobal('getComputedStyle', () => ({
      getPropertyValue: () => 'rgb(255, 0, 0)',
    }))

    const result = getColorFromCSSVar('--red')

    expect(result).toBe('#ff0000')
  })

  it('should convert rgba to hex', () => {
    vi.stubGlobal('getComputedStyle', () => ({
      getPropertyValue: () => 'rgba(0, 255, 0, 1)',
    }))

    const result = getColorFromCSSVar('--green')

    expect(result).toBe('#00ff00')
  })

  it('should return null for non-hex non-rgb values', () => {
    vi.stubGlobal('getComputedStyle', () => ({
      getPropertyValue: () => 'red',
    }))

    const result = getColorFromCSSVar('--named-color')

    expect(result).toBeNull()
  })
})
