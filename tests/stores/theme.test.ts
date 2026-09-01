// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const enforceThemeContrast = vi.fn(() => 0)
const validateThemeContrast = vi.fn(() => ({ passed: [], failed: [], warnings: [] }))
const saveConfigNow = vi.fn().mockResolvedValue(undefined)

vi.mock('@/utils/themeContrastValidator', () => ({
  enforceThemeContrast,
  validateThemeContrast,
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

vi.mock('@/stores/config', () => ({
  useConfigStore: () => ({
    general: { theme: 'auto' },
    saveConfigNow,
  }),
}))

const { useThemeStore } = await import('@/stores/theme')

/** 控制系统 prefers-color-scheme 的返回值 */
let prefersDark = false
function stubMatchMedia() {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      matches: query.includes('dark') ? prefersDark : false,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
      onchange: null,
    }),
  })
}

const rootStyle = () => document.documentElement.style
const customCssText = () => document.getElementById('theme-custom-variables')?.textContent ?? ''

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  prefersDark = false
  stubMatchMedia()
  document.documentElement.removeAttribute('data-theme')
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useThemeStore initial state', () => {
  it('starts in light mode with auto preference', () => {
    const store = useThemeStore()
    expect(store.isDarkMode).toBe(false)
    expect(store.isDark).toBe(false)
    expect(store.themePreference).toBe('auto')
    expect(store.primaryColor).toBe('#2C2C2C')
    expect(store.enableGlassEffect).toBe(true)
    expect(store.enableGradients).toBe(true)
    expect(store.immersiveThemeOverride).toBe(false)
  })
})

describe('toggleDarkMode', () => {
  it('flips the mode and applies the theme', () => {
    const store = useThemeStore()

    store.toggleDarkMode()

    expect(store.isDarkMode).toBe(true)
    expect(store.isDark).toBe(true)
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')
  })

  it('keeps the auto preference untouched', () => {
    const store = useThemeStore()

    return store.toggleDarkMode().then(() => {
      expect(store.themePreference).toBe('auto')
      expect(saveConfigNow).toHaveBeenCalled()
    })
  })

  it('maps an explicit preference to the new mode', () => {
    const store = useThemeStore()
    store.themePreference = 'light'

    return store.toggleDarkMode().then(() => {
      expect(store.isDarkMode).toBe(true)
      expect(store.themePreference).toBe('dark')
    })
  })

  it('toggles back to light and updates the preference accordingly', () => {
    const store = useThemeStore()
    store.themePreference = 'dark'
    store.isDarkMode = true

    return store.toggleDarkMode().then(() => {
      expect(store.isDarkMode).toBe(false)
      expect(store.themePreference).toBe('light')
    })
  })
})

describe('setPrimaryColor / setPrimaryColorLive', () => {
  it('applies the colour immediately and persists it', () => {
    const store = useThemeStore()

    return store.setPrimaryColor('#ff0000').then(() => {
      expect(store.primaryColor).toBe('#ff0000')
      // hex 写入 themePreference 即表示自定义色模式
      expect(store.themePreference).toBe('#ff0000')
      expect(saveConfigNow).toHaveBeenCalledTimes(1)
    })
  })

  it('updates live without touching the config', () => {
    const store = useThemeStore()

    store.setPrimaryColorLive('#00ff00')

    expect(store.primaryColor).toBe('#00ff00')
    expect(saveConfigNow).not.toHaveBeenCalled()
  })
})

describe('setThemePreference', () => {
  it('follows the system when auto is selected', () => {
    const store = useThemeStore()
    prefersDark = true

    store.setThemePreference('auto')

    expect(store.isDarkMode).toBe(true)
    expect(store.primaryColor).toBe('#2C2C2C')
  })

  it('follows the system when auto is selected and the system is light', () => {
    const store = useThemeStore()
    prefersDark = false

    store.setThemePreference('auto')

    expect(store.isDarkMode).toBe(false)
  })

  it('forces light mode', () => {
    const store = useThemeStore()
    store.isDarkMode = true

    store.setThemePreference('light')

    expect(store.isDarkMode).toBe(false)
  })

  it('forces dark mode', () => {
    const store = useThemeStore()

    store.setThemePreference('dark')

    expect(store.isDarkMode).toBe(true)
  })

  it('treats a hex string as a custom colour', () => {
    const store = useThemeStore()

    store.setThemePreference('#123456')

    expect(store.primaryColor).toBe('#123456')
  })

  it('applies the theme for every branch', () => {
    const store = useThemeStore()

    for (const preference of ['auto', 'light', 'dark', '#abcdef'] as const) {
      store.setThemePreference(preference)
      expect(enforceThemeContrast).toHaveBeenCalled()
    }
  })
})

describe('setImmersiveDarkMode', () => {
  it('does nothing when clearing an override that was never set', () => {
    const store = useThemeStore()

    store.setImmersiveDarkMode(null)

    expect(store.immersiveThemeOverride).toBe(false)
    expect(store.isDarkMode).toBe(false)
  })

  it('sets the override and switches to dark', () => {
    const store = useThemeStore()

    store.setImmersiveDarkMode(true)

    expect(store.immersiveThemeOverride).toBe(true)
    expect(store.isDarkMode).toBe(true)
  })

  it('skips re-applying when the mode already matches', () => {
    const store = useThemeStore()
    store.isDarkMode = false
    enforceThemeContrast.mockClear()

    store.setImmersiveDarkMode(false)

    expect(enforceThemeContrast).not.toHaveBeenCalled()
  })

  it('restores the dark preference when the override is cleared', () => {
    const store = useThemeStore()
    store.themePreference = 'dark'
    store.setImmersiveDarkMode(false)

    store.setImmersiveDarkMode(null)

    expect(store.immersiveThemeOverride).toBe(false)
    expect(store.isDarkMode).toBe(true)
  })

  it('restores the light preference when the override is cleared', () => {
    const store = useThemeStore()
    store.themePreference = 'light'
    store.setImmersiveDarkMode(true)

    store.setImmersiveDarkMode(null)

    expect(store.isDarkMode).toBe(false)
  })

  it('falls back to the system preference for auto', () => {
    const store = useThemeStore()
    store.themePreference = 'auto'
    prefersDark = true
    store.setImmersiveDarkMode(false)

    store.setImmersiveDarkMode(null)

    expect(store.isDarkMode).toBe(true)
  })
})

describe('setGlassEffect / setGradients', () => {
  it('writes glass variables when enabled', () => {
    const store = useThemeStore()

    store.setGlassEffect(true)

    const css = customCssText()
    expect(css).toContain('--glass-blur: 12px;')
    expect(css).toContain('--glass-border: 1px solid')
  })

  it('disables the blur when glass is off', () => {
    const store = useThemeStore()

    store.setGlassEffect(false)

    expect(customCssText()).toContain('--glass-blur: 0px;')
  })

  it('writes gradients when enabled and none when disabled', () => {
    const store = useThemeStore()

    store.setGradients(true)
    expect(customCssText()).toContain('--gradient-primary: linear-gradient(')

    store.setGradients(false)
    expect(customCssText()).toContain('--gradient-primary: none;')
  })
})

describe('applyTheme', () => {
  it('emits the MD3 palette, tonal variants and harmony colours', () => {
    const store = useThemeStore()

    store.applyTheme()

    const css = customCssText()
    expect(css).toContain('--theme-source-color: #2C2C2C;')
    expect(css).toContain('--theme-primary-tone40:')
    expect(css).toContain('--theme-primary-tone100:')
    expect(css).toContain('--theme-complementary:')
    expect(css).toContain('--theme-analogous-1:')
    expect(css).toContain('--theme-analogous-2:')
    expect(css).toContain('--theme-triadic-1:')
    expect(css).toContain('--theme-triadic-2:')
    expect(css).toContain('--theme-accent-tone50:')
    expect(css).toContain('--primary-alpha-20:')
  })

  it('picks a black on-primary for light source colours', () => {
    const store = useThemeStore()

    store.setPrimaryColorLive('#ffee00')

    expect(customCssText()).toContain('--theme-on-primary: #000000;')
  })

  it('picks a white on-primary for dark source colours', () => {
    const store = useThemeStore()

    store.setPrimaryColorLive('#101040')

    expect(customCssText()).toContain('--theme-on-primary: #ffffff;')
  })

  it('uses heavier shadows in dark mode', () => {
    const store = useThemeStore()

    store.setThemePreference('dark')

    expect(customCssText()).toContain('rgba(0, 0, 0, 0.5)')
  })

  it('uses lighter shadows in light mode', () => {
    const store = useThemeStore()

    store.setThemePreference('light')

    expect(customCssText()).toContain('rgba(0, 0, 0, 0.08)')
  })

  it('overrides the MD3 palette with true greys for low-chroma colours', () => {
    const store = useThemeStore()

    store.setPrimaryColorLive('#2C2C2C')

    expect(rootStyle().getPropertyValue('--md-sys-color-primary').trim()).toMatch(/^#/)
    expect(rootStyle().getPropertyValue('--md-sys-color-surface').trim()).toMatch(/^#/)
  })

  it('sets the data-theme attribute for both modes', () => {
    const store = useThemeStore()

    store.setThemePreference('dark')
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark')

    store.setThemePreference('light')
    expect(document.documentElement.getAttribute('data-theme')).toBe('light')
  })

  it('reuses the cached CSS for repeated identical themes', () => {
    const store = useThemeStore()
    store.setPrimaryColorLive('#123456')
    const first = customCssText()
    const element = document.getElementById('theme-custom-variables')

    store.applyTheme()
    store.applyTheme()

    // 同一个 <style> 元素被复用,内容由缓存直接得出
    expect(document.getElementById('theme-custom-variables')).toBe(element)
    expect(customCssText()).toBe(first)
  })

  it('regenerates the CSS when a theming input changes', () => {
    const store = useThemeStore()
    store.setPrimaryColorLive('#123456')
    const before = customCssText()

    store.setGlassEffect(false)

    expect(customCssText()).not.toBe(before)
  })

  it('runs the contrast enforcement and debounces the diagnostic validation', () => {
    vi.useFakeTimers()
    const store = useThemeStore()

    store.applyTheme()
    expect(enforceThemeContrast).toHaveBeenCalledTimes(1)
    expect(validateThemeContrast).not.toHaveBeenCalled()

    vi.advanceTimersByTime(300)
    expect(validateThemeContrast).toHaveBeenCalledTimes(1)
    expect(validateThemeContrast).toHaveBeenCalledWith(false)
  })

  it('only keeps the last validation when the theme changes rapidly', () => {
    vi.useFakeTimers()
    const store = useThemeStore()

    store.setPrimaryColorLive('#111111')
    store.setPrimaryColorLive('#222222')
    store.setPrimaryColorLive('#333333')
    vi.advanceTimersByTime(300)

    expect(validateThemeContrast).toHaveBeenCalledTimes(1)
  })
})

describe('saveThemeToConfig', () => {
  it('writes the preference into the config store and saves', () => {
    const store = useThemeStore()
    store.themePreference = '#ff0000'

    return store.saveThemeToConfig().then(() => {
      expect(saveConfigNow).toHaveBeenCalledTimes(1)
    })
  })

  it('swallows config failures', async () => {
    saveConfigNow.mockRejectedValueOnce(new Error('disk full'))
    const store = useThemeStore()
    const logger = (await import('@/utils/logger')).default

    await expect(store.saveThemeToConfig()).resolves.toBeUndefined()
    expect(logger.error).toHaveBeenCalledWith('Failed to save theme to config:', expect.any(Error))
  })
})
