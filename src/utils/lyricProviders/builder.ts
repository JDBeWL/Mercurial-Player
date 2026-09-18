import type { LyricBundle } from './types'
import type { LyricKind } from '@/types'

/** 最终歌词内容与格式（逐字走 ASS，否则 LRC） */
export interface LyricFinal {
  content: string
  format: 'lrc' | 'ass'
}

/** 按时间戳合并原文与翻译/罗马音（时间轴并集，翻译去重） */
export function mergeLyricTexts(original: string, extra: string): string {
  if (!extra) return original
  if (!original) return ''

  const parseLrc = (text: string): Record<string, string> => {
    const results: Record<string, string> = {}
    const timeRegex = /\[(\d{2}):(\d{2})\.(\d{2,3})\]/g
    for (const line of text.split('\n')) {
      const matches = [...line.matchAll(timeRegex)]
      if (matches.length === 0) continue
      const lineText = line.replace(timeRegex, '').trim()
      if (!lineText) continue
      for (const match of matches) {
        results[`${match[1]}:${match[2]}.${match[3]}`] = lineText
      }
    }
    return results
  }

  const orig = parseLrc(original)
  const extraMap = parseLrc(extra)
  const merged: string[] = []
  const times = [...new Set([...Object.keys(orig), ...Object.keys(extraMap)])].sort()

  for (const time of times) {
    const origText = orig[time] || ''
    const extraText = extraMap[time] || ''
    if (origText) {
      merged.push(`[${time}]${origText}`)
      if (extraText && extraText !== origText) {
        merged.push(`[${time}]${extraText}`)
      }
    }
  }
  return merged.join('\n')
}

/** 按用户偏好的文本类型从 bundle 构建最终歌词：带逐字（karaoke ASS）时直接用 ASS；
 *  否则生成 LRC —— original 仅原文 / translation 原文+翻译 / roman 原文+罗马音 / auto 跟随全局。 */
export function buildFinalLyric(
  bundle: LyricBundle,
  kind: LyricKind,
  preferTranslationGlobal: boolean,
): LyricFinal {
  // 逐字优先：ASS 已含原文逐字 + 译文/罗马音普通行
  if (bundle.karaoke && bundle.karaoke.trim()) {
    return { content: bundle.karaoke, format: 'ass' }
  }

  const original = bundle.lrc || ''
  if (!original) return { content: '', format: 'lrc' }

  switch (kind) {
    case 'original':
      return { content: original, format: 'lrc' }
    case 'translation':
      return { content: mergeLyricTexts(original, bundle.tlyric || ''), format: 'lrc' }
    case 'roman':
      return { content: mergeLyricTexts(original, bundle.romalrc || ''), format: 'lrc' }
    case 'auto':
    default:
      return {
        content: preferTranslationGlobal
          ? mergeLyricTexts(original, bundle.tlyric || '')
          : original,
        format: 'lrc',
      }
  }
}

/** 候选预览用文本：与 `buildFinalLyric` 的区别是**永不返回 ASS**（ASS 塞进预览只会看到一堆
 *  `Dialogue:`，且 save 路径命中 ASS 时会忽略 `kind`，预览里切「译文/罗马音」就没反应了）。 */
export function buildPreviewLyric(
  bundle: LyricBundle,
  kind: LyricKind,
  preferTranslationGlobal: boolean,
): string {
  const original = bundle.lrc || ''
  if (!original) return ''

  switch (kind) {
    case 'original':
      return original
    case 'translation':
      return mergeLyricTexts(original, bundle.tlyric || '')
    case 'roman':
      return mergeLyricTexts(original, bundle.romalrc || '')
    case 'auto':
    default:
      return preferTranslationGlobal ? mergeLyricTexts(original, bundle.tlyric || '') : original
  }
}
