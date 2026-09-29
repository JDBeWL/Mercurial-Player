/** CSS 能力探测：Android 端受系统 WebView 版本限制可能缺新特性（`color-mix()` 到
 *  Chrome 111 才引入），需要一个统一开关让新语法在老 WebView 上退回到可读的旧写法，
 *  而不是整条声明失效。 */

let colorMixSupport: boolean | null = null

/** 当前 WebView 是否支持 `color-mix()`。判据与样式表里的
 *  `@supports (color: color-mix(in srgb, red 40%, blue))` 一致；但自定义属性的值不做
 *  语法校验，写进 CSS 变量的 color-mix 只能靠 `@supports` 兜底，不能用这个开关。 */
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
