/**
 * 歌词行语言检测（启发式）
 *
 * 判定优先级：假名 → 日文；谚文 → 韩文；汉字 → 中文；拉丁字母 → 英文。
 * 假名/谚文的存在性判定几乎无歧义；纯汉字行无法区分中文与日文汉字，
 * 归为中文（日文歌词几乎都含假名，实际误判率极低）。
 *
 * 检测结果用于给歌词行标注 lang 属性，为 CSS :lang() 选择器与
 * 字体的 locl 区域字形变体提供语言上下文
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
