import { setAppFontScale } from '@/services/appService'
import logger from '@/utils/logger'

/** 把「界面字号」倍率下发给原生侧。
 *  Android 的系统字号会被 WebView 当成倍率乘到所有 CSS px 字号上，而本应用是固定像素
 *  布局、容器盒子不跟着长，因此由原生侧接管 `textZoom` 整体覆盖；桌面端后端为 no-op。 */
export async function applyAppFontScale(scale: number): Promise<void> {
  try {
    await setAppFontScale(scale)
  } catch (error) {
    logger.warn('Failed to apply app font scale:', error)
  }
}

/** 把「歌词字号」倍率写在根元素的自定义属性 `--lyrics-scale` 上：主歌词面板与可视化
 *  面板各有各的样式表，变量挂根元素两边都吃得到，组件不必知道自己被缩放。尺寸写成
 *  `calc(原始值 * var(--lyrics-scale, 1))`，缺兜底值时整条 font-size 会被丢弃。 */
export function applyLyricsFontScale(scale: number): void {
  document.documentElement.style.setProperty('--lyrics-scale', String(scale))
}
