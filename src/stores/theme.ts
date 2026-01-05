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

// 生成自定义 CSS 变量
function generateCustomCSS(primaryColor: string, isDark: boolean, enableGlass: boolean, enableGradients: boolean): string {
  const primaryHct = Hct.fromInt(argbFromHex(primaryColor))
  const isLightColor = primaryHct.tone > 50
  const primaryPalette = TonalPalette.fromHct(primaryHct)
  
  // 检测是否为深灰色/中性色（色度低，接近灰色）
  // 或者检查 RGB 值是否接近（差值小于 20 认为是灰色）
  const rgb = {
    r: (argbFromHex(primaryColor) >> 16) & 0xFF,
    g: (argbFromHex(primaryColor) >> 8) & 0xFF,
    b: argbFromHex(primaryColor) & 0xFF
  }
  const rgbDiff = Math.max(rgb.r, rgb.g, rgb.b) - Math.min(rgb.r, rgb.g, rgb.b)
  const isNeutralGray = primaryHct.chroma < 15 || rgbDiff < 20
  
  // 计算 on-primary 和 container 颜色
  // 对于深灰色主题，在暗色模式下需要特殊处理
  let effectivePrimaryColor = primaryColor
  let effectivePrimaryHct = primaryHct
  let effectivePalette = primaryPalette
  
  if (isNeutralGray && isDark) {
    // 在暗色模式下，深灰色需要稍微调亮以确保可见性
    // 如果原始 tone 太暗（< 30），调整到 40-50 左右
    const adjustedTone = primaryHct.tone < 30 ? Math.min(primaryHct.tone + 25, 50) : primaryHct.tone
    effectivePrimaryHct = Hct.from(primaryHct.hue, primaryHct.chroma, adjustedTone)
    effectivePrimaryColor = hexFromArgb(effectivePrimaryHct.toInt())
    effectivePalette = TonalPalette.fromHct(effectivePrimaryHct)
  }
  
  const onPrimaryColor = effectivePrimaryHct.tone > 50 ? '#000000' : '#ffffff'
  // 在暗色模式下，container 应该比 primary 稍暗；在亮色模式下，container 应该比 primary 稍亮
  const containerTone = isDark 
    ? (isNeutralGray ? Math.max(effectivePrimaryHct.tone - 10, 20) : 30)
    : 90
  const onContainerTone = isDark ? 90 : 10
  const containerColor = hexFromArgb(effectivePalette.tone(containerTone))
  const onContainerColor = hexFromArgb(effectivePalette.tone(onContainerTone))
  
  // 生成色调变体和和谐色（使用有效的 primary 颜色）
  const tones = generateTonalVariants(effectivePrimaryColor)
  const harmony = generateHarmonyColors(effectivePrimaryColor)
  const accentTones = generateTonalVariants(harmony.complementary)
  
  // 如果是深灰色，生成灰色系的 secondary 和 tertiary
  let secondaryColor = effectivePrimaryColor
  let tertiaryColor = effectivePrimaryColor
  if (isNeutralGray) {
    // 使用相同色调但稍微不同的亮度作为 secondary 和 tertiary
    const secondaryTone = isDark 
      ? Math.min(effectivePrimaryHct.tone + 5, 60)
      : Math.max(effectivePrimaryHct.tone - 5, 0)
    const tertiaryTone = isDark 
      ? Math.min(effectivePrimaryHct.tone + 10, 65)
      : Math.max(effectivePrimaryHct.tone - 10, 0)
    secondaryColor = hexFromArgb(Hct.from(effectivePrimaryHct.hue, effectivePrimaryHct.chroma, secondaryTone).toInt())
    tertiaryColor = hexFromArgb(Hct.from(effectivePrimaryHct.hue, effectivePrimaryHct.chroma, tertiaryTone).toInt())
  }
  
  let css = ''
  
  // 覆盖 primary 相关颜色（使用有效的 primary 颜色）
  css += `--md-sys-color-primary: ${effectivePrimaryColor};\n`
  css += `--md-sys-color-on-primary: ${onPrimaryColor};\n`
  css += `--md-sys-color-primary-container: ${containerColor};\n`
  css += `--md-sys-color-on-primary-container: ${onContainerColor};\n`
  
  // 如果是深灰色，强制覆盖 secondary 和 tertiary 为灰色系
  if (isNeutralGray) {
    const secondaryPalette = TonalPalette.fromHct(Hct.fromInt(argbFromHex(secondaryColor)))
    const tertiaryPalette = TonalPalette.fromHct(Hct.fromInt(argbFromHex(tertiaryColor)))
    const secondaryContainerTone = isDark ? 30 : 90
    const tertiaryContainerTone = isDark ? 30 : 90
    
    css += `--md-sys-color-secondary: ${secondaryColor};\n`
    css += `--md-sys-color-on-secondary: ${isLightColor ? '#000000' : '#ffffff'};\n`
    css += `--md-sys-color-secondary-container: ${hexFromArgb(secondaryPalette.tone(secondaryContainerTone))};\n`
    css += `--md-sys-color-on-secondary-container: ${hexFromArgb(secondaryPalette.tone(isDark ? 90 : 10))};\n`
    
    css += `--md-sys-color-tertiary: ${tertiaryColor};\n`
    css += `--md-sys-color-on-tertiary: ${isLightColor ? '#000000' : '#ffffff'};\n`
    css += `--md-sys-color-tertiary-container: ${hexFromArgb(tertiaryPalette.tone(tertiaryContainerTone))};\n`
    css += `--md-sys-color-on-tertiary-container: ${hexFromArgb(tertiaryPalette.tone(isDark ? 90 : 10))};\n`
  }
  
  // 主题源颜色（使用有效的 primary 颜色）
  css += `--theme-source-color: ${effectivePrimaryColor};\n`
  css += `--theme-on-primary: ${onPrimaryColor};\n`
  css += `--theme-on-primary-container: ${onContainerColor};\n`
  
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
  
  // 主色透明度变体（使用有效的 primary 颜色）
  css += `--primary-alpha-5: color-mix(in srgb, ${effectivePrimaryColor} 5%, transparent);\n`
  css += `--primary-alpha-10: color-mix(in srgb, ${effectivePrimaryColor} 10%, transparent);\n`
  css += `--primary-alpha-20: color-mix(in srgb, ${effectivePrimaryColor} 20%, transparent);\n`
  css += `--primary-alpha-30: color-mix(in srgb, ${effectivePrimaryColor} 30%, transparent);\n`
  
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
      // 对于深灰色主题，在暗色模式下需要调整颜色以确保可见性
      let effectivePrimaryColor = this.primaryColor
      const primaryHct = Hct.fromInt(argbFromHex(this.primaryColor))
      const rgb = {
        r: (argbFromHex(this.primaryColor) >> 16) & 0xFF,
        g: (argbFromHex(this.primaryColor) >> 8) & 0xFF,
        b: argbFromHex(this.primaryColor) & 0xFF
      }
      const rgbDiff = Math.max(rgb.r, rgb.g, rgb.b) - Math.min(rgb.r, rgb.g, rgb.b)
      const isNeutralGray = primaryHct.chroma < 15 || rgbDiff < 20
      
      if (isNeutralGray && this.isDarkMode && primaryHct.tone < 30) {
        // 在暗色模式下，深灰色需要稍微调亮以确保可见性
        const adjustedTone = Math.min(primaryHct.tone + 25, 50)
        const adjustedHct = Hct.from(primaryHct.hue, primaryHct.chroma, adjustedTone)
        effectivePrimaryColor = hexFromArgb(adjustedHct.toInt())
      }
      
      // 1. 使用 Material Design 库设置基础颜色（使用调整后的颜色）
      const theme = themeFromSourceColor(argbFromHex(effectivePrimaryColor))
      applyTheme(theme, { target: document.documentElement, dark: this.isDarkMode })
      
      // 2. 生成并应用自定义 CSS（覆盖和扩展）
      const cacheKey = `${this.primaryColor}-${this.isDarkMode}-${this.enableGlassEffect}-${this.enableGradients}`
      
      if (!customStyleCache.has(cacheKey)) {
        const customCSS = generateCustomCSS(
          this.primaryColor, 
          this.isDarkMode, 
          this.enableGlassEffect, 
          this.enableGradients
        )
        customStyleCache.set(cacheKey, customCSS)
        
        // 限制缓存大小
        if (customStyleCache.size > 20) {
          const firstKey = customStyleCache.keys().next().value
          if (firstKey) customStyleCache.delete(firstKey)
        }
      }
      
      // 创建或更新自定义 style 元素
      if (!customStyleElement) {
        customStyleElement = document.createElement('style')
        customStyleElement.id = 'theme-custom-variables'
        document.head.appendChild(customStyleElement)
      }
      
      // 使用 :root 选择器，确保覆盖 MD3 生成的颜色
      customStyleElement.textContent = `:root {\n${customStyleCache.get(cacheKey)}}`
      
      // 对于深灰色主题，额外确保覆盖所有相关颜色（使用已声明的变量）
      if (isNeutralGray) {
        // 在暗色模式下，深灰色需要稍微调亮以确保可见性
        let effectivePrimaryHct = primaryHct
        if (this.isDarkMode && primaryHct.tone < 30) {
          const adjustedTone = Math.min(primaryHct.tone + 25, 50)
          effectivePrimaryHct = Hct.from(primaryHct.hue, primaryHct.chroma, adjustedTone)
        }
        
        // 生成灰色系的 container 颜色
        const effectivePalette = TonalPalette.fromHct(effectivePrimaryHct)
        const containerTone = this.isDarkMode 
          ? Math.max(effectivePrimaryHct.tone - 10, 20)
          : 90
        const onContainerTone = this.isDarkMode ? 90 : 10
        const containerColor = hexFromArgb(effectivePalette.tone(containerTone))
        const onContainerColor = hexFromArgb(effectivePalette.tone(onContainerTone))
        
        // 生成 secondary 和 tertiary 的灰色变体
        const secondaryTone = this.isDarkMode 
          ? Math.min(effectivePrimaryHct.tone + 5, 60)
          : Math.max(effectivePrimaryHct.tone - 5, 0)
        const tertiaryTone = this.isDarkMode 
          ? Math.min(effectivePrimaryHct.tone + 10, 65)
          : Math.max(effectivePrimaryHct.tone - 10, 0)
        const effectivePrimaryColor = hexFromArgb(effectivePrimaryHct.toInt())
        const secondaryColor = hexFromArgb(Hct.from(effectivePrimaryHct.hue, effectivePrimaryHct.chroma, secondaryTone).toInt())
        const tertiaryColor = hexFromArgb(Hct.from(effectivePrimaryHct.hue, effectivePrimaryHct.chroma, tertiaryTone).toInt())
        const secondaryPalette = TonalPalette.fromHct(Hct.fromInt(argbFromHex(secondaryColor)))
        const tertiaryPalette = TonalPalette.fromHct(Hct.fromInt(argbFromHex(tertiaryColor)))
        const secondaryContainerColor = hexFromArgb(secondaryPalette.tone(containerTone))
        const tertiaryContainerColor = hexFromArgb(tertiaryPalette.tone(containerTone))
        const onSecondaryContainerColor = hexFromArgb(secondaryPalette.tone(onContainerTone))
        const onTertiaryContainerColor = hexFromArgb(tertiaryPalette.tone(onContainerTone))
        
        // 强制覆盖所有可能的 MD3 颜色变体为灰色系
        const overrideCSS = `
          :root {
            --md-sys-color-primary: ${effectivePrimaryColor} !important;
            --md-sys-color-on-primary: ${effectivePrimaryHct.tone > 50 ? '#000000' : '#ffffff'} !important;
            --md-sys-color-primary-container: ${containerColor} !important;
            --md-sys-color-on-primary-container: ${onContainerColor} !important;
            --md-sys-color-secondary: ${secondaryColor} !important;
            --md-sys-color-on-secondary: ${effectivePrimaryHct.tone > 50 ? '#000000' : '#ffffff'} !important;
            --md-sys-color-secondary-container: ${secondaryContainerColor} !important;
            --md-sys-color-on-secondary-container: ${onSecondaryContainerColor} !important;
            --md-sys-color-tertiary: ${tertiaryColor} !important;
            --md-sys-color-on-tertiary: ${effectivePrimaryHct.tone > 50 ? '#000000' : '#ffffff'} !important;
            --md-sys-color-tertiary-container: ${tertiaryContainerColor} !important;
            --md-sys-color-on-tertiary-container: ${onTertiaryContainerColor} !important;
          }
        `
        const overrideStyle = document.getElementById('theme-gray-override') as HTMLStyleElement
        if (!overrideStyle) {
          const style = document.createElement('style')
          style.id = 'theme-gray-override'
          style.textContent = overrideCSS
          document.head.appendChild(style)
        } else {
          overrideStyle.textContent = overrideCSS
        }
      } else {
        // 移除灰色覆盖样式（如果存在）
        const overrideStyle = document.getElementById('theme-gray-override')
        if (overrideStyle) {
          overrideStyle.remove()
        }
      }
      
      // 设置 data-theme 属性
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light')
      
      logger.debug('Theme applied:', cacheKey)
      
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
