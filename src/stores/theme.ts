import { defineStore } from 'pinia'
import {
  argbFromHex,
  hexFromArgb,
  themeFromSourceColor,
  applyTheme,
  TonalPalette,
  Hct,
} from '@material/material-color-utilities'
import logger from '../utils/logger'
import { validateThemeContrast } from '../utils/themeContrastValidator'
import { useConfigStore } from './config'
import type { TonalVariants, HarmonyColors, ThemePreference } from '@/types'

// 缓存已生成的主题样式
const customStyleCache = new Map<string, string>()
let customStyleElement: HTMLStyleElement | null = null

// 检测是否为中性灰色（低饱和度）
function isNeutralGray(hexColor: string): boolean {
  const argb = argbFromHex(hexColor)
  const hct = Hct.fromInt(argb)
  // MD3 的 neutral palette 使用 chroma 4-8，低于 15 视为灰色
  return hct.chroma < 15
}

// 辅助函数：从 HEX 颜色生成色调变体
function generateTonalVariants(hexColor: string): TonalVariants {
  const argb = argbFromHex(hexColor)
  const hct = Hct.fromInt(argb)
  const palette = TonalPalette.fromHct(hct)
  
  const tones = [0, 5, 10, 15, 20, 25, 30, 35, 40, 50, 60, 70, 80, 90, 95, 98, 99, 100]
  const result: TonalVariants = {}
  for (const tone of tones) {
    result[`tone${tone}`] = hexFromArgb(palette.tone(tone))
  }
  return result
}

// 辅助函数：生成互补色和类似色
function generateHarmonyColors(hexColor: string): HarmonyColors {
  const argb = argbFromHex(hexColor)
  const hct = Hct.fromInt(argb)
  const hue = hct.hue
  const chroma = hct.chroma
  const tone = hct.tone
  
  return {
    complementary: hexFromArgb(Hct.from((hue + 180) % 360, chroma, tone).toInt()),
    analogous1: hexFromArgb(Hct.from((hue + 30) % 360, chroma, tone).toInt()),
    analogous2: hexFromArgb(Hct.from((hue + 330) % 360, chroma, tone).toInt()),
    triadic1: hexFromArgb(Hct.from((hue + 120) % 360, chroma, tone).toInt()),
    triadic2: hexFromArgb(Hct.from((hue + 240) % 360, chroma, tone).toInt()),
  }
}

// 为灰色主题生成 MD3 风格的颜色系统
function generateGrayThemeColors(hexColor: string, isDark: boolean) {
  const hct = Hct.fromInt(argbFromHex(hexColor))
  const hue = hct.hue
  
  // 使用 MD3 neutral palette 的 chroma 值，但保持用户选择的色相
  const primaryPalette = TonalPalette.fromHueAndChroma(hue, 8)      // 稍高于 neutral variant
  const secondaryPalette = TonalPalette.fromHueAndChroma(hue, 6)    // 介于 n1 和 n2 之间
  const tertiaryPalette = TonalPalette.fromHueAndChroma(hue, 4)     // 等于 neutral
  const neutralPalette = TonalPalette.fromHueAndChroma(hue, 4)
  const neutralVariantPalette = TonalPalette.fromHueAndChroma(hue, 8)
  
  if (isDark) {
    return {
      primary: hexFromArgb(primaryPalette.tone(80)),
      onPrimary: hexFromArgb(primaryPalette.tone(20)),
      primaryContainer: hexFromArgb(primaryPalette.tone(30)),
      onPrimaryContainer: hexFromArgb(primaryPalette.tone(90)),
      secondary: hexFromArgb(secondaryPalette.tone(80)),
      onSecondary: hexFromArgb(secondaryPalette.tone(20)),
      secondaryContainer: hexFromArgb(secondaryPalette.tone(30)),
      onSecondaryContainer: hexFromArgb(secondaryPalette.tone(90)),
      tertiary: hexFromArgb(tertiaryPalette.tone(80)),
      onTertiary: hexFromArgb(tertiaryPalette.tone(20)),
      tertiaryContainer: hexFromArgb(tertiaryPalette.tone(30)),
      onTertiaryContainer: hexFromArgb(tertiaryPalette.tone(90)),
      surface: hexFromArgb(neutralPalette.tone(6)),
      onSurface: hexFromArgb(neutralPalette.tone(90)),
      surfaceVariant: hexFromArgb(neutralVariantPalette.tone(30)),
      onSurfaceVariant: hexFromArgb(neutralVariantPalette.tone(80)),
      background: hexFromArgb(neutralPalette.tone(6)),
      onBackground: hexFromArgb(neutralPalette.tone(90)),
      outline: hexFromArgb(neutralVariantPalette.tone(60)),
      outlineVariant: hexFromArgb(neutralVariantPalette.tone(30)),
    }
  } else {
    return {
      primary: hexFromArgb(primaryPalette.tone(40)),
      onPrimary: hexFromArgb(primaryPalette.tone(100)),
      primaryContainer: hexFromArgb(primaryPalette.tone(90)),
      onPrimaryContainer: hexFromArgb(primaryPalette.tone(10)),
      secondary: hexFromArgb(secondaryPalette.tone(40)),
      onSecondary: hexFromArgb(secondaryPalette.tone(100)),
      secondaryContainer: hexFromArgb(secondaryPalette.tone(90)),
      onSecondaryContainer: hexFromArgb(secondaryPalette.tone(10)),
      tertiary: hexFromArgb(tertiaryPalette.tone(40)),
      onTertiary: hexFromArgb(tertiaryPalette.tone(100)),
      tertiaryContainer: hexFromArgb(tertiaryPalette.tone(90)),
      onTertiaryContainer: hexFromArgb(tertiaryPalette.tone(10)),
      surface: hexFromArgb(neutralPalette.tone(98)),
      onSurface: hexFromArgb(neutralPalette.tone(10)),
      surfaceVariant: hexFromArgb(neutralVariantPalette.tone(90)),
      onSurfaceVariant: hexFromArgb(neutralVariantPalette.tone(30)),
      background: hexFromArgb(neutralPalette.tone(98)),
      onBackground: hexFromArgb(neutralPalette.tone(10)),
      outline: hexFromArgb(neutralVariantPalette.tone(50)),
      outlineVariant: hexFromArgb(neutralVariantPalette.tone(80)),
    }
  }
}

// 生成自定义 CSS 变量
function generateCustomCSS(primaryColor: string, isDark: boolean, enableGlass: boolean, enableGradients: boolean): string {
  const primaryHct = Hct.fromInt(argbFromHex(primaryColor))
  const isLightColor = primaryHct.tone > 50
  
  // 生成色调变体和和谐色
  const tones = generateTonalVariants(primaryColor)
  const harmony = generateHarmonyColors(primaryColor)
  const accentTones = generateTonalVariants(harmony.complementary)
  
  const onPrimaryColor = isLightColor ? '#000000' : '#ffffff'
  
  let css = ''
  
  // 主题源颜色
  css += `--theme-source-color: ${primaryColor};\n`
  css += `--theme-on-primary: ${onPrimaryColor};\n`
  
  // 主色调变体
  for (const [key, value] of Object.entries(tones)) {
    css += `--theme-primary-${key}: ${value};\n`
  }
  
  // 和谐色
  css += `--theme-complementary: ${harmony.complementary};\n`
  css += `--theme-analogous-1: ${harmony.analogous1};\n`
  css += `--theme-analogous-2: ${harmony.analogous2};\n`
  css += `--theme-triadic-1: ${harmony.triadic1};\n`
  css += `--theme-triadic-2: ${harmony.triadic2};\n`
  
  // 强调色变体
  for (const [key, value] of Object.entries(accentTones)) {
    css += `--theme-accent-${key}: ${value};\n`
  }
  
  // 阴影
  const shadowAlpha = isDark ? [0.5, 0.6, 0.7] : [0.08, 0.12, 0.16]
  css += `--shadow-soft: 0 4px 20px rgba(0, 0, 0, ${shadowAlpha[0]});\n`
  css += `--shadow-medium: 0 8px 30px rgba(0, 0, 0, ${shadowAlpha[1]});\n`
  css += `--shadow-strong: 0 12px 40px rgba(0, 0, 0, ${shadowAlpha[2]});\n`

  // Hover 叠加层 - 暗色模式用白色，亮色模式用黑色
  css += `--md-sys-color-hover-overlay: ${isDark ? 'rgba(255, 255, 255, 0.08)' : 'rgba(0, 0, 0, 0.08)'};\n`

  // 主色透明度变体
  css += `--primary-alpha-5: color-mix(in srgb, var(--md-sys-color-primary) 5%, transparent);\n`
  css += `--primary-alpha-10: color-mix(in srgb, var(--md-sys-color-primary) 10%, transparent);\n`
  css += `--primary-alpha-20: color-mix(in srgb, var(--md-sys-color-primary) 20%, transparent);\n`
  css += `--primary-alpha-30: color-mix(in srgb, var(--md-sys-color-primary) 30%, transparent);\n`
  
  // 玻璃态效果
  if (enableGlass) {
    css += `--glass-blur: 12px;\n`
    css += `--glass-opacity: ${isDark ? '0.75' : '0.85'};\n`
    css += `--glass-border: 1px solid rgba(255, 255, 255, ${isDark ? '0.1' : '0.3'});\n`
    css += `--glass-shadow: 0 8px 32px rgba(0, 0, 0, ${isDark ? '0.4' : '0.1'});\n`
  } else {
    css += `--glass-blur: 0px;\n`
    css += `--glass-opacity: 1;\n`
    css += `--glass-border: none;\n`
    css += `--glass-shadow: var(--md-sys-elevation-level2);\n`
  }
  
  // 渐变效果
  if (enableGradients) {
    css += `--gradient-primary: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%);\n`
    css += `--gradient-surface: none;\n`
    css += `--gradient-accent: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-tertiary) 100%);\n`
    css += `--gradient-background: none;\n`
    css += `--gradient-hover: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%);\n`
  } else {
    css += `--gradient-primary: none;\n`
    css += `--gradient-surface: none;\n`
    css += `--gradient-accent: none;\n`
    css += `--gradient-background: none;\n`
    css += `--gradient-hover: none;\n`
  }
  
  return css
}

interface ThemeState {
  isDarkMode: boolean
  themePreference: ThemePreference
  primaryColor: string
  enableGlassEffect: boolean
  enableGradients: boolean
}

export const useThemeStore = defineStore('theme', {
  state: (): ThemeState => ({
    isDarkMode: false,
    themePreference: 'auto',
    primaryColor: '#2C2C2C',
    enableGlassEffect: true,
    enableGradients: true,
  }),

  getters: {
    isDark: (state): boolean => state.isDarkMode,
  },

  actions: {
    async toggleDarkMode(): Promise<void> {
      this.isDarkMode = !this.isDarkMode
      if (this.themePreference !== 'auto') {
        this.themePreference = this.isDarkMode ? 'dark' : 'light'
      }
      this.applyTheme()
      await this.saveThemeToConfig()
    },

    async setPrimaryColor(color: string): Promise<void> {
      this.primaryColor = color
      this.themePreference = color
      this.applyTheme()
      await this.saveThemeToConfig()
    },

    setThemePreference(preference: ThemePreference): void {
      this.themePreference = preference
      if (preference === 'auto') {
        this.isDarkMode = window.matchMedia('(prefers-color-scheme: dark)').matches
        this.primaryColor = '#2C2C2C'
      } else if (preference === 'light') {
        this.isDarkMode = false
        this.primaryColor = '#2C2C2C'
      } else if (preference === 'dark') {
        this.isDarkMode = true
        this.primaryColor = '#2C2C2C'
      } else if (preference.startsWith('#')) {
        this.primaryColor = preference
      }
      this.applyTheme()
    },
    
    setGlassEffect(enabled: boolean): void {
      this.enableGlassEffect = enabled
      this.applyTheme()
    },
    
    setGradients(enabled: boolean): void {
      this.enableGradients = enabled
      this.applyTheme()
    },

    applyTheme(): void {
      const isGray = isNeutralGray(this.primaryColor)
      
      // 1. 使用 MD3 库设置基础颜色
      const theme = themeFromSourceColor(argbFromHex(this.primaryColor))
      applyTheme(theme, { target: document.documentElement, dark: this.isDarkMode })
      
      // 2. 如果是灰色，覆盖 MD3 生成的彩色为真正的灰色
      if (isGray) {
        const grayColors = generateGrayThemeColors(this.primaryColor, this.isDarkMode)
        const root = document.documentElement
        
        root.style.setProperty('--md-sys-color-primary', grayColors.primary)
        root.style.setProperty('--md-sys-color-on-primary', grayColors.onPrimary)
        root.style.setProperty('--md-sys-color-primary-container', grayColors.primaryContainer)
        root.style.setProperty('--md-sys-color-on-primary-container', grayColors.onPrimaryContainer)
        root.style.setProperty('--md-sys-color-secondary', grayColors.secondary)
        root.style.setProperty('--md-sys-color-on-secondary', grayColors.onSecondary)
        root.style.setProperty('--md-sys-color-secondary-container', grayColors.secondaryContainer)
        root.style.setProperty('--md-sys-color-on-secondary-container', grayColors.onSecondaryContainer)
        root.style.setProperty('--md-sys-color-tertiary', grayColors.tertiary)
        root.style.setProperty('--md-sys-color-on-tertiary', grayColors.onTertiary)
        root.style.setProperty('--md-sys-color-tertiary-container', grayColors.tertiaryContainer)
        root.style.setProperty('--md-sys-color-on-tertiary-container', grayColors.onTertiaryContainer)
        root.style.setProperty('--md-sys-color-surface', grayColors.surface)
        root.style.setProperty('--md-sys-color-on-surface', grayColors.onSurface)
        root.style.setProperty('--md-sys-color-surface-variant', grayColors.surfaceVariant)
        root.style.setProperty('--md-sys-color-on-surface-variant', grayColors.onSurfaceVariant)
        root.style.setProperty('--md-sys-color-background', grayColors.background)
        root.style.setProperty('--md-sys-color-on-background', grayColors.onBackground)
        root.style.setProperty('--md-sys-color-outline', grayColors.outline)
        root.style.setProperty('--md-sys-color-outline-variant', grayColors.outlineVariant)
      }
      
      // 3. 生成并应用自定义 CSS
      const cacheKey = `${this.primaryColor}-${this.isDarkMode}-${this.enableGlassEffect}-${this.enableGradients}`
      
      if (!customStyleCache.has(cacheKey)) {
        const customCSS = generateCustomCSS(
          this.primaryColor, 
          this.isDarkMode, 
          this.enableGlassEffect, 
          this.enableGradients
        )
        customStyleCache.set(cacheKey, customCSS)
        
        if (customStyleCache.size > 20) {
          const firstKey = customStyleCache.keys().next().value
          if (firstKey) customStyleCache.delete(firstKey)
        }
      }
      
      if (!customStyleElement) {
        customStyleElement = document.createElement('style')
        customStyleElement.id = 'theme-custom-variables'
        document.head.appendChild(customStyleElement)
      }
      
      customStyleElement.textContent = `:root {\n${customStyleCache.get(cacheKey)}}`
      
      // 移除旧的覆盖样式
      const overrideStyle = document.getElementById('theme-gray-override')
      if (overrideStyle) {
        overrideStyle.remove()
      }
      
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light')
      
      logger.debug('Theme applied:', cacheKey, isGray ? '(gray mode)' : '')
      
      // 验证颜色对比度
      setTimeout(() => {
        const results = validateThemeContrast(this.isDarkMode)
        if (results.failed.length > 0) {
          logger.warn('主题颜色对比度不符合 WCAG 标准:', results.failed)
        }
      }, 50)
    },
    
    async saveThemeToConfig(): Promise<void> {
      try {
        const configStore = useConfigStore()
        configStore.general.theme = this.themePreference
        await configStore.saveConfigNow()
      } catch (error) {
        logger.error('Failed to save theme to config:', error)
      }
    },
  },
})
