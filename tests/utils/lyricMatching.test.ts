import { describe, it, expect } from 'vitest'
import { normalizeLyricStr, findBestLyricMatch } from '@/utils/lyricMatching'

describe('normalizeLyricStr', () => {
  it('去除空白与标点', () => {
    expect(normalizeLyricStr('A - B (Live)')).toBe('ab')
  })
})

describe('findBestLyricMatch', () => {
  const items = [
    { title: 'Song Title', artist: 'Artist Name', duration_ms: 180000 },
    { title: 'Other', artist: 'Someone', duration_ms: 90000 },
  ]

  it('空列表返回 null', () => {
    expect(findBestLyricMatch([], 'x', '', 0)).toBeNull()
  })

  it('精确标题匹配', () => {
    expect(findBestLyricMatch(items, 'Song Title', '', 0)).toEqual(items[0])
  })

  it('标题加艺人匹配', () => {
    expect(findBestLyricMatch(items, 'Song', 'Artist Name', 0)).toEqual(items[0])
  })

  it('时长接近加分', () => {
    expect(findBestLyricMatch(items, 'Song Title', '', 180000)).toEqual(items[0])
  })

  it('匹配分不足时返回 null', () => {
    expect(findBestLyricMatch(items, 'Completely Unrelated', 'Nobody', 0)).toBeNull()
  })
})
