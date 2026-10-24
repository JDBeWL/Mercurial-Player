/** BPM 识别：按需分析当前播放的曲目，结果回填到曲目与元数据缓存。 */
import { invoke } from '@tauri-apps/api/core'
import type { Track } from '@/types'
import logger from '@/utils/logger'

/** 后端返回的单条分析结果 */
export interface BpmAnalysisResult {
  path: string
  bpm: number | null
}

/** 请求后端识别 BPM（标签优先，缺标签才解码）；现在只传当前播放的一首。 */
export function analyzeTracksBpm(paths: string[]): Promise<BpmAnalysisResult[]> {
  return invoke<BpmAnalysisResult[]>('analyze_tracks_bpm', { paths })
}

/** 把结果写回曲目对象；返回本次真正填充的条数 */
function applyResults(tracks: Track[], results: BpmAnalysisResult[] | null | undefined): number {
  // 后端异常时可能返回 null/undefined；调用方是后台任务，不能让它变成 unhandled rejection
  if (!Array.isArray(results) || results.length === 0) return 0
  const byPath = new Map(results.map((result) => [result.path, result.bpm]))
  let applied = 0
  for (const track of tracks) {
    const bpm = byPath.get(track.path)
    // 后端对"没测出"返回 null；不要写入曲目字段，避免 null/undefined 差异传染到快照
    if (typeof bpm === 'number' && bpm > 0) {
      track.bpm = bpm
      applied++
    }
  }
  return applied
}

/** 需要识别的曲目：跳过已知 BPM 的，避免每次切歌都重算 */
export function tracksNeedingBpm(tracks: Track[]): Track[] {
  return tracks.filter((track) => track.path && !(typeof track.bpm === 'number' && track.bpm > 0))
}

/** 识别单首曲目的 BPM 并就地写回；只在当前播放曲目上调用。
 * 要解码并做 FFT，放进列表载入流程会拖慢启动；未识别或失败返回 `null`。 */
export async function analyzeTrackBpm(
  track: Track,
  options: { signal?: AbortSignal } = {},
): Promise<number | null> {
  const { signal } = options
  if (!track.path || signal?.aborted) return null
  // 已有值就不重复分析：切歌频繁时这一步能挡掉绝大多数调用
  if (typeof track.bpm === 'number' && track.bpm > 0) return track.bpm

  let results: BpmAnalysisResult[]
  try {
    results = await analyzeTracksBpm([track.path])
  } catch (error) {
    logger.debug('BPM 识别失败:', error)
    return null
  }

  // 后端返回后可能已经切歌：结果照常写回（该曲目的缓存结论是对的），
  // 但调用方不该拿它去更新"当前播放"的界面
  if (signal?.aborted) return null

  applyResults([track], results)
  return typeof track.bpm === 'number' && track.bpm > 0 ? track.bpm : null
}
