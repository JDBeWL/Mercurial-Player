// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { validateThemeContrast, setupThemeContrastValidation } from '@/utils/themeContrastValidator'

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

describe('validateThemeContrast', () => {
  let originalGetComputedStyle: any

  beforeEach(() => {
    originalGetComputedStyle = window.getComputedStyle
  })

  afterEach(() => {
    window.getComputedStyle = originalGetComputedStyle
    vi.restoreAllMocks()
  })

  it('should return warnings when CSS variables are not set', () => {
    window.getComputedStyle = () => ({
      getPropertyValue: () => '',
    }) as any

    const result = validateThemeContrast()

    expect(result.warnings.length).toBeGreaterThan(0)
    expect(result.passed).toHaveLength(0)
    expect(result.failed).toHaveLength(0)
  })

  it('should pass for high contrast colors', () => {
    window.getComputedStyle = () => ({
      getPropertyValue: (varName: string) => {
        const colors: Record<string, string> = {
          '--md-sys-color-on-surface': '#000000',
          '--md-sys-color-background': '#ffffff',
          '--md-sys-color-on-surface-variant': '#000000',
          '--md-sys-color-surface': '#ffffff',
          '--md-sys-color-on-background': '#000000',
          '--md-sys-color-on-primary-container': '#000000',
          '--md-sys-color-primary-container': '#ffffff',
          '--md-sys-color-on-secondary-container': '#000000',
          '--md-sys-color-secondary-container': '#ffffff',
          '--md-sys-color-on-error-container': '#000000',
          '--md-sys-color-error-container': '#ffffff',
          '--md-sys-color-on-primary': '#000000',
          '--md-sys-color-primary': '#0000ff',
        }
        return colors[varName] || ''
      },
    }) as any

    const result = validateThemeContrast()

    expect(result.passed.length).toBeGreaterThan(0)
  })

  it('should fail for low contrast required pairs', () => {
    window.getComputedStyle = () => ({
      getPropertyValue: (varName: string) => {
        const colors: Record<string, string> = {
          '--md-sys-color-on-surface': '#777777',
          '--md-sys-color-background': '#888888',
          '--md-sys-color-on-surface-variant': '#777777',
          '--md-sys-color-surface': '#888888',
          '--md-sys-color-on-background': '#777777',
          '--md-sys-color-on-primary-container': '#777777',
          '--md-sys-color-primary-container': '#888888',
          '--md-sys-color-on-secondary-container': '#777777',
          '--md-sys-color-secondary-container': '#888888',
          '--md-sys-color-on-error-container': '#777777',
          '--md-sys-color-error-container': '#888888',
          '--md-sys-color-on-primary': '#777777',
          '--md-sys-color-primary': '#888888',
        }
        return colors[varName] || ''
      },
    }) as any

    const result = validateThemeContrast()

    expect(result.failed.length).toBeGreaterThan(0)
  })

  it('should warn for low contrast optional pairs', () => {
    window.getComputedStyle = () => ({
      getPropertyValue: (varName: string) => {
        const colors: Record<string, string> = {
          '--md-sys-color-on-surface': '#000000',
          '--md-sys-color-background': '#ffffff',
          '--md-sys-color-on-surface-variant': '#000000',
          '--md-sys-color-surface': '#ffffff',
          '--md-sys-color-on-background': '#000000',
          '--md-sys-color-on-primary-container': '#000000',
          '--md-sys-color-primary-container': '#ffffff',
          '--md-sys-color-on-secondary-container': '#000000',
          '--md-sys-color-secondary-container': '#ffffff',
          '--md-sys-color-on-error-container': '#000000',
          '--md-sys-color-error-container': '#ffffff',
          '--md-sys-color-on-primary': '#777777',
          '--md-sys-color-primary': '#888888',
        }
        return colors[varName] || ''
      },
    }) as any

    const result = validateThemeContrast()

    // Optional pairs with low contrast should go to warnings, not failed
    const onPrimaryWarning = result.warnings.find(w => w.name?.includes('On Primary on Primary'))
    expect(onPrimaryWarning).toBeDefined()
  })

  it('should handle rgb colors from CSS', () => {
    window.getComputedStyle = () => ({
      getPropertyValue: (varName: string) => {
        const colors: Record<string, string> = {
          '--md-sys-color-on-surface': 'rgb(0, 0, 0)',
          '--md-sys-color-background': 'rgb(255, 255, 255)',
          '--md-sys-color-on-surface-variant': 'rgb(0, 0, 0)',
          '--md-sys-color-surface': 'rgb(255, 255, 255)',
          '--md-sys-color-on-background': 'rgb(0, 0, 0)',
          '--md-sys-color-on-primary-container': 'rgb(0, 0, 0)',
          '--md-sys-color-primary-container': 'rgb(255, 255, 255)',
          '--md-sys-color-on-secondary-container': 'rgb(0, 0, 0)',
          '--md-sys-color-secondary-container': 'rgb(255, 255, 255)',
          '--md-sys-color-on-error-container': 'rgb(0, 0, 0)',
          '--md-sys-color-error-container': 'rgb(255, 255, 255)',
          '--md-sys-color-on-primary': 'rgb(0, 0, 0)',
          '--md-sys-color-primary': 'rgb(255, 255, 255)',
        }
        return colors[varName] || ''
      },
    }) as any

    const result = validateThemeContrast()

    expect(result.passed.length).toBeGreaterThan(0)
  })
})

describe('setupThemeContrastValidation', () => {
  it('should not throw when window is undefined', () => {
    const originalWindow = globalThis.window
    // @ts-ignore
    globalThis.window = undefined

    expect(() => setupThemeContrastValidation()).not.toThrow()

    globalThis.window = originalWindow
  })

  it('should setup mutation observer', () => {
    const observeSpy = vi.fn()
    class MockMutationObserver {
      observe = observeSpy
    }
    globalThis.MutationObserver = MockMutationObserver as any

    setupThemeContrastValidation()

    expect(observeSpy).toHaveBeenCalledWith(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-theme', 'style'],
    })
  })
})
