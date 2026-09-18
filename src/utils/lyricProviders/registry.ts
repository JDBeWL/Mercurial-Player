import type { LyricProviderDescriptor, LyricProviderMethod } from './types'
import type { LyricsProviderId, LyricKind } from '@/types'

/** 歌词来源注册表：新增来源在此登记描述符 + 在 Rust `lyrics::search_candidates` 中分发 */
export const LYRIC_PROVIDERS: LyricProviderDescriptor[] = [
  {
    id: 'netease',
    nameKey: 'providers.name.netease',
    defaultKind: 'auto',
    methods: [{ id: 'webapi', nameKey: 'providers.method.netease.webapi' }],
  },
  {
    id: 'lrclib',
    nameKey: 'providers.name.lrclib',
    defaultKind: 'auto',
    methods: [
      { id: 'get', nameKey: 'providers.method.lrclib.get' },
      { id: 'search', nameKey: 'providers.method.lrclib.search' },
    ],
  },
  {
    id: 'qq',
    nameKey: 'providers.name.qq',
    defaultKind: 'auto',
    methods: [{ id: 'web', nameKey: 'providers.method.qq.web' }],
  },
  {
    id: 'kugou',
    nameKey: 'providers.name.kugou',
    defaultKind: 'auto',
    methods: [{ id: 'web', nameKey: 'providers.method.kugou.web' }],
  },
]

/** 按 id 查来源描述符；未知 id 返回默认方法的外来源兜底 null */
export function getProviderDescriptor(id: LyricsProviderId): LyricProviderDescriptor | null {
  return LYRIC_PROVIDERS.find((p) => p.id === id) ?? null
}

/** 解析某来源实际使用的 method id（设置里没选时用默认/第一个） */
export function resolveMethod(providerId: LyricsProviderId, configuredMethod?: string): string {
  const descriptor = getProviderDescriptor(providerId)
  if (!descriptor || descriptor.methods.length === 0) return ''
  if (
    configuredMethod &&
    descriptor.methods.some((m: LyricProviderMethod) => m.id === configuredMethod)
  ) {
    return configuredMethod
  }
  return descriptor.methods[0]!.id
}

/** 解析某来源实际使用的文本类型（设置里没选/auto 时用来源默认） */
export function resolveKind(providerId: LyricsProviderId, configuredKind?: LyricKind): LyricKind {
  if (configuredKind && configuredKind !== 'auto') return configuredKind
  return getProviderDescriptor(providerId)?.defaultKind ?? 'auto'
}
