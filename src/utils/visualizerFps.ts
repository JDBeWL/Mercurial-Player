import { getScreenRefreshRate, setTargetFps } from '@/services/appService'
import logger from '@/utils/logger'
import type { VisualizerConfig } from '@/types'

interface AppliedFpsResult {
  /** 实际下发到后端的帧率 */
  fps: number
  /** 实时查询到的屏幕刷新率, 未启用限制或查询失败时为 null */
  screenRate: number | null
}

/** 按配置把目标帧率下发后端；配置缺失时不动后端并返回 `null`。
 * `enableVerticalSync` 是历史字段名，实义为限制到屏幕刷新率（取 min(targetFps, 刷新率)）。 */
export async function applyVisualizerFps(
  visualizer: VisualizerConfig | undefined | null,
): Promise<AppliedFpsResult | null> {
  const targetFps = visualizer?.targetFps
  if (!targetFps) {
    return null
  }

  let fps = targetFps
  let screenRate: number | null = null
  if (visualizer?.enableVerticalSync) {
    try {
      screenRate = await getScreenRefreshRate()
      fps = Math.min(targetFps, screenRate)
    } catch (error) {
      screenRate = null
      logger.warn('Failed to query screen refresh rate, applying target FPS as-is:', error)
    }
  }

  await setTargetFps(fps)
  return { fps, screenRate }
}
