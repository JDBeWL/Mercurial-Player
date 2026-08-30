/**
 * 主题对比度验证器
 * 在应用主题时验证颜色对比度是否符合 WCAG 标准
 */

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

  // 防抖调度:拖动取色器时 applyTheme 会高频触发(每个 input 事件一次),
  // 每次都全量验证(18 次 getComputedStyle + 9 组对比计算)会造成取色卡顿;
  // 验证只是诊断用途,静默期后跑一次即可
  const VALIDATE_DEBOUNCE_MS = 300
  let validateTimer: ReturnType<typeof setTimeout> | null = null
  const scheduleValidation = (): void => {
    if (validateTimer) clearTimeout(validateTimer)
    validateTimer = setTimeout(() => {
      validateTimer = null
      validateThemeContrast(document.documentElement.getAttribute('data-theme') === 'dark')
    }, VALIDATE_DEBOUNCE_MS)
  }

  // 监听主题变化
  const observer = new MutationObserver(() => {
    scheduleValidation()
  })

  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style'],
  })

  // 初始验证:主题颜色由 theme store 异步应用(config 加载完成后才有第一次 applyTheme),
  // 轮询等待 --md-sys-color-* 变量就绪,避免在颜色未应用时产生一轮
  // "无法获取颜色值" 的噪音告警;超时后照常验证,保留缺失诊断价值
  const INIT_POLL_INTERVAL_MS = 100
  const INIT_MAX_ATTEMPTS = 50 // 最长等待 5s
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
 * 强制执行 WCAG 2.1 AA 合规
 *
 * 验证当前主题的关键颜色对，对未达标的 required 组合调用
 * adjustColorForContrast 主动调整 foreground 并回写 CSS 变量，
 * 确保所有关键文本/容器对比度满足 WCAG 2.1 AA 标准。
 *
 * @returns 修复的颜色对数量
 */
export function enforceThemeContrast(): number {
  const root = document.documentElement
  let fixedCount = 0

  // 复用 validateThemeContrast 的配置，但这里需要同步处理
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
      // 主动调整 foreground 颜色以满足对比度要求
      const adjusted = adjustColorForContrast(fgColor, bgColor, {
        level: 'AA',
        largeText,
      })
      root.style.setProperty(foreground, adjusted)
      fixedCount++
      logger.info(
        `WCAG 修复: ${name} 对比度 ${check.ratio}:1 → 已调整 foreground (${fgColor} → ${adjusted})`,
      )
    }
  }

  if (fixedCount > 0) {
    logger.info(`WCAG 2.1 强制合规完成，共修复 ${fixedCount} 个颜色对`)
  }

  return fixedCount
}
