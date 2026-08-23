import { describe, it, expect } from 'vitest'
import { detectLyricLanguage } from '../../src/utils/languageDetect'

describe('detectLyricLanguage', () => {
  it('检测日文（假名存在即判定）', () => {
    expect(detectLyricLanguage('君のことが好きだ')).toBe('ja')
    expect(detectLyricLanguage('ドキドキ')).toBe('ja')
    // 汉字与假名混排仍是日文
    expect(detectLyricLanguage('涙そうそう')).toBe('ja')
  })

  it('检测韩文（谚文）', () => {
    expect(detectLyricLanguage('사랑해')).toBe('ko')
    expect(detectLyricLanguage('너무 보고 싶어')).toBe('ko')
  })

  it('检测中文（汉字且无假名/谚文）', () => {
    expect(detectLyricLanguage('后来终于在眼泪中明白')).toBe('zh')
    // 中文歌词夹英文单词仍是中文
    expect(detectLyricLanguage('我说的 love you 都是真的')).toBe('zh')
  })

  it('纯汉字的日文行归为中文（已知局限：假名缺失时无法区分）', () => {
    expect(detectLyricLanguage('東京')).toBe('zh')
  })

  it('检测英文（仅拉丁字母）', () => {
    expect(detectLyricLanguage('Take me to your heart')).toBe('en')
    // 变音符号属于拉丁扩展，同样归为英文
    expect(detectLyricLanguage('À toi la vie, señör')).toBe('en')
  })

  it('空值与无字母内容返回空串', () => {
    expect(detectLyricLanguage('')).toBe('')
    expect(detectLyricLanguage(undefined)).toBe('')
    expect(detectLyricLanguage(null)).toBe('')
    expect(detectLyricLanguage('♪ ～ ♪')).toBe('')
    expect(detectLyricLanguage('...')).toBe('')
    expect(detectLyricLanguage('12345')).toBe('')
  })
})
