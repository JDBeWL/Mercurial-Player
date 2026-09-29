/**
 * 浏览器 CSS 能力探测。
 *
 * 桌面端跑的是 WebView2 / Chromium 较新的版本，而 Android 端受系统 WebView 版本
 * 限制，可能缺一些新特性（例如 MuMu 上的 Android System WebView 110 不支持
 * `color-mix()`，该语法到 Chrome 111 才引入）。前端因此需要一个统一的能力开关，
 * 让"用新语法写的样式"在老 WebView 上退回到可读的版本，而不是整条声明失效。
 */

let colorMixSupport: boolean | null = null

/**
 * 当前 WebView 是否支持 `color-mix()`。
 *
 * 注意：**只对"直接写在属性值里"的用法有效**（如 `--inactive-color: color-mix(...)` 之外、
 * 直接赋给 `color` / `background-color`）。自定义属性的值是不做语法校验的，
 * 所以一旦发现某处的 color-mix 是塞进 CSS 变量的，就必须用 `@supports` 兜底，
 * 不能只靠这里的开关。
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
