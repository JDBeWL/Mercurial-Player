import { setAppFontScale } from '@/services/appService'
import logger from '@/utils/logger'

/** 下发界面字号倍率 (1 = 原始大小, 非百分比)；Android 由原生侧接管 `textZoom` 整体覆盖, 桌面端为 no-op */
export async function applyAppFontScale(scale: number): Promise<void> {
  try {
    await setAppFontScale(scale)
  } catch (error) {
    logger.warn('Failed to apply app font scale:', error)
  }
}

/** 歌词字号写成根元素自定义属性 `--lyrics-scale`, 主歌词面板与可视化面板的样式表都能读到；
 *  `calc()` 里必须带 `var(--lyrics-scale, 1)` 兜底, 否则整条 `font-size` 会被浏览器丢弃 */
export function applyLyricsFontScale(scale: number): void {
  document.documentElement.style.setProperty('--lyrics-scale', String(scale))
}
