import { invoke } from '@tauri-apps/api/core'
import type { LyricBundle, LyricCandidate, LyricQuery } from './types'
import { resolveKind, resolveMethod } from './registry'
import { buildFinalLyric } from './builder'
import { findBestLyricMatch } from '@/utils/lyricMatching'
import type { LyricsConfig, LyricsProviderId, ProviderLyricSetting } from '@/types'

/** 后端返回的原始候选（无 provider/method 字段） */
interface RawCandidate {
  id: string
  title: string
  artist: string
  album: string
  duration_ms: number
  bundle: LyricBundle
}

/** 单来源取候选（附加来源与所用算法） */
export async function listCandidates(
  provider: LyricsProviderId,
  query: LyricQuery,
  settings: ProviderLyricSetting | undefined,
  limit = 5,
): Promise<LyricCandidate[]> {
  const method = resolveMethod(provider, settings?.method)
  try {
    const raws = await invoke<RawCandidate[]>('lyrics_search_candidates', {
      provider,
      method,
      title: query.title,
      artist: query.artist,
      duration: query.duration_ms,
      limit,
    })
    return (raws || []).map((raw) => ({
      ...raw,
      provider,
      method,
    }))
  } catch (error) {
    // best-effort：单来源失败返回空，绝不中断整体流程
    console.debug(`[lyricProviders] ${provider} 候选获取失败:`, error)
    return []
  }
}

/** 按配置的启用来源顺序聚合候选（每个来源附其算法/类型偏好） */
export async function collectCandidates(
  config: LyricsConfig,
  query: LyricQuery,
): Promise<LyricCandidate[]> {
  const order =
    config.lyricProviderOrder && config.lyricProviderOrder.length > 0
      ? config.lyricProviderOrder
      : [config.onlineSource || 'netease']
  const settings = config.lyricProviderSettings ?? {}

  const results: LyricCandidate[] = []
  for (const id of order) {
    const provider = id as LyricsProviderId
    const byProvider = await listCandidates(provider, query, settings[provider])
    results.push(...byProvider)
  }
  return results
}

/** 按顺延优先级自动挑选最佳歌词，返回最终歌词内容+格式（用于无感自动流程） */
export async function fetchBestLyrics(
  config: LyricsConfig,
  query: LyricQuery,
): Promise<{
  content: string
  format: 'lrc' | 'ass'
  source: string
  candidates: LyricCandidate[]
} | null> {
  const order =
    config.lyricProviderOrder && config.lyricProviderOrder.length > 0
      ? config.lyricProviderOrder
      : [config.onlineSource || 'netease']
  const settings = config.lyricProviderSettings ?? {}

  for (const id of order) {
    const provider = id as LyricsProviderId
    const providerSetting = settings[provider]
    const byProvider = await listCandidates(provider, query, providerSetting)
    const best = findBestLyricMatch(byProvider, query.title, query.artist, query.duration_ms)
    if (best) {
      const kind = resolveKind(provider, providerSetting?.preferKind)
      const final = buildFinalLyric(best.bundle, kind, config.preferTranslation)
      return {
        content: final.content,
        format: final.format,
        source: provider,
        candidates: byProvider,
      }
    }
  }
  return null
}
