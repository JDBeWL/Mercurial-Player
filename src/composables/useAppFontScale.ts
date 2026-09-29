import { setAppFontScale } from '@/services/appService'
import logger from '@/utils/logger'

/**
 * 把「界面字号」倍率下发给原生侧。
 *
 * # 为什么这件事要在应用里做
 *
 * Android 的系统「字体大小」会被 WebView 当成字号倍率，乘到**所有 CSS px 字号**上
 * （真机实测 `font_scale=1.17` 时 CSS 写的 16px 实际按 18.56px 排版；把系统字号调回
 * 1.0 立刻变回 16px）。本应用是固定像素布局的桌面级 UI，字号被放大后容器盒子并不会
 * 跟着长 —— `max-height: 1.4em` 这类按原始字号算出的高度就不够了，`g` / `y` 这些
 * 带下伸部的字母会被切掉下半截。
 *
 * 原生侧（`FontScaleBridge`）直接接管 WebView 的 `textZoom`（倍率的唯一来源），
 * 于是系统设置被整体覆盖，应用内的「通用设置 → 界面字号」成为唯一来源。
 * 桌面端后端是 no-op。
 *
 * # 调用时机
 *
 * - **启动**：应用生命周期里、配置加载完成后调用一次（`useAppLifecycle`）。
 *   原生侧自己也在 WebView 创建时应用过一次持久化的倍率，这里是"对齐一次"。
 * - **改动**：设置页滑块松手时调用（`@change`）。拖拽过程中每帧下发会让 WebView
 *   反复整页重排，手机上肉眼可见地卡，所以刻意不放在 `@input`。
 */
export async function applyAppFontScale(scale: number): Promise<void> {
  try {
    await setAppFontScale(scale)
  } catch (error) {
    logger.warn('Failed to apply app font scale:', error)
  }
}

/**
 * 把「歌词字号」倍率写到根元素的自定义属性 `--lyrics-scale` 上。
 *
 * 用 CSS 变量而不是给组件逐个传值：歌词同时出现在主歌词面板（`LyricsDisplay`）
 * 和可视化面板的单行歌词（`VisualizerPanel`）里，两边的字号分别写在各自的样式表中，
 * 变量挂在根元素上两边都吃得到，组件也不必知道自己被缩放了。
 *
 * 尺寸都写成 `calc(<原始值> * var(--lyrics-scale, 1))`：未知变量时的兜底值是 1，
 * 所以变量没设上也不会让整条 font-size 失效（自定义属性值不做语法校验，
 * 一旦变量是空串，`calc(30px * )` 会让这条声明被整条丢弃）。
 */
export function applyLyricsFontScale(scale: number): void {
  document.documentElement.style.setProperty('--lyrics-scale', String(scale))
}
