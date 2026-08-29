import { describe, expect, it } from 'vitest'
import {
  createDefaultLyricsConfig,
  ensureLyricsConfigDefaults,
  migrateLyricsFieldsFromGeneral,
} from '@/utils/configDefaults'

describe('createDefaultLyricsConfig', () => {
  it('returns a complete default lyrics config', () => {
    const config = createDefaultLyricsConfig()
    expect(config.enableOnlineFetch).toBe(false)
    expect(config.autoSaveOnlineLyrics).toBe(true)
    expect(config.desktopLyrics?.colorPreset).toBe('auto')
    expect(config.desktopLyrics?.fontSize).toBe(28)
  })
})

describe('ensureLyricsConfigDefaults', () => {
  it('fills missing fields without overwriting existing ones', () => {
    // 模拟旧版本配置文件:只有部分字段存在
    const lyrics = ensureLyricsConfigDefaults({
      lyricsAlignment: 'left',
      lyricsFontFamily: 'Custom',
    } as unknown as Parameters<typeof ensureLyricsConfigDefaults>[0])

    expect(lyrics.lyricsAlignment).toBe('left')
    expect(lyrics.lyricsFontFamily).toBe('Custom')
    expect(lyrics.translationFontFamily).toBe('')
    expect(lyrics.lyricsStyle).toBe('modern')
    expect(lyrics.onlineSource).toBe('netease')
    expect(lyrics.showNoLyricsHint).toBe(true)
    expect(lyrics.showFetchLyricsButton).toBe(true)
    expect(lyrics.desktopLyrics?.enabled).toBe(false)
  })
})

describe('migrateLyricsFieldsFromGeneral', () => {
  it('returns false when general section is missing', () => {
    expect(migrateLyricsFieldsFromGeneral({})).toBe(false)
  })

  it('returns false when general has no lyrics fields', () => {
    expect(migrateLyricsFieldsFromGeneral({ general: { volume: 50 } as never })).toBe(false)
  })

  it('migrates lyrics fields from general to lyrics section', () => {
    const configData = {
      general: { lyricsAlignment: 'left', lyricsFontFamily: 'F', lyricsStyle: 'classic' },
    } as unknown as Parameters<typeof migrateLyricsFieldsFromGeneral>[0]

    expect(migrateLyricsFieldsFromGeneral(configData)).toBe(true)
    expect(configData.lyrics?.lyricsAlignment).toBe('left')
    expect(configData.lyrics?.lyricsFontFamily).toBe('F')
    expect(configData.lyrics?.lyricsStyle).toBe('classic')
    // 迁移后原字段应被删除
    const general = configData.general as unknown as Record<string, unknown>
    expect(general.lyricsAlignment).toBeUndefined()
    expect(general.lyricsFontFamily).toBeUndefined()
    expect(general.lyricsStyle).toBeUndefined()
  })

  it('applies defaults for blank migrated values', () => {
    // 只要 general 里存在任一非空歌词字段就触发迁移;空白字段留在 general 并给默认值
    const configData = {
      general: { lyricsAlignment: 'left', lyricsFontFamily: '' },
    } as unknown as Parameters<typeof migrateLyricsFieldsFromGeneral>[0]

    expect(migrateLyricsFieldsFromGeneral(configData)).toBe(true)
    expect(configData.lyrics?.lyricsAlignment).toBe('left')
    expect(configData.lyrics?.lyricsFontFamily).toBe('Noto Sans SC')
  })
})
