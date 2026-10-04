import { computed, type CSSProperties } from 'vue'
import { useConfigStore } from '@/stores/config'

/**
 * 歌词排版 Composable
 *
 * LyricsDisplay 与 VisualizerPanel 共用:原文行字体与译文字体都取自 configStore 的歌词配置
 */
export function useLyricsTypography() {
  const configStore = useConfigStore()

  // font-family 值必须加引号:未加引号的标识符不允许以数字开头 (如按文件名解析出的 "975"),
  // 这类值赋给 CSSOM 会被整体丢弃
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
