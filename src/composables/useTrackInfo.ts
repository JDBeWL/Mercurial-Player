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
 * 缓存上限,避免长期使用后无限增长。
 * 200 条足够覆盖常见使用场景 (一张专辑通常 10-20 首,200 条 = 10-20 张专辑)。
 */
const MAX_PROCESSED_TRACKS = 200

/**
 * 访问顺序追踪,用于 LRU 驱逐。
 * 用独立数组记录 key 的访问顺序,最近访问的在末尾。
 */
const accessOrder: string[] = []

/** 将 key 移到 accessOrder 末尾 (最近使用) */
function touchKey(key: string): void {
  const idx = accessOrder.indexOf(key)
  if (idx !== -1) accessOrder.splice(idx, 1)
  accessOrder.push(key)
}

/** 当缓存超过上限时,驱逐最久未使用的 key */
function evictIfNeeded(cache: Record<string, ProcessedTrackInfo>): void {
  while (accessOrder.length > MAX_PROCESSED_TRACKS) {
    const oldest = accessOrder.shift()
    if (oldest) delete cache[oldest]
  }
}

// 模块级别的共享缓存,确保所有 useTrackInfo 实例共享同一份数据,
// 避免 App.vue 和 MiniPlayer.vue 各创建一份独立缓存。
// 使用 ref 包裹 Record 保持 Vue 响应式 (processTrackInfo 完成后模板会自动更新)。
const sharedProcessedTracks = ref<Record<string, ProcessedTrackInfo>>({})

// 模块级别的 store 引用(在首次调用 useTrackInfo 时赋值)
let _configStore: ReturnType<typeof useConfigStore> | null = null

function ensureConfigStore(): ReturnType<typeof useConfigStore> {
  if (!_configStore) {
    _configStore = useConfigStore()
  }
  return _configStore
}

/** 从缓存读取并更新访问顺序 (LRU) */
function getCached(trackPath: string): ProcessedTrackInfo | undefined {
  const value = sharedProcessedTracks.value[trackPath]
  if (value) touchKey(trackPath)
  return value
}

/** 写入缓存并更新访问顺序 (LRU) */
function setCached(trackPath: string, value: ProcessedTrackInfo): void {
  if (!sharedProcessedTracks.value[trackPath]) {
    // 新 key,可能需要驱逐
    evictIfNeeded(sharedProcessedTracks.value)
  }
  sharedProcessedTracks.value[trackPath] = value
  touchKey(trackPath)
}

/** 删除缓存项 */
function deleteCached(trackPath: string): void {
  const idx = accessOrder.indexOf(trackPath)
  if (idx !== -1) accessOrder.splice(idx, 1)
  delete sharedProcessedTracks.value[trackPath]
}

/**
 * 异步处理音轨信息
 */
async function processTrackInfo(trackPath: string): Promise<void> {
  try {
    // 如果已经在处理中,跳过
    if (getCached(trackPath)?.processing) return

    // 标记为处理中
    setCached(trackPath, { processing: true })

    const configStore = ensureConfigStore()
    // 获取配置
    const config = {
      preferMetadata: configStore.titleExtraction?.preferMetadata ?? true,
      hideFileExtension: configStore.titleExtraction?.hideFileExtension ?? true,
      parseArtistTitle: configStore.titleExtraction?.parseArtistTitle ?? true,
      separator: configStore.titleExtraction?.separator ?? '-',
      customSeparators: configStore.titleExtraction?.customSeparators ?? ['-', '_', '.', ' ']
    }

    // 使用 TitleExtractor 智能提取标题信息
    const titleInfo = await TitleExtractor.extractTitle(trackPath, config)

    // 更新处理结果
    setCached(trackPath, {
      processing: false,
      ...titleInfo
    })

  } catch (error) {
    logger.error('处理音轨信息失败:', trackPath, error)
    // 出错时使用文件名作为标题
    setCached(trackPath, {
      processing: false,
      title: FileUtils.getFileName(trackPath),
      artist: '',
      fileName: FileUtils.getFileName(trackPath),
      isFromMetadata: false
    })
  }
}

/**
 * 音轨信息处理 composable
 *
 * 使用模块级共享缓存 (sharedProcessedTracks),所有 useTrackInfo 实例
 * 共享同一份数据,避免 App.vue 和 MiniPlayer.vue 各创建一份独立缓存。
 * 缓存有 LRU 上限 (200 条),防止长期使用后无限增长。
 */
export function useTrackInfo() {
  // 确保模块级 configStore 引用已初始化
  ensureConfigStore()

  /**
   * 获取音轨标题
   */
  const getTrackTitle = (track: Track | null | undefined, fallback: string = ''): string => {
    if (!track || !track.path) {
      return fallback
    }

    const trackPath = track.path

    // 如果已经处理过该音轨,直接返回结果
    const cached = getCached(trackPath)
    if (cached && !cached.processing) {
      return cached.title || fallback
    }

    // 异步处理音轨信息,但不阻塞当前渲染
    if (!cached || !cached.processing) {
      processTrackInfo(trackPath)
    }

    // 处理中:优先读 store 已用元数据填充的 title 字段,避免显示原始文件名
    return track.title || track.name || FileUtils.getFileName(trackPath)
  }

  /**
   * 获取音轨艺术家
   */
  const getTrackArtist = (track: Track | null | undefined, fallback: string = ''): string => {
    if (!track || !track.path) {
      return fallback
    }

    const trackPath = track.path

    // 如果已经处理过该音轨,直接返回结果
    const cached = getCached(trackPath)
    if (cached && !cached.processing) {
      return cached.artist || fallback
    }

    // 异步处理音轨信息,但不阻塞当前渲染
    if (!cached || !cached.processing) {
      processTrackInfo(trackPath)
    }

    // 处理中:读 store 已填充的 artist 字段
    return track.artist || fallback
  }

  /**
   * 设置音轨变化监听器
   * 切换曲目时先用 track.title/artist（store 已用元数据填充）预填缓存,
   * 再触发异步精细提取,避免首次渲染返回原始文件名造成视觉抖动。
   */
  const watchTrack = (trackGetter: () => Track | null | undefined): WatchStopHandle => {
    return watch(trackGetter, (newTrack) => {
      if (newTrack && newTrack.path) {
        const path = newTrack.path
        // 如果尚无缓存或仍在处理中,先用已有的 title/artist 预填
        // 让 getTrackTitle/getTrackArtist 在异步完成前也能返回有意义的值
        const existing = getCached(path)
        if (!existing || existing.processing) {
          const preTitle = newTrack.title || newTrack.name || FileUtils.getFileName(path)
          const preArtist = newTrack.artist || ''
          setCached(path, {
            processing: true,
            title: preTitle,
            artist: preArtist,
            fileName: FileUtils.getFileName(path),
            isFromMetadata: false
          })
        }
        processTrackInfo(path)
      }
    }, { immediate: true })
  }

  /**
   * 清除指定音轨的缓存
   */
  const clearCache = (trackPath: string): void => {
    if (trackPath) {
      deleteCached(trackPath)
    }
  }

  /**
   * 清除所有缓存
   */
  const clearAllCache = (): void => {
    accessOrder.length = 0
    sharedProcessedTracks.value = {}
  }

  return {
    processedTracks: sharedProcessedTracks,
    getTrackTitle,
    getTrackArtist,
    processTrackInfo,
    watchTrack,
    clearCache,
    clearAllCache
  }
}
