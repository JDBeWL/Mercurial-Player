import { ref, watch, type WatchStopHandle } from 'vue'
import { useConfigStore } from '@/stores/config'
import FileUtils from '@/utils/fileUtils'
import { TitleExtractor } from '@/utils/titleExtractor'
import logger from '@/utils/logger'
import type { Track } from '@/types'

interface ProcessedTrackInfo {
  processing: boolean
  title?: string
  artist?: string
  album?: string
  fileName?: string
  isFromMetadata?: boolean
}

/**
 * 缓存条数上限,超出按 LRU 驱逐
 *
 * 取值须容得下一次目录扫描的曲目数,过小会让缓存刚填满就被清空
 * 导出供单测读取,避免测试里再硬编码一份上限
 */
export const MAX_PROCESSED_TRACKS = 5000

/**
 * LRU 访问顺序表,依赖 Map 的插入顺序特性
 *
 * delete + set 即 O(1) 地把 key 挪到"最近使用"的末尾,头部就是最久未使用的
 */
const accessOrder = new Map<string, void>()

/** 将 key 移到 accessOrder 末尾 (最近使用),O(1) */
function touchKey(key: string): void {
  accessOrder.delete(key)
  accessOrder.set(key, undefined)
}

/** 当缓存达到上限时,驱逐最久未使用的 key,O(1) */
function evictIfNeeded(cache: Map<string, ProcessedTrackInfo>): void {
  while (accessOrder.size >= MAX_PROCESSED_TRACKS) {
    const oldestKey = accessOrder.keys().next().value
    if (oldestKey !== undefined) {
      accessOrder.delete(oldestKey)
      cache.delete(oldestKey)
    } else {
      break
    }
  }
}

// 模块级共享缓存,所有实例共用一份 (原因见 useTrackInfo 的 JSDoc)
// 用 Map 而非 Record:频繁 delete 会让 V8 hidden class 降级 (slow properties),增删查改始终均摊 O(1)
// reactive proxy 原生追踪 Map 的增删,processTrackInfo 完成后模板会自动更新
const sharedProcessedTracks = ref<Map<string, ProcessedTrackInfo>>(new Map())

// 模块级 store 引用,首次调用 useTrackInfo 时赋值
let _configStore: ReturnType<typeof useConfigStore> | null = null

function ensureConfigStore(): ReturnType<typeof useConfigStore> {
  if (!_configStore) {
    _configStore = useConfigStore()
  }
  return _configStore
}

/** 根据配置获取用于回退显示的文件名 */
function getFallbackDisplayName(trackPath: string): string {
  const hideExt = ensureConfigStore().titleExtraction?.hideFileExtension ?? true
  return hideExt
    ? FileUtils.getFileNameWithoutExtension(trackPath)
    : FileUtils.getFileName(trackPath)
}

/** 当 hideFileExtension=true 时,去除标题末尾与文件扩展名匹配的后缀 */
function stripTitleExt(trackPath: string, title: string): string {
  const hideExt = ensureConfigStore().titleExtraction?.hideFileExtension ?? true
  if (!hideExt || !title) return title
  const ext = FileUtils.getFileExtension(trackPath)
  if (!ext) return title
  const suffix = `.${ext}`
  return title.toLowerCase().endsWith(suffix) ? title.slice(0, -suffix.length) : title
}

/** 从缓存读取并更新访问顺序 (LRU) */
function getCached(trackPath: string): ProcessedTrackInfo | undefined {
  const value = sharedProcessedTracks.value.get(trackPath)
  if (value) touchKey(trackPath)
  return value
}

/** 写入缓存并更新访问顺序 (LRU) */
function setCached(trackPath: string, value: ProcessedTrackInfo): void {
  if (!sharedProcessedTracks.value.has(trackPath)) {
    evictIfNeeded(sharedProcessedTracks.value)
  }
  sharedProcessedTracks.value.set(trackPath, value)
  touchKey(trackPath)
}

/** 删除缓存项 */
function deleteCached(trackPath: string): void {
  accessOrder.delete(trackPath)
  sharedProcessedTracks.value.delete(trackPath)
}

/** 异步提取单条音轨的标题信息,结果写入共享缓存 */
async function processTrackInfo(trackPath: string): Promise<void> {
  try {
    if (getCached(trackPath)?.processing) return

    setCached(trackPath, { processing: true })

    const configStore = ensureConfigStore()
    const config = {
      preferMetadata: configStore.titleExtraction?.preferMetadata ?? true,
      hideFileExtension: configStore.titleExtraction?.hideFileExtension ?? true,
      parseArtistTitle: configStore.titleExtraction?.parseArtistTitle ?? true,
      separator: configStore.titleExtraction?.separator ?? '-',
      customSeparators: configStore.titleExtraction?.customSeparators ?? ['-', '_', '.'],
    }

    const titleInfo = await TitleExtractor.extractTitle(trackPath, config)

    setCached(trackPath, {
      processing: false,
      ...titleInfo,
    })
  } catch (error) {
    logger.error('处理音轨信息失败:', trackPath, error)
    setCached(trackPath, {
      processing: false,
      title: getFallbackDisplayName(trackPath),
      artist: '',
      fileName: FileUtils.getFileName(trackPath),
      isFromMetadata: false,
    })
  }
}

/**
 * 音轨信息处理 composable
 *
 * 缓存是模块级共享的 (sharedProcessedTracks),App.vue 与 MiniPlayer.vue 因此只算一次,
 * 并有 LRU 上限兜住长期增长
 */
export function useTrackInfo() {
  ensureConfigStore()

  /** 获取音轨标题 */
  const getTrackTitle = (track: Track | null | undefined, fallback: string = ''): string => {
    if (!track || !track.path) {
      return fallback
    }

    const trackPath = track.path

    const cached = getCached(trackPath)
    if (cached && !cached.processing) {
      return (cached.title && stripTitleExt(trackPath, cached.title)) || fallback
    }

    // 异步补全,不阻塞本次渲染
    if (!cached || !cached.processing) {
      void processTrackInfo(trackPath)
    }

    // 缓存未就绪时优先用 store 已填的 title,避免露出原始文件名
    return (
      (track.title && stripTitleExt(trackPath, track.title)) || getFallbackDisplayName(trackPath)
    )
  }

  /** 获取音轨艺术家 */
  const getTrackArtist = (track: Track | null | undefined, fallback: string = ''): string => {
    if (!track || !track.path) {
      return fallback
    }

    const trackPath = track.path

    const cached = getCached(trackPath)
    if (cached && !cached.processing) {
      return cached.artist || fallback
    }

    // 同 getTrackTitle:异步补全,不阻塞渲染
    if (!cached || !cached.processing) {
      void processTrackInfo(trackPath)
    }

    return track.artist || fallback
  }

  /**
   * 设置音轨变化监听器
   *
   * 切歌时先用 track.title/artist 预填缓存,再触发异步精细提取,避免首帧显示原始文件名而抖动
   */
  const watchTrack = (trackGetter: () => Track | null | undefined): WatchStopHandle => {
    return watch(
      trackGetter,
      (newTrack) => {
        if (newTrack && newTrack.path) {
          const path = newTrack.path
          // 预填值让 getTrackTitle / getTrackArtist 在异步完成前就返回有意义的结果
          const existing = getCached(path)
          if (!existing || existing.processing) {
            const preTitle = newTrack.title || getFallbackDisplayName(path)
            const preArtist = newTrack.artist || ''
            setCached(path, {
              processing: true,
              title: preTitle,
              artist: preArtist,
              fileName: FileUtils.getFileName(path),
              isFromMetadata: false,
            })
          }
          void processTrackInfo(path)
        }
      },
      { immediate: true },
    )
  }

  /** 清除指定音轨的缓存 */
  const clearCache = (trackPath: string): void => {
    if (trackPath) {
      deleteCached(trackPath)
    }
  }

  /** 清除所有缓存 */
  const clearAllCache = (): void => {
    accessOrder.clear()
    sharedProcessedTracks.value.clear()
  }

  return {
    processedTracks: sharedProcessedTracks,
    getTrackTitle,
    getTrackArtist,
    processTrackInfo,
    watchTrack,
    clearCache,
    clearAllCache,
  }
}
