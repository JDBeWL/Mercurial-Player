/** CSS 能力探测 / Android WebView 可能缺新特性（如 `color-mix()` 要 Chrome 111），统一开关让语法回退。 */

let colorMixSupport: boolean | null = null

/** 当前 WebView 是否支持 `color-mix()`；判据须与样式表的 `@supports` 一致。
 * 自定义属性值不做语法校验，写进 CSS 变量的 `color-mix` 只能靠 `@supports` 兜底。 */
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
