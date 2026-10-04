/**
 * 歌词行语言检测（启发式），按假名 -> 谚文 -> 汉字 -> 拉丁的优先级判定 ja/ko/zh/en，
 * 结果供歌词行的 lang 属性（CSS :lang() 与字体 locl 字形变体）取语言上下文。
 *
 * 假名/谚文判定几乎无歧义；纯汉字行无法区分中文与日文汉字，归为中文（日文歌词几乎都含假名，误判率极低）。
 */

export type LyricLanguage = 'ja' | 'ko' | 'zh' | 'en' | ''

const KANA = /[\u3040-\u309F\u30A0-\u30FF\u31F0-\u31FF]/
const HANGUL = /[\u1100-\u11FF\uAC00-\uD7A3]/
const HAN = /[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]/
const LATIN = /[A-Za-z]/

export function detectLyricLanguage(text: string | undefined | null): LyricLanguage {
  if (!text) {
    return ''
  }
  if (KANA.test(text)) {
    return 'ja'
  }
  if (HANGUL.test(text)) {
    return 'ko'
  }
  if (HAN.test(text)) {
    return 'zh'
  }
  if (LATIN.test(text)) {
    return 'en'
  }
  return ''
}
