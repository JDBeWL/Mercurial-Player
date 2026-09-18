import type { LyricKind, LyricsProviderId } from '@/types'

// 让来源包同时向外重导出这两个共享类型
export type { LyricKind, LyricsProviderId }

/** 与后端 `LyricsData` 对应的原始歌词 bundle */
export interface LyricBundle {
  lrc: string
  tlyric?: string
  romalrc?: string
  /** ASS 逐字歌词（原文逐字 + 译文/罗马音普通行），无逐字时为空 */
  karaoke?: string
}

/** 单条候选歌词（内嵌 bundle 供预览），附加来源与所采用算法 */
export interface LyricCandidate {
  id: string
  title: string
  artist: string
  album: string
  duration_ms: number
  bundle: LyricBundle
  provider: LyricsProviderId
  method: string
}

/** 歌词查询参数 */
export interface LyricQuery {
  title: string
  artist: string
  duration_ms: number
}

/** 平台内的一种获取算法 */
export interface LyricProviderMethod {
  id: string
  /** i18n key，如 `providers.method.lrclib.get` */
  nameKey: string
}

/** 歌词来源描述符 */
export interface LyricProviderDescriptor {
  id: LyricsProviderId
  /** i18n key，如 `providers.name.netease` */
  nameKey: string
  methods: LyricProviderMethod[]
  /** 该来源默认文本类型（auto = 跟随全局 preferTranslation） */
  defaultKind: LyricKind
}
