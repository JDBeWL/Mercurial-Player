/** 配置默认值兜底与旧版本字段迁移：从 config store 抽离的纯逻辑，store 只保留状态与读写 */
import type { AppConfig, LyricsConfig } from '@/types'

/** 歌词配置默认值；desktopLyrics 含全部字段，迁移兜底也复用它 */
export function createDefaultLyricsConfig(): LyricsConfig {
  return {
    enableOnlineFetch: false,
    autoSaveOnlineLyrics: true,
    preferTranslation: true,
    onlineSource: 'netease',
    lyricProviderOrder: ['netease'],
    lyricProviderSettings: {},
    lyricsAlignment: 'center',
    lyricsFontFamily: 'Noto Sans SC',
    translationFontFamily: '',
    lyricsStyle: 'modern',
    /* 字号倍率：1 = 样式表原始大小，供用户在系统放大字体时自行调整 */
    fontScale: 1,
    showNoLyricsHint: true,
    showFetchLyricsButton: true,
    autoSelectBestLyrics: true,
    desktopLyrics: {
      enabled: false,
      locked: true,
      fontSize: 28,
      colorPreset: 'auto' as const,
    },
  }
}

/** 补齐缺失字段的旧版本兼容默认值，原地修改并返回同一对象 */
export function ensureLyricsConfigDefaults(lyrics: LyricsConfig): LyricsConfig {
  if (!lyrics.lyricsAlignment) lyrics.lyricsAlignment = 'center'
  if (!lyrics.lyricsFontFamily) lyrics.lyricsFontFamily = 'Noto Sans SC'
  if (lyrics.translationFontFamily === undefined) lyrics.translationFontFamily = ''
  if (!lyrics.lyricsStyle) lyrics.lyricsStyle = 'modern'
  if (lyrics.fontScale === undefined) lyrics.fontScale = 1
  if (lyrics.onlineSource === undefined) lyrics.onlineSource = 'netease'
  if (lyrics.lyricProviderOrder === undefined) {
    lyrics.lyricProviderOrder = [lyrics.onlineSource || 'netease']
  }
  if (lyrics.lyricProviderSettings === undefined) lyrics.lyricProviderSettings = {}
  if (lyrics.showNoLyricsHint === undefined) lyrics.showNoLyricsHint = true
  if (lyrics.showFetchLyricsButton === undefined) lyrics.showFetchLyricsButton = true
  if (lyrics.autoSelectBestLyrics === undefined) lyrics.autoSelectBestLyrics = true
  if (!lyrics.desktopLyrics) lyrics.desktopLyrics = createDefaultLyricsConfig().desktopLyrics
  return lyrics
}

/** 把旧版 general 分区的歌词字段迁移到 lyrics 分区；返回是否迁移，调用方据此标记配置为脏 */
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
