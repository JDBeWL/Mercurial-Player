/**
 * CSS 能力探测：Android 端系统 WebView 版本可能缺新特性（如 color-mix() 到 Chrome 111 才引入）。
 *
 * 提供统一开关，让新语法在老 WebView 上退回到可读的旧写法，而不是整条声明失效。
 */

let colorMixSupport: boolean | null = null

/**
 * 当前 WebView 是否支持 color-mix()，判据需与样式表里的 @supports (color: color-mix(...)) 保持一致。
 *
 * 自定义属性值不做语法校验，写进 CSS 变量的 color-mix 只能靠 @supports 兜底，不能用这个开关。
 */
export function supportsColorMix(): boolean {
  if (colorMixSupport !== null) return colorMixSupport
  if (typeof CSS === 'undefined' || typeof CSS.supports !== 'function') {
    colorMixSupport = false
    return colorMixSupport
  }
  try {
    colorMixSupport = CSS.supports('color', 'color-mix(in srgb, red 40%, blue)')
  } catch {
    colorMixSupport = false
  }
  return colorMixSupport
}
