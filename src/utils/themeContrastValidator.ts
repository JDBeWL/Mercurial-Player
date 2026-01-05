/**
 * 主题对比度验证器
 * 在应用主题时验证颜色对比度是否符合 WCAG 标准
 */

import { checkContrast, getColorFromCSSVar } from './colorContrast'
import logger from './logger'

interface ValidationResult {
  name: string
  foreground?: string
  background?: string
  ratio?: number
  required?: number
  level?: string
  largeText?: boolean
  message?: string
}

interface ValidationResults {
  passed: ValidationResult[]
  failed: ValidationResult[]
  warnings: ValidationResult[]
}

interface ColorPairConfig {
  name: string
  foreground: string
  background: string
  largeText: boolean
  required: boolean
}

/**
 * 验证 Material Design 3 颜色系统的对比度
 */
export function validateThemeContrast(_isDark: boolean = false): ValidationResults {
  const results: ValidationResults = {
    passed: [],
    failed: [],
    warnings: [],
  }

  // 需要验证的颜色组合
  const colorPairs: ColorPairConfig[] = [
    // 关键文本颜色组合（必须符合标准）
    {
      name: 'On Surface on Background',
      foreground: '--md-sys-color-on-surface',
      background: '--md-sys-color-background',
      largeText: false,
      required: true,
    },
    {
      name: 'On Surface Variant on Surface',
      foreground: '--md-sys-color-on-surface-variant',
      background: '--md-sys-color-surface',
      largeText: false,
      required: true,
    },
    {
      name: 'On Background on Background',
      foreground: '--md-sys-color-on-background',
      background: '--md-sys-color-background',
      largeText: false,
      required: true,
    },
    // 容器颜色组合
    {
      name: 'On Primary Container on Primary Container',
      foreground: '--md-sys-color-on-primary-container',
      background: '--md-sys-color-primary-container',
      largeText: false,
      required: true,
    },
    {
      name: 'On Secondary Container on Secondary Container',
      foreground: '--md-sys-color-on-secondary-container',
      background: '--md-sys-color-secondary-container',
      largeText: false,
      required: true,
    },
    {
      name: 'On Error Container on Error Container',
      foreground: '--md-sys-color-on-error-container',
      background: '--md-sys-color-error-container',
      largeText: false,
      required: true,
    },
    // 按钮和交互元素
    {
      name: 'On Primary on Primary',
      foreground: '--md-sys-color-on-primary',
      background: '--md-sys-color-primary',
      largeText: false,
      required: false,
    },
    // 大文本
    {
      name: 'Headline on Background (Large)',
      foreground: '--md-sys-color-on-background',
      background: '--md-sys-color-background',
      largeText: true,
      required: true,
    },
    // Primary 作为文本颜色
    {
      name: 'Primary on Background (Links/Accents)',
      foreground: '--md-sys-color-primary',
      background: '--md-sys-color-background',
      largeText: true,
      required: false,
    },
  ]

  colorPairs.forEach(({ name, foreground, background, largeText, required = true }) => {
    const fgColor = getColorFromCSSVar(foreground)
    const bgColor = getColorFromCSSVar(background)

    if (!fgColor || !bgColor) {
      results.warnings.push({
        name,
        message: `无法获取颜色值: ${foreground} 或 ${background}`,
      })
      return
    }

    // 检查 AA 级别
    const checkAA = checkContrast(fgColor, bgColor, {
      level: 'AA',
      largeText,
    })

    if (checkAA.pass) {
      results.passed.push({
        name,
        foreground,
        background,
        ratio: checkAA.ratio,
        level: 'AA',
        largeText,
      })
    } else {
      const result: ValidationResult = {
        name,
        foreground,
        background,
        ratio: checkAA.ratio,
        required: checkAA.requiredRatio,
        level: 'AA',
        largeText,
        message: checkAA.message,
      }
      
      if (required) {
        results.failed.push(result)
      } else {
        results.warnings.push({
          ...result,
          message: `${name}: ${checkAA.message} (设计权衡，可能可接受)`,
        })
      }
    }
  })

  // 记录结果
  if (results.failed.length > 0) {
    logger.warn('主题对比度验证失败（关键组合）:', results.failed)
    if (results.warnings.length > 0) {
      logger.info('主题对比度验证警告（设计权衡）:', results.warnings)
    }
  } else if (results.warnings.length > 0) {
    logger.debug('主题对比度验证: 关键组合通过，但有设计权衡警告:', results.warnings)
  } else {
    logger.debug('主题对比度验证通过')
  }

  return results
}

/**
 * 在主题应用后自动验证
 */
export function setupThemeContrastValidation(): void {
  if (typeof window === 'undefined') return

  // 监听主题变化
  const observer = new MutationObserver(() => {
    const isDark = document.documentElement.getAttribute('data-theme') === 'dark'
    validateThemeContrast(isDark)
  })

  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style'],
  })

  // 初始验证
  setTimeout(() => {
    const isDark = document.documentElement.getAttribute('data-theme') === 'dark'
    validateThemeContrast(isDark)
  }, 100)
}
