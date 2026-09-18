/** 歌词候选匹配工具：归一化与打分，用于在"多来源多候选"里挑选最贴合当前歌曲的歌词 */

/** 可参与匹配的最小结构 */
export interface MatchableSong {
  title: string
  artist: string
  /** 时长(毫秒)，0 表示未知 */
  duration_ms?: number
}

/** 归一化歌名/艺人：去空白、连字符、下划线、括号及其中内容 */
export function normalizeLyricStr(str: string): string {
  return str
    .toLowerCase()
    .replace(/[\s\-_.]/g, '')
    .replace(/[（(][^）)]*[）)]/g, '')
    .trim()
}

/** 从候选里挑最匹配的歌词（返回打分最高项），低于阈值返回 null */
export function findBestLyricMatch<T extends MatchableSong>(
  items: T[],
  title: string,
  artist: string,
  durationMs = 0,
): T | null {
  if (items.length === 0) return null

  let bestScore = -1
  let bestMatch: T | null = null

  const normalizedTitle = normalizeLyricStr(title)
  const normalizedArtist = normalizeLyricStr(artist)

  for (const item of items) {
    let score = 0

    const songTitle = normalizeLyricStr(item.title)
    if (songTitle === normalizedTitle) {
      score += 100
    } else if (songTitle.includes(normalizedTitle) || normalizedTitle.includes(songTitle)) {
      score += 50
    }

    if (artist) {
      const songArtist = normalizeLyricStr(item.artist)
      if (songArtist === normalizedArtist) {
        score += 50
      } else if (songArtist.includes(normalizedArtist) || normalizedArtist.includes(songArtist)) {
        score += 25
      }
    }

    if (durationMs > 0 && item.duration_ms && item.duration_ms > 0) {
      const durationDiff = Math.abs(durationMs - item.duration_ms)
      if (durationDiff < 3000) {
        score += 30
      } else if (durationDiff < 10000) {
        score += 15
      }
    }

    if (score > bestScore) {
      bestScore = score
      bestMatch = item
    }
  }

  return bestScore >= 50 ? bestMatch : null
}
