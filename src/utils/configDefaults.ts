/**
 * 配置默认值与旧版本配置迁移
 *
 * 从 config store 抽离的纯逻辑:store 只保留状态与读写流程,
 * 默认值兜底、字段迁移在此集中维护。
 */
import type { AppConfig, LyricsConfig } from '@/types'

/** 歌词配置默认值(desktopLyrics 含全部字段) */
export function createDefaultLyricsConfig(): LyricsConfig {
  return {
    enableOnlineFetch: false,
    autoSaveOnlineLyrics: true,
    preferTranslation: true,
    onlineSource: 'netease',
    lyricsAlignment: 'center',
    lyricsFontFamily: 'Noto Sans SC',
    translationFontFamily: '',
    lyricsStyle: 'modern',
    showNoLyricsHint: true,
    showFetchLyricsButton: true,
    desktopLyrics: {
      enabled: false,
      locked: true,
      fontSize: 28,
      colorPreset: 'auto' as const,
    },
  }
}

/**
 * 确保歌词配置包含所有必需字段(旧版本配置文件兼容),原样返回
 */
export function ensureLyricsConfigDefaults(lyrics: LyricsConfig): LyricsConfig {
  if (!lyrics.lyricsAlignment) lyrics.lyricsAlignment = 'center'
  if (!lyrics.lyricsFontFamily) lyrics.lyricsFontFamily = 'Noto Sans SC'
  if (lyrics.translationFontFamily === undefined) lyrics.translationFontFamily = ''
  if (!lyrics.lyricsStyle) lyrics.lyricsStyle = 'modern'
  if (lyrics.onlineSource === undefined) lyrics.onlineSource = 'netease'
  if (lyrics.showNoLyricsHint === undefined) lyrics.showNoLyricsHint = true
  if (lyrics.showFetchLyricsButton === undefined) lyrics.showFetchLyricsButton = true
  if (!lyrics.desktopLyrics) lyrics.desktopLyrics = createDefaultLyricsConfig().desktopLyrics
  return lyrics
}

/**
 * 把旧版本存放在 general 分区下的歌词字段迁移到 lyrics 分区。
 * 返回是否发生了迁移(调用方据此标记配置为脏)。
 */
export function migrateLyricsFieldsFromGeneral(configData: Partial<AppConfig>): boolean {
  if (!configData.general) return false

  const general = configData.general as AppConfig['general'] & {
    lyricsAlignment?: string
    lyricsFontFamily?: string
    lyricsStyle?: string
  }
  if (!(general.lyricsAlignment || general.lyricsFontFamily || general.lyricsStyle)) {
    return false
  }

  if (!configData.lyrics) {
    configData.lyrics = createDefaultLyricsConfig()
  }
  if (general.lyricsAlignment) {
    configData.lyrics.lyricsAlignment = general.lyricsAlignment as LyricsConfig['lyricsAlignment']
    delete general.lyricsAlignment
  }
  if (general.lyricsFontFamily) {
    configData.lyrics.lyricsFontFamily = general.lyricsFontFamily
    delete general.lyricsFontFamily
  }
  if (general.lyricsStyle) {
    configData.lyrics.lyricsStyle = general.lyricsStyle
    delete general.lyricsStyle
  }
  if (!configData.lyrics.lyricsAlignment) configData.lyrics.lyricsAlignment = 'center'
  if (!configData.lyrics.lyricsFontFamily) configData.lyrics.lyricsFontFamily = 'Noto Sans SC'
  if (!configData.lyrics.lyricsStyle) configData.lyrics.lyricsStyle = 'modern'

  return true
}
