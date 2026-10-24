/** 节拍分析（Beat This!）：整曲拍点与小节结构。
 * 只用于用户显式开启的批量分析，结果进 `beat-cache.json`；不要放进播放路径。 */
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import logger from '@/utils/logger'

/** 批量进度事件名，与后端 `BEATS_PROGRESS_EVENT` 必须一致 */
export const BEATS_PROGRESS_EVENT = 'beats-analysis-progress'

/** 一首曲目的整曲节拍分析结果 */
export interface BeatTrack {
  bpm: number | null
  /** 拍点时间（秒） */
  beats: number[]
  /** 小节线时间（秒），是 beats 的子集 */
  downbeats: number[]
  /** 每小节拍数分布，首项即主导拍号 */
  meter: { beats_per_bar: number; count: number }[]
  span_secs: number
}

export type BeatsProgressStatus =
  | 'started'
  | 'analyzed'
  | 'skipped'
  | 'missing'
  | 'failed'
  | 'finished'
  | 'cancelled'

/** 批量分析的进度 */
export interface BeatsProgress {
  done: number
  total: number
  /** 当前曲目文件名（`started` 时就是正在分析的那首） */
  current: string
  /** 完整路径：文件名可能重名，前端定位必须用路径 */
  path: string
  bpm: number | null
  status: BeatsProgressStatus
  /** 这一首的实际耗时（毫秒）；`started` 与 `finished` 为 0 */
  track_ms: number
}

/** 一类跳过/失败原因及数量 */
export interface BeatIssue {
  message: string
  count: number
}

/** 批量分析汇总 */
export interface BeatsBatchSummary {
  analyzed: number
  skipped: number
  failed: number
  cancelled: boolean
  /** 按原因归并的明细，已按数量降序 */
  issues: BeatIssue[]
}

/** 节拍分析可选组件的安装状态 */
export interface BeatComponentStatus {
  /** 组件目录绝对路径（可能尚未创建） */
  dir: string
  /** 功能是否可用。含开发态从源码树回退的情形，不等于"用户装没装" */
  ready: boolean
  /** GPU 运行时就位，推理会走显卡 */
  gpu: boolean
  /** 还需要补的文件；功能已可用时只会列运行时 */
  missing: string[]
}

/** 节拍模型是否随包分发（缺模型时整个功能不可用） */
export function beatModelAvailable(): Promise<boolean> {
  return invoke<boolean>('beat_model_available')
}

/** 查询可选组件（模型 + GPU 运行时）装没装 */
export function beatComponentStatus(): Promise<BeatComponentStatus> {
  return invoke<BeatComponentStatus>('beat_component_status')
}

/** 打开组件目录（不存在则连同 models/ runtime/ 一起创建好） */
export function openBeatComponentDir(): Promise<void> {
  return invoke<void>('open_beat_component_dir')
}

/** 推理会走哪条路：`directml`（显卡）或 `rten`（CPU）。只查运行库，不加载模型 */
export function beatBackend(): Promise<'directml' | 'rten'> {
  return invoke<'directml' | 'rten'>('beat_backend')
}

/** 上次分析实际用上的后端；没分析过返回空串，说明还看不出真实快慢 */
export function beatBackendActual(): Promise<string> {
  return invoke<string>('beat_backend_actual')
}

/** 单首分析（会先查缓存）；未命中时最长阻塞约 9 秒 */
export function analyzeBeats(path: string): Promise<BeatTrack> {
  return invoke<BeatTrack>('analyze_beats', { path })
}

/** 批量分析整库；调用方必须同时订阅 {@link onBeatsProgress}。
 * `force = false` 时跳过已有缓存，让"中断后继续"成为默认行为。 */
export function analyzeBeatsBatch(paths: string[], force = false): Promise<BeatsBatchSummary> {
  return invoke<BeatsBatchSummary>('analyze_beats_batch', { paths, force })
}

/** 请求中止批量分析；当前正在算的那一首仍会算完（≤9 秒） */
export function cancelBeatsBatch(): Promise<void> {
  return invoke<void>('cancel_beats_batch')
}

/** 缓存统计：[已分析且仍有效的曲目数, 缓存文件 KB] */
export function beatCacheStats(): Promise<[number, number]> {
  return invoke<[number, number]>('beat_cache_stats')
}

/** 订阅批量进度；返回取消订阅函数 */
export async function onBeatsProgress(
  handler: (progress: BeatsProgress) => void,
): Promise<UnlistenFn> {
  return listen<BeatsProgress>(BEATS_PROGRESS_EVENT, (event) => {
    try {
      handler(event.payload)
    } catch (error) {
      // 回调抛错不该中断事件循环，否则后续进度全部丢失
      logger.error('节拍进度回调异常:', error)
    }
  })
}

/**
 * 估算批量耗时（秒）：rten 按 7.5 秒/首，DirectML 按 1.2 秒/首。
 * 只给用户量级预期，不要拿它做超时。
 */
export function estimateBatchSeconds(trackCount: number, backend?: 'directml' | 'rten'): number {
  return Math.ceil(trackCount * (backend === 'directml' ? 1.2 : 7.5))
}
