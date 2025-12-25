/**
 * 歌词解析器类，支持多种歌词格式
 */
import logger from './logger';

export class LyricsParser {
  /**
   * 解析歌词文件
   * @param {string} content - 歌词文件内容
   * @param {string} format - 歌词格式（lrc, ass, srt）
   * @returns {Array<{time: number, text: string}>} 解析后的歌词数组
   */
  static parse(content, format = 'auto') {
    if (!content || typeof content !== 'string') {
      return []
    }

    // 自动检测格式
    if (format === 'auto') {
      format = this.detectFormat(content)
    }

    switch (format.toLowerCase()) {
      case 'lrc':
        return this.parseLRC(content)
      case 'ass':
      case 'ssa':
        return this.parseASS(content)
      case 'srt':
        return this.parseSRT(content)
      default:
        logger.warn(`Unsupported lyrics format: ${format}`)
        return []
    }
  }

  /**
   * 自动检测歌词格式
   * @param {string} content - 歌词文件内容
   * @returns {string} 检测到的格式
   */
  static detectFormat(content) {
    // 检测 ASS/SSA 格式
    if (content.includes('[Script Info]') || content.includes('[V4+ Styles]') || content.includes('[Events]')) {
      return 'ass'
    }

    // 检测 SRT 格式
    if (/^\d+\s*\n\d{2}:\d{2}:\d{2},\d{3}\s*-->\s*\d{2}:\d{2}:\d{2},\d{3}\s*\n/m.test(content)) {
      return 'srt'
    }

    // 默认为 LRC 格式
    return 'lrc'
  }

  /**
   * 解析 LRC 格式歌词
   * @param {string} content - LRC 歌词内容
   * @returns {Array<{time: number, text: string}>} 解析后的歌词数组
   */
  static parseLRC(content) {
    const lines = content.split('\n')
    const lyrics = []
    const timeRegex = /^\[(\d{2}):(\d{2})(?:\.(\d{2,3}))?\](.*)$/
    
    for (const line of lines) {
      const trimmedLine = line.trim()
      if (!trimmedLine) continue
      
      // 处理多时间标签的行（如 [00:12.34][00:56.78]歌词内容）
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
        // 尝试单时间标签格式
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
    
    // 按时间排序
    lyrics.sort((a, b) => a.time - b.time)
    
    return lyrics
  }

  /**
   * 解析 ASS/SSA 格式歌词
   * @param {string} content - ASS/SSA 歌词内容
   * @returns {Array<{time: number, text: string}>} 解析后的歌词数组
   */
  static parseASS(content) {
    const lines = content.split('\n')
    const lyrics = []
    let inEvents = false
    let formatFields = []
    
    for (const line of lines) {
      const trimmedLine = line.trim()
      
      // 检测事件部分
      if (trimmedLine === '[Events]') {
        inEvents = true
        continue
      }
      
      // 检测其他部分，退出事件处理
      if (inEvents && trimmedLine.startsWith('[')) {
        inEvents = false
        continue
      }
      
      // 处理格式行
      if (inEvents && trimmedLine.startsWith('Format:')) {
        formatFields = trimmedLine.substring(7).split(',').map(field => field.trim())
        continue
      }
      
      // 处理对话行
      if (inEvents && trimmedLine.startsWith('Dialogue:')) {
        const parts = trimmedLine.substring(9).split(',')
        
        if (parts.length >= formatFields.length) {
          // 查找开始时间和文本字段
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
    
    // 按时间排序
    lyrics.sort((a, b) => a.time - b.time)
    
    return lyrics
  }

  /**
   * 解析 SRT 格式歌词
   * @param {string} content - SRT 歌词内容
   * @returns {Array<{time: number, text: string}>} 解析后的歌词数组
   */
  static parseSRT(content) {
    const blocks = content.trim().split(/\n\s*\n/)
    const lyrics = []
    const timeRegex = /(\d{1,2}):(\d{2}):(\d{2}),(\d{3})\s*-->\s*(\d{1,2}):(\d{2}):(\d{2}),(\d{3})/
    
    for (const block of blocks) {
      const lines = block.trim().split('\n')
      if (lines.length < 2) continue
      
      // 解析时间行
      const timeMatch = lines[1].match(timeRegex)
      if (!timeMatch) continue
      
      const startHours = parseInt(timeMatch[1])
      const startMinutes = parseInt(timeMatch[2])
      const startSeconds = parseInt(timeMatch[3])
      const startMilliseconds = parseInt(timeMatch[4])
      
      const startTime = startHours * 3600 + startMinutes * 60 + startSeconds + startMilliseconds / 1000
      
      // 获取文本内容（跳过序号和时间行）
      const text = lines.slice(2).join('\n').trim()
      
      if (text) {
        lyrics.push({ time: startTime, text })
      }
    }
    
    // 按时间排序
    lyrics.sort((a, b) => a.time - b.time)
    
    return lyrics
  }

  /**
   * 解析 ASS 时间格式
   * @param {string} timeStr - ASS 时间字符串 (H:MM:SS.cc)
   * @returns {number|null} 时间（秒）
   */
  static parseASSTime(timeStr) {
    const match = timeStr.match(/^(\d+):(\d{2}):(\d{2})\.(\d{2})$/)
    if (match) {
      const hours = parseInt(match[1])
      const minutes = parseInt(match[2])
      const seconds = parseInt(match[3])
      const centiseconds = parseInt(match[4])
      
      return hours * 3600 + minutes * 60 + seconds + centiseconds / 100
    }
    return null
  }

  /**
   * 将歌词数组转换为指定格式的字符串
   * @param {Array<{time: number, text: string}>} lyrics - 歌词数组
   * @param {string} format - 目标格式（lrc, ass, srt）
   * @returns {string} 格式化后的歌词字符串
   */
  static stringify(lyrics, format = 'lrc') {
    if (!lyrics || !Array.isArray(lyrics)) {
      return ''
    }

    switch (format.toLowerCase()) {
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

  /**
   * 将歌词数组转换为 LRC 格式字符串
   * @param {Array<{time: number, text: string}>} lyrics - 歌词数组
   * @returns {string} LRC 格式字符串
   */
  static stringifyLRC(lyrics) {
    return lyrics.map(item => {
      const minutes = Math.floor(item.time / 60)
      const seconds = Math.floor(item.time % 60)
      const milliseconds = Math.floor((item.time % 1) * 100)
      
      const timeTag = `[${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}.${milliseconds.toString().padStart(2, '0')}]`
      return `${timeTag}${item.text}`
    }).join('\n')
  }

  /**
   * 将歌词数组转换为 ASS 格式字符串
   * @param {Array<{time: number, text: string}>} lyrics - 歌词数组
   * @returns {string} ASS 格式字符串
   */
  static stringifyASS(lyrics) {
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
      const hours = Math.floor(item.time / 3600)
      const minutes = Math.floor((item.time % 3600) / 60)
      const seconds = Math.floor(item.time % 60)
      const centiseconds = Math.floor((item.time % 1) * 100)
      
      const startTime = `${hours}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}.${centiseconds.toString().padStart(2, '0')}`
      
      // 设置结束时间为下一句的开始时间或当前时间+5秒
      const nextTime = index < lyrics.length - 1 ? lyrics[index + 1].time : item.time + 5
      const endHours = Math.floor(nextTime / 3600)
      const endMinutes = Math.floor((nextTime % 3600) / 60)
      const endSeconds = Math.floor(nextTime % 60)
      const endCentiseconds = Math.floor((nextTime % 1) * 100)
      
      const endTime = `${endHours}:${endMinutes.toString().padStart(2, '0')}:${endSeconds.toString().padStart(2, '0')}.${endCentiseconds.toString().padStart(2, '0')}`
      
      return `Dialogue: 0,${startTime},${endTime},Default,,0,0,0,,${item.text}`
    }).join('\n')
  }

  /**
   * 将歌词数组转换为 SRT 格式字符串
   * @param {Array<{time: number, text: string}>} lyrics - 歌词数组
   * @returns {string} SRT 格式字符串
   */
  static stringifySRT(lyrics) {
    return lyrics.map((item, index) => {
      const hours = Math.floor(item.time / 3600)
      const minutes = Math.floor((item.time % 3600) / 60)
      const seconds = Math.floor(item.time % 60)
      const milliseconds = Math.floor((item.time % 1) * 1000)
      
      // 设置结束时间为下一句的开始时间或当前时间+5秒
      const nextTime = index < lyrics.length - 1 ? lyrics[index + 1].time : item.time + 5
      const endHours = Math.floor(nextTime / 3600)
      const endMinutes = Math.floor((nextTime % 3600) / 60)
      const endSeconds = Math.floor(nextTime % 60)
      const endMilliseconds = Math.floor((nextTime % 1) * 1000)
      
      const startTime = `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')},${milliseconds.toString().padStart(3, '0')}`
      const endTime = `${endHours.toString().padStart(2, '0')}:${endMinutes.toString().padStart(2, '0')}:${endSeconds.toString().padStart(2, '0')},${endMilliseconds.toString().padStart(3, '0')}`
      
      return `${index + 1}\n${startTime} --> ${endTime}\n${item.text}\n`
    }).join('\n')
  }
}

export default LyricsParser