import { describe, it, expect } from 'vitest'
import { LyricsParser } from '@/utils/lyricsParser'

describe('LyricsParser', () => {
  describe('detectFormat', () => {
    it('should detect LRC format', () => {
      const content = '[00:01.00]Hello world'
      expect(LyricsParser.detectFormat(content)).toBe('lrc')
    })

    it('should detect ASS format with Script Info', () => {
      const content = '[Script Info]\nTitle: Test'
      expect(LyricsParser.detectFormat(content)).toBe('ass')
    })

    it('should detect ASS format with V4+ Styles', () => {
      const content = '[V4+ Styles]\nFormat: Name'
      expect(LyricsParser.detectFormat(content)).toBe('ass')
    })

    it('should detect ASS format with Events', () => {
      const content = '[Events]\nDialogue: 0,0:00:01.00'
      expect(LyricsParser.detectFormat(content)).toBe('ass')
    })

    it('should detect SRT format', () => {
      const content = '1\n00:00:01,000 --> 00:00:02,000\nHello'
      expect(LyricsParser.detectFormat(content)).toBe('srt')
    })

    it('should default to LRC for unknown format', () => {
      const content = 'Just some text'
      expect(LyricsParser.detectFormat(content)).toBe('lrc')
    })
  })

  describe('parseLRC', () => {
    it('should parse simple LRC', () => {
      const content = '[00:01.00]First line\n[00:05.50]Second line'
      const result = LyricsParser.parseLRC(content)
      
      expect(result).toHaveLength(2)
      expect(result[0].time).toBeCloseTo(1.0, 1)
      expect(result[0].text).toBe('First line')
      expect(result[1].time).toBeCloseTo(5.5, 1)
      expect(result[1].text).toBe('Second line')
    })

    it('should handle multiple timestamps per line', () => {
      const content = '[00:01.00][00:10.00]Repeated line'
      const result = LyricsParser.parseLRC(content)
      
      expect(result).toHaveLength(2)
      expect(result[0].text).toBe('Repeated line')
      expect(result[1].text).toBe('Repeated line')
    })

    it('should skip empty lines', () => {
      const content = '[00:01.00]\n[00:05.00]Valid line'
      const result = LyricsParser.parseLRC(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].text).toBe('Valid line')
    })

    it('should return empty array for empty content', () => {
      expect(LyricsParser.parseLRC('')).toEqual([])
    })

    it('should handle 3-digit milliseconds', () => {
      const content = '[00:01.500]Line with 3 digit ms'
      const result = LyricsParser.parseLRC(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].time).toBeCloseTo(1.5, 2)
    })

    it('should sort lyrics by time', () => {
      const content = '[00:10.00]Second\n[00:01.00]First'
      const result = LyricsParser.parseLRC(content)
      
      expect(result[0].text).toBe('First')
      expect(result[1].text).toBe('Second')
    })
  })

  describe('parseSRT', () => {
    it('should parse SRT format', () => {
      const content = `1
00:00:01,000 --> 00:00:04,000
First subtitle

2
00:00:05,000 --> 00:00:08,000
Second subtitle`
      
      const result = LyricsParser.parseSRT(content)
      
      expect(result).toHaveLength(2)
      expect(result[0].time).toBe(1)
      expect(result[0].text).toBe('First subtitle')
      expect(result[1].time).toBe(5)
    })
  })

  describe('parseASSTime', () => {
    it('should parse ASS time format', () => {
      expect(LyricsParser.parseASSTime('0:00:01.50')).toBeCloseTo(1.5, 2)
      expect(LyricsParser.parseASSTime('1:30:00.00')).toBe(5400)
    })

    it('should return null for invalid format', () => {
      expect(LyricsParser.parseASSTime('invalid')).toBeNull()
    })
  })

  describe('stringify', () => {
    const lyrics = [
      { time: 1, text: 'First', texts: [] },
      { time: 5.5, text: 'Second', texts: [] },
    ]

    it('should stringify to LRC', () => {
      const result = LyricsParser.stringifyLRC(lyrics)
      expect(result).toContain('[00:01.00]First')
      expect(result).toContain('[00:05.50]Second')
    })

    it('should stringify to SRT', () => {
      const result = LyricsParser.stringifySRT(lyrics)
      expect(result).toContain('00:00:01,000')
      expect(result).toContain('First')
    })

    it('should return empty string for invalid input', () => {
      expect(LyricsParser.stringify(null as any)).toBe('')
      expect(LyricsParser.stringify(undefined as any)).toBe('')
    })
  })

  describe('parse with auto detection', () => {
    it('should auto-detect and parse LRC', () => {
      const content = '[00:01.00]Auto detected'
      const result = LyricsParser.parse(content, 'auto')
      
      expect(result).toHaveLength(1)
      expect(result[0].text).toBe('Auto detected')
    })

    it('should return empty array for empty content', () => {
      expect(LyricsParser.parse('')).toEqual([])
      expect(LyricsParser.parse(null as any)).toEqual([])
    })

    it('should warn for unsupported format', () => {
      const result = LyricsParser.parse('test', 'unknown' as any)
      expect(result).toEqual([])
    })
  })

  describe('parseLRCAsync', () => {
    it('should parse LRC asynchronously', async () => {
      const content = '[00:01.00]First\n[00:05.00]Second'
      const result = await LyricsParser.parseLRCAsync(content)
      
      expect(result).toHaveLength(2)
      expect(result[0].texts).toContain('First')
    })

    it('should handle karaoke timestamps', async () => {
      const content = '[00:01.00][00:01.50][00:02.00]Word by word'
      const result = await LyricsParser.parseLRCAsync(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].karaoke).not.toBeNull()
    })

    it('should handle mm:ss:cs format', async () => {
      const content = '[01:30:00]Track lyric'
      const result = await LyricsParser.parseLRCAsync(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].time).toBe(90) // 1m 30s = 90s
    })

    it('should skip lines without timestamps', async () => {
      const content = 'No timestamp here\n[00:01.00]Valid line'
      const result = await LyricsParser.parseLRCAsync(content)
      
      expect(result).toHaveLength(1)
    })

    it('should skip lines with only timestamps', async () => {
      const content = '[00:01.00]\n[00:05.00]Valid'
      const result = await LyricsParser.parseLRCAsync(content)
      
      expect(result).toHaveLength(1)
    })
  })

  describe('parseASSAsync', () => {
    it('should parse ASS dialogue lines', async () => {
      const content = `[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:05.00,orig,,0,0,0,,Hello world`
      
      const result = await LyricsParser.parseASSAsync(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].time).toBeCloseTo(1, 1)
    })

    it('should parse karaoke tags in ASS', async () => {
      const content = `[Events]
Dialogue: 0,0:00:01.00,0:00:05.00,orig,,0,0,0,,{\\k50}Hel{\\k50}lo`
      
      const result = await LyricsParser.parseASSAsync(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].words).toBeDefined()
      expect(result[0].words!.length).toBeGreaterThan(0)
    })

    it('should handle translation style', async () => {
      const content = `[Events]
Dialogue: 0,0:00:01.00,0:00:05.00,orig,,0,0,0,,English
Dialogue: 0,0:00:01.00,0:00:05.00,ts,,0,0,0,,中文`
      
      const result = await LyricsParser.parseASSAsync(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].texts).toHaveLength(2)
    })

    it('should skip non-dialogue lines', async () => {
      const content = `[Script Info]
Title: Test
[Events]
Dialogue: 0,0:00:01.00,0:00:05.00,orig,,0,0,0,,Valid`
      
      const result = await LyricsParser.parseASSAsync(content)
      
      expect(result).toHaveLength(1)
    })

    it('should skip malformed dialogue lines', async () => {
      const content = `Dialogue: 0,0:00:01.00`
      const result = await LyricsParser.parseASSAsync(content)
      
      expect(result).toHaveLength(0)
    })
  })

  describe('parseAsync', () => {
    it('should auto-detect and parse LRC async', async () => {
      const content = '[00:01.00]Test'
      const result = await LyricsParser.parseAsync(content, 'auto')
      
      expect(result).toHaveLength(1)
    })

    it('should auto-detect and parse ASS async', async () => {
      const content = `[Script Info]
[Events]
Dialogue: 0,0:00:01.00,0:00:05.00,orig,,0,0,0,,Test`
      
      const result = await LyricsParser.parseAsync(content, 'auto')
      
      expect(result).toHaveLength(1)
    })

    it('should fall back to sync parse for SRT', async () => {
      const content = `1
00:00:01,000 --> 00:00:05,000
Test`
      
      const result = await LyricsParser.parseAsync(content, 'srt')
      
      expect(result).toHaveLength(1)
    })

    it('should return empty for invalid input', async () => {
      expect(await LyricsParser.parseAsync('')).toEqual([])
      expect(await LyricsParser.parseAsync(null as any)).toEqual([])
    })
  })

  describe('parseASS (sync)', () => {
    it('should parse ASS with Format line', () => {
      const content = `[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.50,0:00:05.00,Default,,0,0,0,,Hello`
      
      const result = LyricsParser.parseASS(content)
      
      expect(result).toHaveLength(1)
      expect(result[0].time).toBeCloseTo(1.5, 2)
      expect(result[0].text).toBe('Hello')
    })

    it('should handle section transitions', () => {
      const content = `[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,First
[Another Section]
Dialogue: 0,0:00:10.00,0:00:15.00,Default,,0,0,0,,Should be ignored`
      
      const result = LyricsParser.parseASS(content)
      
      expect(result).toHaveLength(1)
    })

    it('should strip ASS tags from text', () => {
      const content = `[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,{\\k50}Clean{\\k50}Text`
      
      const result = LyricsParser.parseASS(content)
      
      expect(result[0].text).toBe('CleanText')
    })

    it('should handle text with commas', () => {
      const content = `[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,Hello, World, Test`
      
      const result = LyricsParser.parseASS(content)
      
      expect(result[0].text).toBe('Hello, World, Test')
    })
  })

  describe('stringifyASS', () => {
    it('should generate valid ASS format', () => {
      const lyrics = [{ time: 1, text: 'Hello', texts: [] }]
      const result = LyricsParser.stringifyASS(lyrics)
      
      expect(result).toContain('[Script Info]')
      expect(result).toContain('[V4+ Styles]')
      expect(result).toContain('[Events]')
      expect(result).toContain('Dialogue:')
      expect(result).toContain('Hello')
    })

    it('should format time correctly', () => {
      const lyrics = [{ time: 3661.5, text: 'Test', texts: [] }] // 1:01:01.50
      const result = LyricsParser.stringifyASS(lyrics)
      
      expect(result).toContain('1:01:01.50')
    })
  })
})
