/** 歌词解析器: LRC / ASS / SRT 互转, 时间统一为秒 */
import logger from './logger'
import type { LyricLine, LyricsFormat, KaraokeWord } from '@/types'

const yieldToMain = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

/** 二分查找最后一条 time <= time 的行, 无则 -1; 要求 lyrics 按 time 升序, 偏移由调用方换算 */
export function findLyricIndex(lyrics: Pick<LyricLine, 'time'>[], time: number): number {
  let l = 0
  let r = lyrics.length - 1
  let idx = -1
  while (l <= r) {
    const mid = (l + r) >> 1
    if (lyrics[mid]!.time <= time) {
      idx = mid
      l = mid + 1
    } else {
      r = mid - 1
    }
  }
  return idx
}

export class LyricsParser {
  /** 同步解析, 不处理卡拉OK与翻译行 */
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

  /** 异步解析, 保留卡拉OK与翻译行; 其余走 parse */
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

  /** 按区块标记识别 ass/srt, 都不匹配时按 lrc 处理 */
  static detectFormat(content: string): LyricsFormat {
    if (
      content.includes('[Script Info]') ||
      content.includes('[V4+ Styles]') ||
      content.includes('[Events]')
    ) {
      return 'ass'
    }
    if (/^\d+\s*\n\d{2}:\d{2}:\d{2},\d{3}\s*-->\s*\d{2}:\d{2}:\d{2},\d{3}\s*\n/m.test(content)) {
      return 'srt'
    }
    return 'lrc'
  }

  /** 异步 LRC: 一行多时间戳视作同一行, 第二个起作为卡拉OK逐字时间点 */
  static async parseLRCAsync(content: string): Promise<LyricLine[]> {
    const lines = content.split('\n')
    const pattern = /\[(\d{2}):(\d{2}):(\d{2})\]|\[(\d{2}):(\d{2})\.(\d{2,3})\]/g
    const resultMap: Record<number, LyricLine> = {}
    const CHUNK_SIZE = 100

    for (let i = 0; i < lines.length; i++) {
      // 每 CHUNK_SIZE 行让出一次主线程, 防止长文件解析卡住 UI
      if (i > 0 && i % CHUNK_SIZE === 0) {
        await yieldToMain()
      }

      const line = lines[i]!
      const linePattern = new RegExp(pattern)
      const timestamps: Array<{ time: number; index: number }> = []
      let match: RegExpExecArray | null
      while ((match = linePattern.exec(line)) !== null) {
        let time: number
        if (match[1] !== undefined) {
          // 三段时间戳 [mm:ss:cs], 末段是百分秒
          time = parseInt(match[1]) * 60 + parseInt(match[2]!) + parseInt(match[3]!) / 100
        } else {
          // 小数段给 2 位时按百分秒读, 补齐 3 位再换算毫秒
          time =
            parseInt(match[4]!) * 60 +
            parseInt(match[5]!) +
            parseInt(match[6]!.padEnd(3, '0')) / 1000
        }
        timestamps.push({ time, index: match.index })
      }
      if (timestamps.length < 1) continue
      const text = line.replace(linePattern, '').trim()
      if (!text) continue
      // 一行多时间戳 = 同一句在多个时间点出现（副歌重复的常见写法），与同步版 parseLRC 保持一致，
      // 每个时间点各存一条。原来只存第一个时间、其余塞进 karaoke.timings，而没有任何渲染读
      // timings，结果这种行一到当前行就整行空白
      for (const stamp of timestamps) {
        const entry = (resultMap[stamp.time] ??= { time: stamp.time, texts: [], karaoke: null })
        if (!entry.texts!.includes(text)) entry.texts!.push(text)
      }
    }
    return Object.values(resultMap).sort((a, b) => a.time - b.time)
  }

  /** 异步 ASS: 不读 Format 行, 按 v4.00+ 固定列序取 Start/End/Style/Text */
  static async parseASSAsync(content: string): Promise<LyricLine[]> {
    const lines = content.split('\n')
    const dialogues: Array<{ startTime: number; endTime: number; style: string; text: string }> = []
    const toSeconds = (t: string): number => {
      // ASS 时间形如 h:mm:ss.cc, 秒段的小数部分即百分秒
      const [h, m, s] = t.split(':')
      return parseInt(h!) * 3600 + parseInt(m!) * 60 + parseFloat(s!)
    }
    const CHUNK_SIZE = 100

    for (let i = 0; i < lines.length; i++) {
      // 分块让出主线程, 同 parseLRCAsync
      if (i > 0 && i % CHUNK_SIZE === 0) {
        await yieldToMain()
      }

      const line = lines[i]!
      if (!line.startsWith('Dialogue:')) continue
      const parts = line.split(',')
      if (parts.length < 10) continue
      const start = parts[1]!.trim()
      const end = parts[2]!.trim()
      const style = parts[3]!.trim()
      // 第 10 列起都是 Text: 正文里的逗号不能被当列边界, split 后要用逗号拼回
      const text = parts.slice(9).join(',').trim()
      dialogues.push({ startTime: toSeconds(start), endTime: toSeconds(end), style, text })
    }

    const isTranslationStyle = (style: string): boolean => {
      const lowerStyle = style.toLowerCase()
      // 翻译判定优先于原文: style 名小写包含任一关键词即算译文
      const translationKeywords = [
        'ts',
        'translation',
        'trans',
        'cn',
        'zh',
        'chs',
        'cht',
        'chinese',
        'romaji',
        'roma',
        'chn',
        '翻译',
        '中文',
      ]
      return translationKeywords.some((keyword) => lowerStyle.includes(keyword))
    }

    const isOriginalStyle = (style: string): boolean => {
      const lowerStyle = style.toLowerCase()
      // 原文关键词, 优先级低于翻译判定
      const originalKeywords = [
        'orig',
        'original',
        'en',
        'english',
        'jp',
        'ja',
        'japanese',
        'main',
        'default',
        'lyric',
        '原文',
        '日文',
        '英文',
      ]
      return originalKeywords.some((keyword) => lowerStyle.includes(keyword))
    }

    const groupedMap = new Map<
      string,
      {
        startTime: number
        endTime: number
        texts: { orig: string; ts: string }
        styles: Set<string>
        karaoke: null
      }
    >()
    dialogues.forEach((d) => {
      // 分组键取起止时间(3 位小数): 同一时间窗内不同 style 的行合成一条歌词
      const key = d.startTime.toFixed(3) + '-' + d.endTime.toFixed(3)
      if (!groupedMap.has(key)) {
        groupedMap.set(key, {
          startTime: d.startTime,
          endTime: d.endTime,
          texts: { orig: '', ts: '' },
          styles: new Set(),
          karaoke: null,
        })
      }
      const group = groupedMap.get(key)!
      group.styles.add(d.style)

      if (isTranslationStyle(d.style)) {
        group.texts.ts = d.text
      } else if (isOriginalStyle(d.style) || group.texts.orig === '') {
        // 原文槽为空时首个非译文行占位, 之后的再落进译文槽
        if (group.texts.orig === '') {
          group.texts.orig = d.text
        } else if (!isTranslationStyle(d.style) && group.texts.ts === '') {
          group.texts.ts = d.text
        }
      } else {
        // 角色无法识别的 style 只补译文槽, 不覆盖已有原文
        if (group.texts.ts === '') {
          group.texts.ts = d.text
        }
      }
    })

    const result: LyricLine[] = []
    groupedMap.forEach((group) => {
      const parseKaraoke = (text: string): KaraokeWord[] => {
        // ASS 逐字标签 {\k} / {\kf}, 数值单位是百分秒, 时间点按累加时长推
        const karaokeTag = /{\\k[f]?(\d+)}([^{}]*)/g
        const words: KaraokeWord[] = []
        let accTime = group.startTime
        let match: RegExpExecArray | null
        while ((match = karaokeTag.exec(text)) !== null) {
          const duration = parseInt(match[1]!) * 0.01
          words.push({ text: match[2]!, start: accTime, end: accTime + duration })
          accTime += duration
        }
        return words
      }
      const enWords = parseKaraoke(group.texts.orig)
      const plainText = group.texts.orig.replace(/{.*?}/g, '')
      // 无逐字标记时合成一个覆盖整行的虚拟 word
      const finalWords =
        enWords.length > 0
          ? enWords
          : plainText.length > 0 && group.endTime > group.startTime
            ? [{ text: plainText, start: group.startTime, end: group.endTime }]
            : []
      result.push({
        time: group.startTime,
        texts: [plainText, group.texts.ts.replace(/{.*?}/g, '')],
        words: finalWords,
        karaoke: finalWords.length > 0 ? { fullText: group.texts.orig, timings: [] } : null,
      })
    })
    return result.sort((a, b) => a.time - b.time)
  }

  /** 同步 LRC: 一行多时间戳共享同一文本; 小数段 2 位按百分秒补齐为毫秒 */
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
          const minutes = parseInt(match[1]!)
          const seconds = parseInt(match[2]!)
          const milliseconds = match[3] ? parseInt(match[3].padEnd(3, '0').substring(0, 3)) : 0
          const time = minutes * 60 + seconds + milliseconds / 1000
          lyrics.push({ time, text: textPart, texts: [textPart] })
        }
      } else {
        const singleMatch = trimmedLine.match(timeRegex)
        if (singleMatch) {
          const minutes = parseInt(singleMatch[1]!)
          const seconds = parseInt(singleMatch[2]!)
          const milliseconds = singleMatch[3]
            ? parseInt(singleMatch[3].padEnd(3, '0').substring(0, 3))
            : 0
          const time = minutes * 60 + seconds + milliseconds / 1000
          const text = singleMatch[4]!.trim()
          if (text) {
            lyrics.push({ time, text, texts: [text] })
          }
        }
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /** 同步 ASS: 按 Format 行给出的列序定位 Start/Text, 兼容非标准字段排布 */
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
        formatFields = trimmedLine
          .substring(7)
          .split(',')
          .map((field) => field.trim())
        continue
      }

      if (inEvents && trimmedLine.startsWith('Dialogue:')) {
        const parts = trimmedLine.substring(9).split(',')

        if (parts.length >= formatFields.length) {
          const startIndex = formatFields.indexOf('Start')
          const textIndex = formatFields.indexOf('Text')

          if (startIndex !== -1 && textIndex !== -1) {
            const startTime = this.parseASSTime(parts[startIndex]!)
            const text = parts
              .slice(textIndex)
              .join(',')
              .replace(/{[^}]*}/g, '')
              .trim()

            if (text && startTime !== null) {
              lyrics.push({ time: startTime, text, texts: [] })
            }
          }
        }
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /** SRT: 空行分块, 时间行在第 2 行, 其余行合并为文本; 只取 cue 起始时间 */
  static parseSRT(content: string): LyricLine[] {
    const blocks = content.trim().split(/\n\s*\n/)
    const lyrics: LyricLine[] = []
    const timeRegex = /(\d{1,2}):(\d{2}):(\d{2}),(\d{3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2}),(\d{3})/

    for (const block of blocks) {
      const lines = block.trim().split('\n')
      if (lines.length < 2) continue

      const timeMatch = lines[1]!.match(timeRegex)
      if (!timeMatch) continue

      const startTime =
        parseInt(timeMatch[1]!) * 3600 +
        parseInt(timeMatch[2]!) * 60 +
        parseInt(timeMatch[3]!) +
        parseInt(timeMatch[4]!) / 1000
      const text = lines.slice(2).join('\n').trim()

      if (text) {
        lyrics.push({ time: startTime, text, texts: [text] })
      }
    }

    lyrics.sort((a, b) => a.time - b.time)
    return lyrics
  }

  /** ASS 时间 h:mm:ss.cc, 末段是百分秒; 格式不符返回 null 由调用方丢弃该行 */
  static parseASSTime(timeStr: string): number | null {
    const match = timeStr.match(/^(\d+):(\d{2}):(\d{2})\.(\d{2})$/)
    if (match) {
      return (
        parseInt(match[1]!) * 3600 +
        parseInt(match[2]!) * 60 +
        parseInt(match[3]!) +
        parseInt(match[4]!) / 100
      )
    }
    return null
  }

  /** 序列化为目标格式字符串, 未识别格式返回空串并告警 */
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
    return lyrics
      .map((item) => {
        const minutes = Math.floor(item.time / 60)
        const seconds = Math.floor(item.time % 60)
        // LRC 标签只到百分秒, 多余精度直接截断
        const milliseconds = Math.floor((item.time % 1) * 100)
        const timeTag = `[${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}.${milliseconds.toString().padStart(2, '0')}]`
        return `${timeTag}${item.text || ''}`
      })
      .join('\n')
  }

  static stringifyASS(lyrics: LyricLine[]): string {
    const ass = `[Script Info]
Title: Lyrics
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
`
    return (
      ass +
      lyrics
        .map((item, index) => {
          const formatTime = (t: number): string => {
            const h = Math.floor(t / 3600)
            const m = Math.floor((t % 3600) / 60)
            const s = Math.floor(t % 60)
            const cs = Math.floor((t % 1) * 100)
            return `${h}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}.${cs.toString().padStart(2, '0')}`
          }
          // 结束时间取下一条的起始时间, 末行没有下一条则 +5 秒兜底
          const nextTime = index < lyrics.length - 1 ? lyrics[index + 1]!.time : item.time + 5
          return `Dialogue: 0,${formatTime(item.time)},${formatTime(nextTime)},Default,,0,0,0,,${item.text || ''}`
        })
        .join('\n')
    )
  }

  static stringifySRT(lyrics: LyricLine[]): string {
    return lyrics
      .map((item, index) => {
        const formatTime = (t: number): string => {
          const h = Math.floor(t / 3600)
          const m = Math.floor((t % 3600) / 60)
          const s = Math.floor(t % 60)
          const ms = Math.floor((t % 1) * 1000)
          return `${h.toString().padStart(2, '0')}:${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')},${ms.toString().padStart(3, '0')}`
        }
        // 结束时间规则同 stringifyASS
        const nextTime = index < lyrics.length - 1 ? lyrics[index + 1]!.time : item.time + 5
        return `${index + 1}\n${formatTime(item.time)} --> ${formatTime(nextTime)}\n${item.text || ''}\n`
      })
      .join('\n')
  }
}
