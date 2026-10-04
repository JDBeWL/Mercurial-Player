/** 颜色对比度工具：检查颜色是否符合 WCAG 无障碍标准 */

interface RGB {
  r: number
  g: number
  b: number
}

interface ContrastCheckResult {
  pass: boolean
  ratio: number
  requiredRatio: number
  level: string
  largeText: boolean
  message: string
}

interface ContrastCheckOptions {
  level?: 'AA' | 'AAA'
  largeText?: boolean
}

interface AdjustColorOptions extends ContrastCheckOptions {
  step?: number
}

function hexToRgb(hex: string): RGB {
  hex = hex.replace('#', '')

  if (hex.length === 3) {
    hex = hex
      .split('')
      .map((char) => char + char)
      .join('')
  }

  const r = parseInt(hex.substring(0, 2), 16)
  const g = parseInt(hex.substring(2, 4), 16)
  const b = parseInt(hex.substring(4, 6), 16)

  return { r, g, b }
}

/** 相对亮度：WCAG 定义的 sRGB 线性化与加权系数（0.2126/0.7152/0.0722），非经验值 */
function getRelativeLuminance(r: number, g: number, b: number): number {
  const [rs, gs, bs] = [r, g, b].map((val) => {
    val = val / 255
    return val <= 0.03928 ? val / 12.92 : Math.pow((val + 0.055) / 1.055, 2.4)
  })

  return 0.2126 * rs! + 0.7152 * gs! + 0.0722 * bs!
}

/** 对比度：WCAG 公式 (L1+0.05)/(L2+0.05) */
export function getContrastRatio(color1: string, color2: string): number {
  const rgb1 = hexToRgb(color1)
  const rgb2 = hexToRgb(color2)

  const lum1 = getRelativeLuminance(rgb1.r, rgb1.g, rgb1.b)
  const lum2 = getRelativeLuminance(rgb2.r, rgb2.g, rgb2.b)

  const lighter = Math.max(lum1, lum2)
  const darker = Math.min(lum1, lum2)

  return (lighter + 0.05) / (darker + 0.05)
}

export function checkContrast(
  foreground: string,
  background: string,
  options: ContrastCheckOptions = {},
): ContrastCheckResult {
  const { level = 'AA', largeText = false } = options

  const ratio = getContrastRatio(foreground, background)

  // WCAG 对比度阈值：AA/AAA x 正常/大字号
  const standards = {
    AA: {
      normal: 4.5,
      large: 3.0,
    },
    AAA: {
      normal: 7.0,
      large: 4.5,
    },
  }

  const requiredRatio = largeText ? standards[level].large : standards[level].normal

  const pass = ratio >= requiredRatio

  const message = pass
    ? `符合 WCAG ${level} 标准（对比度 ${ratio.toFixed(2)}:1，要求 ${requiredRatio}:1）`
    : `不符合 WCAG ${level} 标准（对比度 ${ratio.toFixed(2)}:1，要求 ${requiredRatio}:1）`

  return {
    pass,
    ratio: parseFloat(ratio.toFixed(2)),
    requiredRatio,
    level,
    largeText,
    message,
  }
}

/** 只朝纯白/纯黑方向逐步调整亮度，直至满足对比度阈值 */
export function adjustColorForContrast(
  color: string,
  targetBackground: string,
  options: AdjustColorOptions = {},
): string {
  const { level = 'AA', largeText = false, step = 0.05 } = options

  let currentColor = color
  let check = checkContrast(currentColor, targetBackground, { level, largeText })

  if (check.pass) {
    return currentColor
  }

  const rgb = hexToRgb(currentColor)
  const bgRgb = hexToRgb(targetBackground)
  const bgLum = getRelativeLuminance(bgRgb.r, bgRgb.g, bgRgb.b)

  // 让颜色远离背景以增大对比度：比背景亮则变亮，否则变暗
  const currentLum = getRelativeLuminance(rgb.r, rgb.g, rgb.b)
  const shouldLighten = currentLum > bgLum

  let attempts = 0
  const maxAttempts = 100

  while (!check.pass && attempts < maxAttempts) {
    if (shouldLighten) {
      rgb.r = Math.min(255, Math.round(rgb.r + (255 - rgb.r) * step))
      rgb.g = Math.min(255, Math.round(rgb.g + (255 - rgb.g) * step))
      rgb.b = Math.min(255, Math.round(rgb.b + (255 - rgb.b) * step))
    } else {
      rgb.r = Math.max(0, Math.round(rgb.r * (1 - step)))
      rgb.g = Math.max(0, Math.round(rgb.g * (1 - step)))
      rgb.b = Math.max(0, Math.round(rgb.b * (1 - step)))
    }

    currentColor = `#${[rgb.r, rgb.g, rgb.b]
      .map((val) => val.toString(16).padStart(2, '0'))
      .join('')}`

    check = checkContrast(currentColor, targetBackground, { level, largeText })
    attempts++
  }

  return currentColor
}

export function getColorFromCSSVar(varName: string): string | null {
  if (typeof window === 'undefined') return null

  const value = getComputedStyle(document.documentElement).getPropertyValue(varName).trim()

  if (!value) return null

  if (value.startsWith('rgb')) {
    const matches = value.match(/\d+/g)
    if (matches && matches.length >= 3) {
      const r = parseInt(matches[0])
      const g = parseInt(matches[1]!)
      const b = parseInt(matches[2]!)
      return `#${[r, g, b].map((val) => val.toString(16).padStart(2, '0')).join('')}`
    }
  }

  return value.startsWith('#') ? value : null
}
