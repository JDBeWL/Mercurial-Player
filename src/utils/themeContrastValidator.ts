/** 主题对比度检查: validateThemeContrast 只出诊断日志, enforceThemeContrast 才会回写颜色 */

import { checkContrast, getColorFromCSSVar, adjustColorForContrast } from './colorContrast'
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

/** 校验当前主题的全部关键颜色对, required 项不达标进 failed, 其余进 warnings */
export function validateThemeContrast(_isDark: boolean = false): ValidationResults {
  const results: ValidationResults = {
    passed: [],
    failed: [],
    warnings: [],
  }

  const colorPairs: ColorPairConfig[] = [
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
    {
      name: 'On Primary on Primary',
      foreground: '--md-sys-color-on-primary',
      background: '--md-sys-color-primary',
      largeText: false,
      required: false,
    },
    // largeText=true 时 AA 阈值从 4.5:1 降到 3:1
    {
      name: 'Headline on Background (Large)',
      foreground: '--md-sys-color-on-background',
      background: '--md-sys-color-background',
      largeText: true,
      required: true,
    },
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

/** 挂上 MutationObserver, 主题属性变化后自动跑一次验证 */
export function setupThemeContrastValidation(): void {
  if (typeof window === 'undefined') return

  // 防抖: 拖动取色器时每个 input 事件都会 applyTheme, 全量验证要 18 次 getComputedStyle + 9 组对比计算, 会卡取色
  // 验证只写诊断日志不改颜色, 静默期后跑一次就够
  const VALIDATE_DEBOUNCE_MS = 300
  let validateTimer: ReturnType<typeof setTimeout> | null = null
  const scheduleValidation = (): void => {
    if (validateTimer) clearTimeout(validateTimer)
    validateTimer = setTimeout(() => {
      validateTimer = null
      validateThemeContrast(document.documentElement.getAttribute('data-theme') === 'dark')
    }, VALIDATE_DEBOUNCE_MS)
  }

  const observer = new MutationObserver(() => {
    scheduleValidation()
  })

  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style'],
  })

  // 主题色由 theme store 在 config 加载后才异步应用, 变量缺失时验证只会刷"无法获取颜色值"噪音告警
  // 故轮询等 --md-sys-color-* 就绪; 超时后照常验证, 保留缺失颜色的诊断价值
  const INIT_POLL_INTERVAL_MS = 100
  const INIT_MAX_ATTEMPTS = 50 // 上限 5s
  let attempts = 0
  const initialValidate = (): void => {
    if (attempts < INIT_MAX_ATTEMPTS && !getColorFromCSSVar('--md-sys-color-primary')) {
      attempts += 1
      setTimeout(initialValidate, INIT_POLL_INTERVAL_MS)
      return
    }
    scheduleValidation()
  }
  setTimeout(initialValidate, INIT_POLL_INTERVAL_MS)
}

/**
 * 强制执行 WCAG 2.1 AA: 未达标的 required 颜色对交由 adjustColorForContrast 修前景色, 再回写 CSS 变量。
 *
 * @returns 修复的颜色对数量
 */
export function enforceThemeContrast(): number {
  const root = document.documentElement
  let fixedCount = 0

  // 与 validateThemeContrast 里的 colorPairs 是同一份配置的副本, 增删颜色对时两处都要改
  const colorPairs: ColorPairConfig[] = [
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
    {
      name: 'Headline on Background (Large)',
      foreground: '--md-sys-color-on-background',
      background: '--md-sys-color-background',
      largeText: true,
      required: true,
    },
  ]

  for (const { name, foreground, background, largeText } of colorPairs) {
    const fgColor = getColorFromCSSVar(foreground)
    const bgColor = getColorFromCSSVar(background)

    if (!fgColor || !bgColor) continue

    const check = checkContrast(fgColor, bgColor, { level: 'AA', largeText })
    if (!check.pass) {
      const adjusted = adjustColorForContrast(fgColor, bgColor, {
        level: 'AA',
        largeText,
      })
      root.style.setProperty(foreground, adjusted)
      fixedCount++
      logger.info(
        `WCAG 修复: ${name} 对比度 ${check.ratio}:1 -> 已调整 foreground (${fgColor} -> ${adjusted})`,
      )
    }
  }

  if (fixedCount > 0) {
    logger.info(`WCAG 2.1 强制合规完成，共修复 ${fixedCount} 个颜色对`)
  }

  return fixedCount
}
