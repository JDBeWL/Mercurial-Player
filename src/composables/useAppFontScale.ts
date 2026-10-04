import { setAppFontScale } from '@/services/appService'
import logger from '@/utils/logger'

/** 下发"界面字号"倍率 (倍数, 1 = 原始大小, 不是百分比) 给原生侧。
 *  Android 的系统字号会被 WebView 当成倍率乘到所有 CSS px 字号上, 而本应用是固定 px 布局、容器盒子不跟着长,
 *  所以由原生侧接管 textZoom 整体覆盖; 桌面端后端为 no-op。滑块上下限与 Kotlin 常量的镜像见 fontScaleBoundsMirror.test.ts */
export async function applyAppFontScale(scale: number): Promise<void> {
  try {
    await setAppFontScale(scale)
  } catch (error) {
    logger.warn('Failed to apply app font scale:', error)
  }
}

/** 歌词字号倍率写成根元素自定义属性 `--lyrics-scale`: 主歌词面板与可视化面板各有各的样式表, 挂根元素两边都吃得到。
 *  尺寸形如 `calc(原始值 * var(--lyrics-scale, 1))`, 缺兜底值时整条 font-size 会被浏览器丢弃 */
export function applyLyricsFontScale(scale: number): void {
  document.documentElement.style.setProperty('--lyrics-scale', String(scale))
}
