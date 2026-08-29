import { computed, type CSSProperties } from 'vue'
import { useConfigStore } from '@/stores/config'

/**
 * 歌词排版 Composable
 *
 * 主歌词页（LyricsDisplay）与可视化面板（VisualizerPanel）共用的
 * 歌词字体样式：原文行字体与译文字体均来自 configStore 的歌词配置。
 */
export function useLyricsTypography() {
  const configStore = useConfigStore()

  // 原文行字体样式。
  // 字体名必须加引号：不带引号的 font-family 标识符不允许以数字开头
  // （如按文件名解析出的 "975"），赋给 CSSOM 会被整体丢弃
  const lyricFontStyle = computed<CSSProperties>(() => ({
    fontFamily: `"${configStore.lyrics?.lyricsFontFamily || 'Noto Sans SC'}"`,
  }))

  // 译文字体：为空时跟随原文（不设置该样式，继承行容器的字体）
  const translationStyle = computed<CSSProperties | undefined>(() => {
    const family = configStore.lyrics?.translationFontFamily
    if (!family) {
      return undefined
    }
    return { fontFamily: `"${family}"` }
  })

  return {
    lyricFontStyle,
    translationStyle,
  }
}
