import { describe, it, expect } from 'vitest'
import { parseFontFileName } from '../../src/utils/bundledFonts'

describe('parseFontFileName', () => {
  it('无后缀解析为常规字重', () => {
    expect(parseFontFileName('思源黑体.ttf')).toEqual({
      family: '思源黑体',
      weight: 400,
      format: 'truetype',
    })
  })

  it('三位数字字重后缀', () => {
    expect(parseFontFileName('Poppins-300.ttf')).toEqual({
      family: 'Poppins',
      weight: 300,
      format: 'truetype',
    })
    expect(parseFontFileName('思源黑体-700.otf')).toMatchObject({ family: '思源黑体', weight: 700 })
  })

  it('-VF 后缀解析为全字重区间', () => {
    expect(parseFontFileName('Noto Sans SC-VF.ttf')).toEqual({
      family: 'Noto Sans SC',
      weight: '100 900',
      format: 'truetype',
    })
    expect(parseFontFileName('My Font-VF.woff2')).toMatchObject({
      family: 'My Font',
      weight: '100 900',
      format: 'woff2',
    })
  })

  it('兼容 Adobe 官方包命名（SourceHanSansSC-Heavy.otf 等）', () => {
    expect(parseFontFileName('SourceHanSansSC-Regular.otf')).toEqual({
      family: 'SourceHanSansSC',
      weight: 400,
      format: 'opentype',
    })
    expect(parseFontFileName('SourceHanSansSC-ExtraLight.otf')).toMatchObject({
      family: 'SourceHanSansSC',
      weight: 200,
    })
    expect(parseFontFileName('SourceHanSansSC-Light.otf')).toMatchObject({
      family: 'SourceHanSansSC',
      weight: 300,
    })
    expect(parseFontFileName('SourceHanSansSC-Medium.otf')).toMatchObject({
      family: 'SourceHanSansSC',
      weight: 500,
    })
    expect(parseFontFileName('SourceHanSansSC-Bold.otf')).toMatchObject({
      family: 'SourceHanSansSC',
      weight: 700,
    })
    expect(parseFontFileName('SourceHanSansSC-Heavy.otf')).toMatchObject({
      family: 'SourceHanSansSC',
      weight: 900,
    })
  })

  it('兼容 Google Fonts 命名（Poppins-SemiBold 等）', () => {
    expect(parseFontFileName('Poppins-Thin.ttf')).toMatchObject({ family: 'Poppins', weight: 100 })
    expect(parseFontFileName('Poppins-SemiBold.ttf')).toMatchObject({
      family: 'Poppins',
      weight: 600,
    })
    expect(parseFontFileName('Poppins-ExtraBold.ttf')).toMatchObject({
      family: 'Poppins',
      weight: 800,
    })
    expect(parseFontFileName('Poppins-Black.ttf')).toMatchObject({ family: 'Poppins', weight: 900 })
    // 同族多字重合并为一个族名
    const weights = ['Light', 'Regular', 'Bold'].map((w) =>
      parseFontFileName(`Poppins-${w}.ttf`).family,
    )
    expect(new Set(weights).size).toBe(1)
  })

  it('数字后缀优先于英文权重名', () => {
    expect(parseFontFileName('Font-Bold-700.ttf')).toMatchObject({
      family: 'Font-Bold',
      weight: 700,
    })
  })

  it('族名中的引号被剔除', () => {
    expect(parseFontFileName("My'Font.ttf").family).toBe('MyFont')
  })
})
