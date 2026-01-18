/**
 * 歌词解析器类，支持多种歌词格式
 */
import logger from './logger'
import type { LyricLine, LyricsFormat, KaraokeWord } from '@/types'

// 让出主线程的辅助函数
const yieldToMain = (): Promise<void> => new Promise(resolve => setTimeout(resolve, 0))

export class LyricsParser {
  /**
   * 解析歌词文件（同步版本，用于简单场景）
   */
  static parse(content: string, format: LyricsFormat = 'auto'): LyricLine[] {
    if (!content || typeof content !== 'string') {
      return []
    }

    if (format === 'auto') {
      format = this.detectFormat(content)
    }

    switch (format.toLowerCase() as LyricsFormat) {
      case 'lrc':
        return this.parseLRC(content)
      case 'ass':
        return this.parseASS(content)
      case 'srt':
        return this.parseSRT(content)
      default:
        logger.warn(`Unsupported lyrics format: ${format}`)
        return []
    }
  }

  /**
   * 异步解析歌词文件（支持卡拉OK、翻译，分块处理避免阻塞主线程）
   */
  static async parseAsync(content: string, format: LyricsFormat = 'auto'): Promise<LyricLine[]> {
    if (!content || typeof content !== 'string') {
      return []
    }

    if (format === 'auto') {
      format = this.detectFormat(content)
    }

    switch (format.toLowerCase() as LyricsFormat) {
      case 'lrc':
        return this.parseLRCAsync(content)
      case 'ass':
        return this.parseASSAsync(content)
      default:
        return this.parse(content, format)
    }
  }

  /**
   * 自动检测歌词格式
   */
  static detectFormat(content: string): LyricsFormat {
    if (content.includes('[Script Info]') || content.includes('[V4+ Styles]') || content.includes('[Events]')) {
      return 'ass'
    }
    if (/^\d+\s*\n\d{2}:\d{2}:\d{2},\d{3}\s*-->\s*\d{2}:\d{2}:\d{2},\d{3}\s*\n/m.test(content)) {
      return 'srt'
    }
    return 'lrc'
  }

  /**
   * 异步解析 LRC 格式歌词（支持卡拉OK、翻译、分块处理）
   */
  static async parseLRCAsync(content: string): Promise<LyricLine[]> {
    const lines = content.split("\n")
    const pattern = /\[(\d{2}):(\d{2}):(\d{2})\]|\[(\d{2}):(\d{2})\.(\d{2,3})\]/g
    const resultMap: Record<number, LyricLine> = {}
    const CHUNK_SIZE = 100

    for (let i = 0; i < lines.length; i++) {
      if (i > 0 && i % CHUNK_SIZE === 0) {
        await yieldToMain()
      }

      const line = lines[i]
      const timestamps: Array<{ time: number; index: number }> = []
      let match: RegExpExecArray | null
      while ((match = pattern.exec(line)) !== null) {
        let time: number
        if (match[1] !== undefined) {
          time = parseInt(match[1]) * 60 + parseInt(match[2]) + parseInt(match[3]) / 100
        } else {
          time = parseInt(match[4]) * 60 + parseInt(match[5]) + parseInt(match[6].padEnd(3, "0")) / 1000
        }
        timestamps.push({ time, index: match.index })
      }
      if (timestamps.length < 1) continue
      const text = line.replace(pattern, "").trim()
      if (!text) continue
      const startTime = timestamps[0].time
      resultMap[startTime] = resultMap[startTime] || { time: startTime, texts: [], karaoke: null }
      if (timestamps.length > 1) {
        resultMap[startTime].karaoke = {
          fullText: text,
          timings: timestamps.slice(1).map((s, idx) => ({ time: s.time, position: idx + 1 }))
        }
      }
      resultMap[startTime].texts!.push(text)
    }
    return Object.values(resultMap).sort((a, b) => a.time - b.time)
  }

  /**
   * 异步解析 ASS 格式歌词（支持卡拉OK、翻译、分块处理）
   */
  static async parseASSAsync(content: string): Promise<LyricLine[]> {
    const lines = content.split('\n')
    const dialogues: Array<{ startTime: number; endTime: number; style: string; text: string }> = []
    const toSeconds = (t: string): number => {
      const [h, m, s] = t.split(':')
      return parseInt(h) * 3600 + parseInt(m) * 60 + parseFloat(s)
    }
    const CHUNK_SIZE = 100

    for (let i = 0; i < lines.length; i++) {
      if (i > 0 && i % CHUNK_SIZE === 0) {
        await yieldToMain()
      }

      const line = lines[i]
      if (!line.startsWith('Dialogue:')) continue
      const parts = line.split(',')
      if (parts.length < 10) continue
      const start = parts[1].trim()
      const end = parts[2].trim()
      const style = parts[3].trim()
      const text = parts.slice(9).join(',').trim()
      dialogues.push({ startTime: toSeconds(start), endTime: toSeconds(end), style, text })
    }

    // 智能识别 style 名称
    const isTranslationStyle = (style: string): boolean => {
      const lowerStyle = style.toLowerCase()
      // 翻译相关的 style 关键词
      const translationKeywords = ['ts', 'translation', 'trans', 'cn', 'zh', 'chs', 'cht', 'chinese', 'romaji', 'roma', 'chn', '翻译', '中文']
      return translationKeywords.some(keyword => lowerStyle.includes(keyword))
    }

    const isOriginalStyle = (style: string): boolean => {
      const lowerStyle = style.toLowerCase()
      // 原歌词相关的 style 关键词（优先级低于翻译判断）
      const originalKeywords = ['orig', 'original', 'en', 'english', 'jp', 'ja', 'japanese', 'main', 'default', 'lyric', '原文', '日文', '英文']
      return originalKeywords.some(keyword => lowerStyle.includes(keyword))
    }

    const groupedMap = new Map<string, { startTime: number; endTime: number; texts: { orig: string; ts: string }; styles: Set<string>; karaoke: null }>()
    dialogues.forEach(d => {
      const key = d.startTime.toFixed(3) + '-' + d.endTime.toFixed(3)
      if (!groupedMap.has(key)) {
        groupedMap.set(key, { startTime: d.startTime, endTime: d.endTime, texts: { orig: '', ts: '' }, styles: new Set(), karaoke: null })
      }
      const group = groupedMap.get(key)!
      group.styles.add(d.style)

      // 智能判断是原歌词还是翻译
      if (isTranslationStyle(d.style)) {
        // 明确是翻译的 style
        group.texts.ts = d.text
      } else if (isOriginalStyle(d.style) || group.texts.orig === '') {
        // 明确是原歌词的 style，或者原歌词还是空的（第一个遇到的作为原歌词）
        if (group.texts.orig === '') {
          group.texts.orig = d.text
        } else if (!isTranslationStyle(d.style) && group.texts.ts === '') {
          // 如果原歌词已有内容，且当前不是翻译 style，且翻译为空，则作为翻译
          group.texts.ts = d.text
        }
      } else {
        // 其他情况：如果翻译为空，则作为翻译
        if (group.texts.ts === '') {
          group.texts.ts = d.text
        }
      }
    })

    const result: LyricLine[] = []
    groupedMap.forEach(group => {
      const parseKaraoke = (text: string): KaraokeWord[] => {
        const karaokeTag = /{\\k[f]?(\d+)}([^{}]*)/g
        const words: KaraokeWord[] = []
        let accTime = group.startTime
        let match: RegExpExecArray | null
        while ((match = karaokeTag.exec(text)) !== null) {
          const duration = parseInt(match[1]) * 0.01
          words.push({ text: match[2], start: accTime, end: accTime + duration })
          accTime += duration
        }
        return words
      }
      const enWords = parseKaraoke(group.texts.orig)
      result.push({
        time: group.startTime,
        texts: [group.texts.orig.replace(/{.*?}/g, ''), group.texts.ts.replace(/{.*?}/g, '')],
        words: enWords,
        karaoke: enWords.length > 0 ? { fullText: group.texts.orig, timings: [] } : null
      })
    })
    return result.sort((a, b) => a.time - b.time)
  }

  /**
   * 解析 LRC 格式歌词（同步版本）
   */
  static parseLRC(content: string): LyricLine[] {
    const lines = content.split('\n')
    const lyrics: LyricLine[] = []
    const timeRegex = /^\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)$/

    for (const line of lines) {
      const trimmedLine = line.trim()
      if (!trimmedLine) continue

      const timeMatches = [...trimmedLine.matchAll(/\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\]/g)]
      const textPart = trimmedLine.replace(/\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\]/g, '').trim()

      if (timeMatches.length > 0 && textPart) {
        for (const match of timeMatches) {
          const minutes = parseInt(match[1])
          const seconds = parseInt(match[2])
          const milliseconds = match[3] ? parseInt(match[3].padEnd(3, '0').substring(0, 3)) : 0
          const time = minutes * 60 + seconds + milliseconds / 1000
          lyrics.push({ time, text: textPart })
        }
      } else {
        const singleMatch = trimmedLine.match(timeRegex)
        if (singleMatch) {
          const minutes = parseInt(singleMatch[1])
          const seconds = parseInt(singleMatch[2])
          const milliseconds = singleMatch[3] ? parseInt(singleMatch[3].padEnd(3, '0').substring(0, 3)) : 0
          const time = minutes * 60 + seconds + milliseconds / 1000
          const text = singleMatch[4].trim()
          if (text) {
            lyrics.push({ time, text })
          }
        }
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /**
   * 解析 ASS 格式歌词（同步版本）
   */
  static parseASS(content: string): LyricLine[] {
    const lines = content.split('\n')
    const lyrics: LyricLine[] = []
    let inEvents = false
    let formatFields: string[] = []

    for (const line of lines) {
      const trimmedLine = line.trim()

      if (trimmedLine === '[Events]') {
        inEvents = true
        continue
      }

      if (inEvents && trimmedLine.startsWith('[')) {
        inEvents = false
        continue
      }

      if (inEvents && trimmedLine.startsWith('Format:')) {
        formatFields = trimmedLine.substring(7).split(',').map(field => field.trim())
        continue
      }

      if (inEvents && trimmedLine.startsWith('Dialogue:')) {
        const parts = trimmedLine.substring(9).split(',')

        if (parts.length >= formatFields.length) {
          const startIndex = formatFields.indexOf('Start')
          const textIndex = formatFields.indexOf('Text')

          if (startIndex !== -1 && textIndex !== -1) {
            const startTime = this.parseASSTime(parts[startIndex])
            const text = parts.slice(textIndex).join(',').replace(/{[^}]*}/g, '').trim()

            if (text && startTime !== null) {
              lyrics.push({ time: startTime, text })
            }
          }
        }
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /**
   * 解析 SRT 格式歌词
   */
  static parseSRT(content: string): LyricLine[] {
    const blocks = content.trim().split(/\n\s*\n/)
    const lyrics: LyricLine[] = []
    const timeRegex = /(\d{1,2}):(\d{2}):(\d{2}),(\d{3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2}),(\d{3})/

    for (const block of blocks) {
      const lines = block.trim().split('\n')
      if (lines.length < 2) continue

      const timeMatch = lines[1].match(timeRegex)
      if (!timeMatch) continue

      const startTime = parseInt(timeMatch[1]) * 3600 + parseInt(timeMatch[2]) * 60 +
        parseInt(timeMatch[3]) + parseInt(timeMatch[4]) / 1000
      const text = lines.slice(2).join('\n').trim()

      if (text) {
        lyrics.push({ time: startTime, text })
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /**
   * 解析 ASS 时间格式
   */
  static parseASSTime(timeStr: string): number | null {
    const match = timeStr.match(/^(\d+):(\d{2}):(\d{2})\.(\d{2})$/)
    if (match) {
      return parseInt(match[1]) * 3600 + parseInt(match[2]) * 60 +
        parseInt(match[3]) + parseInt(match[4]) / 100
    }
    return null
  }

  /**
   * 将歌词数组转换为指定格式的字符串
   */
  static stringify(lyrics: LyricLine[], format: LyricsFormat = 'lrc'): string {
    if (!lyrics || !Array.isArray(lyrics)) {
      return ''
    }

    switch (format.toLowerCase() as LyricsFormat) {
      case 'lrc':
        return this.stringifyLRC(lyrics)
      case 'ass':
        return this.stringifyASS(lyrics)
      case 'srt':
        return this.stringifySRT(lyrics)
      default:
        logger.warn(`Unsupported export format: ${format}`)
        return ''
    }
  }

  static stringifyLRC(lyrics: LyricLine[]): string {
    return lyrics.map(item => {
      const minutes = Math.floor(item.time / 60)
      const seconds = Math.floor(item.time % 60)
      const milliseconds = Math.floor((item.time % 1) * 100)
      const timeTag = `[${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}.${milliseconds.toString().padStart(2, '0')}]`
      return `${timeTag}${item.text || ''}`
    }).join('\n')
  }

  static stringifyASS(lyrics: LyricLine[]): string {
    let ass = `[Script Info]
Title: Lyrics
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
`
    return ass + lyrics.map((item, index) => {
      const formatTime = (t: number): string => {
        const h = Math.floor(t / 3600)
        const m = Math.floor((t % 3600) / 60)
        const s = Math.floor(t % 60)
        const cs = Math.floor((t % 1) * 100)
        return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}.${cs.toString().padStart(2, '0')}`
      }
      const nextTime = index < lyrics.length - 1 ? lyrics[index + 1].time : item.time + 5
      return `Dialogue: 0,${formatTime(item.time)},${formatTime(nextTime)},Default,,0,0,0,,${item.text || ''}`
    }).join('\n')
  }

  static stringifySRT(lyrics: LyricLine[]): string {
    return lyrics.map((item, index) => {
      const formatTime = (t: number): string => {
        const h = Math.floor(t / 3600)
        const m = Math.floor((t % 3600) / 60)
        const s = Math.floor(t % 60)
        const ms = Math.floor((t % 1) * 1000)
        return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')},${ms.toString().padStart(3, '0')}`
      }
      const nextTime = index < lyrics.length - 1 ? lyrics[index + 1].time : item.time + 5
      return `${index + 1}\n${formatTime(item.time)} --> ${formatTime(nextTime)}\n${item.text || ''}\n`
    }).join('\n')
  }
}

export default LyricsParser
