import { describe, it, expect } from 'vitest'
import { buildFinalLyric, buildPreviewLyric, mergeLyricTexts } from '@/services/lyrics/builder'

describe('mergeLyricTexts', () => {
  it('无额外文本时返回原文', () => {
    expect(mergeLyricTexts('[00:01.00]Hello', '')).toBe('[00:01.00]Hello')
  })

  it('原文为空时返回空', () => {
    expect(mergeLyricTexts('', '[00:01.00]你好')).toBe('')
  })

  it('按时间轴合并', () => {
    const lrc = '[00:01.00]Hello\n[00:02.00]World'
    const trans = '[00:01.00]你好\n[00:02.00]世界'
    const result = mergeLyricTexts(lrc, trans)
    expect(result).toContain('[00:01.00]Hello')
    expect(result).toContain('[00:01.00]你好')
    expect(result).toContain('[00:02.00]World')
    expect(result).toContain('[00:02.00]世界')
  })

  it('同一时间原文与翻译相同时去重翻译行', () => {
    // trans 在 [00:01.00] 处与原文相同，应只保留原文一行
    const result = mergeLyricTexts('[00:01.00]Hello', '[00:01.00]Hello\n[00:02.00]你好')
    const helloCount = (result.match(/\[00:01\.00\]Hello/g) ?? []).length
    expect(helloCount).toBe(1)
    // 仅有翻译、无原文的时间点不输出（与原逻辑一致）
    expect(result).not.toContain('[00:02.00]')
  })
})

describe('逐字 ASS 按 kind 裁剪样式', () => {
  const ass = [
    '[Script Info]',
    'ScriptType: v4.00+',
    '',
    '[Events]',
    'Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text',
    'Dialogue: 0,00:00:01.00,00:00:02.00,orig,,0,0,0,,{\\kf50}原',
    'Dialogue: 0,00:00:01.00,00:00:02.00,ts,,0,0,0,,翻译',
    'Dialogue: 0,00:00:01.00,00:00:02.00,roma,,0,0,0,,Romaji',
  ].join('\n')
  const bundle = { lrc: '[00:01.00]原', karaoke: ass }

  it('original 只留 orig 行', () => {
    const r = buildFinalLyric(bundle, 'original', true)
    expect(r.format).toBe('ass')
    expect(r.content).toContain(',orig,,0,0,0,,')
    expect(r.content).not.toContain(',ts,,0,0,0,,')
    expect(r.content).not.toContain(',roma,,0,0,0,,')
  })

  it('translation 留原文+译文，roman 留原文+罗马音', () => {
    const trans = buildFinalLyric(bundle, 'translation', false).content
    expect(trans).toContain(',ts,,0,0,0,,')
    expect(trans).not.toContain(',roma,,0,0,0,,')
    const roman = buildFinalLyric(bundle, 'roman', true).content
    expect(roman).toContain(',roma,,0,0,0,,')
    expect(roman).not.toContain(',ts,,0,0,0,,')
  })

  it('auto 跟随全局优先译文', () => {
    expect(buildFinalLyric(bundle, 'auto', true).content).toContain(',ts,,0,0,0,,')
    expect(buildFinalLyric(bundle, 'auto', false).content).not.toContain(',ts,,0,0,0,,')
  })

  it('头部与 Format 行原样保留', () => {
    const r = buildFinalLyric(bundle, 'original', false).content
    expect(r).toContain('ScriptType: v4.00+')
    expect(r).toContain('Format: Layer, Start, End, Style')
  })
})

describe('buildFinalLyric', () => {
  const bundle = {
    lrc: '[00:01.00]Original',
    tlyric: '[00:01.00]翻译',
    romalrc: '[00:01.00]Romaji',
  }

  it('original 仅原文 LRC', () => {
    const r = buildFinalLyric(bundle, 'original', true)
    expect(r.format).toBe('lrc')
    expect(r.content).toBe('[00:01.00]Original')
  })

  it('translation 合并翻译', () => {
    expect(buildFinalLyric(bundle, 'translation', false).content).toContain('翻译')
  })

  it('roman 合并罗马音', () => {
    expect(buildFinalLyric(bundle, 'roman', false).content).toContain('Romaji')
  })

  it('auto 跟随全局 preferTranslation', () => {
    expect(buildFinalLyric(bundle, 'auto', true).content).toContain('翻译')
    expect(buildFinalLyric(bundle, 'auto', false).content).toBe('[00:01.00]Original')
  })

  it('无原文返回空', () => {
    const r = buildFinalLyric({ lrc: '' }, 'auto', true)
    expect(r.content).toBe('')
    expect(r.format).toBe('lrc')
  })

  it('有逐字 karaoke 时直接使用 ASS', () => {
    const ass =
      '[Script Info]\nTitle: x\n[Events]\nDialogue: 0,00:00:03.24,00:00:06.69,orig,,0,0,0,,{\\kf14}词'
    const r = buildFinalLyric({ ...bundle, karaoke: ass }, 'original', false)
    expect(r.format).toBe('ass')
    expect(r.content).toBe(ass)
  })
})

describe('buildPreviewLyric', () => {
  const bundle = {
    lrc: '[00:01.00]Original',
    tlyric: '[00:01.00]翻译',
    romalrc: '[00:01.00]Romaji',
  }
  const ass = '[Script Info]\n[Events]\nDialogue: 0,00:00:01.00,00:00:02.00,orig,,0,0,0,,Original'

  it('逐字 bundle 也返回 LRC 视图而不是 ASS', () => {
    const text = buildPreviewLyric({ ...bundle, karaoke: ass }, 'auto', true)
    expect(text).not.toContain('Dialogue')
    expect(text).toContain('Original')
    // auto + 全局开启翻译 -> 原文与翻译都在
    expect(text).toContain('翻译')
  })

  it('kind 切换在逐字 bundle 上依然生效（这是预览的用途）', () => {
    const withKaraoke = { ...bundle, karaoke: ass }
    expect(buildPreviewLyric(withKaraoke, 'original', true)).toBe('[00:01.00]Original')
    expect(buildPreviewLyric(withKaraoke, 'translation', false)).toContain('翻译')
    expect(buildPreviewLyric(withKaraoke, 'roman', false)).toContain('Romaji')
  })

  it('无原文返回空串', () => {
    expect(buildPreviewLyric({ lrc: '', karaoke: ass }, 'auto', true)).toBe('')
  })
})
