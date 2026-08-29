import { getScreenRefreshRate, setTargetFps } from '@/services/appService'
import logger from '@/utils/logger'
import type { VisualizerConfig } from '@/types'

export interface AppliedFpsResult {
  /** 实际应用到后端的帧率 */
  fps: number
  /** 实时查询到的屏幕刷新率；未启用限制或查询失败时为 null */
  screenRate: number | null
}

/**
 * 按可视化配置把目标帧率应用到后端（设置页与启动序列共用）。
 * 开启"限制到屏幕刷新率"（历史字段名 enableVerticalSync）时，
 * 取 min(目标帧率, 实时屏幕刷新率)；实时查询失败则按目标帧率应用。
 * 配置缺失时不做任何操作，返回 null。
 */
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
