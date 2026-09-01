import { beforeEach, describe, expect, it, vi } from 'vitest'

const stores = vi.hoisted(() => ({
  config: null as { lyrics: Record<string, unknown> | undefined } | null,
}))

// 用 getter 读取:composable 会缓存 store 实例,直接取值会冻结首次快照
vi.mock('@/stores/config', async () => {
  const vue = await import('vue')
  stores.config ??= vue.reactive({ lyrics: undefined as Record<string, unknown> | undefined })
  return { useConfigStore: () => stores.config }
})

const { useLyricsTypography } = await import('@/composables/useLyricsTypography')

const config = () => stores.config as { lyrics: Record<string, unknown> | undefined }

beforeEach(() => {
  config().lyrics = undefined
})

describe('useLyricsTypography', () => {
  it('quotes the configured lyric font family', () => {
    config().lyrics = { lyricsFontFamily: 'LXGW WenKai Screen' }

    const { lyricFontStyle } = useLyricsTypography()

    expect(lyricFontStyle.value.fontFamily).toBe('"LXGW WenKai Screen"')
  })

  it('quotes numeric font names so CSSOM accepts them', () => {
    // 不带引号的 font-family 标识符不允许以数字开头,会被整体丢弃
    config().lyrics = { lyricsFontFamily: '975' }

    const { lyricFontStyle } = useLyricsTypography()

    expect(lyricFontStyle.value.fontFamily).toBe('"975"')
  })

  it('falls back to Noto Sans SC when no font is configured', () => {
    config().lyrics = {}

    const { lyricFontStyle } = useLyricsTypography()

    expect(lyricFontStyle.value.fontFamily).toBe('"Noto Sans SC"')
  })

  it('falls back when the lyrics config is missing entirely', () => {
    config().lyrics = undefined

    const { lyricFontStyle } = useLyricsTypography()

    expect(lyricFontStyle.value.fontFamily).toBe('"Noto Sans SC"')
  })

  it('treats an empty font name as unset', () => {
    config().lyrics = { lyricsFontFamily: '' }

    const { lyricFontStyle } = useLyricsTypography()

    expect(lyricFontStyle.value.fontFamily).toBe('"Noto Sans SC"')
  })

  it('omits the translation style when no translation font is set', () => {
    config().lyrics = { lyricsFontFamily: 'Roboto', translationFontFamily: '' }

    const { translationStyle } = useLyricsTypography()

    expect(translationStyle.value).toBeUndefined()
  })

  it('quotes the translation font when configured', () => {
    config().lyrics = { lyricsFontFamily: 'Roboto', translationFontFamily: 'Noto Serif SC' }

    const { translationStyle } = useLyricsTypography()

    expect(translationStyle.value).toEqual({ fontFamily: '"Noto Serif SC"' })
  })

  it('reacts to font changes', () => {
    config().lyrics = { lyricsFontFamily: 'Roboto' }
    const { lyricFontStyle, translationStyle } = useLyricsTypography()
    expect(lyricFontStyle.value.fontFamily).toBe('"Roboto"')
    expect(translationStyle.value).toBeUndefined()

    config().lyrics = { lyricsFontFamily: 'Inter', translationFontFamily: 'Serif' }

    expect(lyricFontStyle.value.fontFamily).toBe('"Inter"')
    expect(translationStyle.value).toEqual({ fontFamily: '"Serif"' })
  })

  it('keeps the reactive store shared across instances', () => {
    config().lyrics = { lyricsFontFamily: 'Shared' }

    const first = useLyricsTypography()
    const second = useLyricsTypography()

    expect(second.lyricFontStyle.value.fontFamily).toBe(first.lyricFontStyle.value.fontFamily)
  })
})
