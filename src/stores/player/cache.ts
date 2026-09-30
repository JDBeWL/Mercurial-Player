/**
 * 播放器缓存管理模块
 *
 * 从 player.ts 抽离的 LRU 缓存管理逻辑,负责文件存在性缓存和元数据缓存的
 * 懒初始化、惰性 TTL 清理和定时清理任务。
 */

import { LRUCache } from '@/utils/lruCache'
import logger from '@/utils/logger'

export interface TrackMetadata {
  title: string
  artist: string
  album: string
  duration: number
  coverPath?: string
  bitrate: number | null
  sampleRate: number | null
  channels: number | null
  bitDepth: number | null
  format: string | null
}

/** 文件存在性缓存: maxSize=200, TTL=30s */
const FILE_EXISTS_CACHE_MAX = 200
const FILE_EXISTS_CACHE_TTL = 30_000

/** 元数据缓存: maxSize=500, TTL=5min */
const METADATA_CACHE_MAX = 500
const METADATA_CACHE_TTL = 300_000

/** 缓存清理任务间隔: 5 分钟 */
const CLEANUP_INTERVAL = 300_000

/** 清理时分块大小,避免阻塞主线程 */
const CLEANUP_CHUNK_SIZE = 50

/**
 * 播放器缓存管理器
 *
 * 管理两个独立的 LRU 缓存:文件存在性检查和元数据查询结果。
 * 提供懒初始化、定时清理和手动清理接口。
 */
export class PlayerCacheManager {
  private fileExistsCache: LRUCache<boolean> | null = null
  private metadataCache: LRUCache<TrackMetadata> | null = null
  private cleanupTimerId: ReturnType<typeof setInterval> | null = null

  /** 获取文件存在性缓存(懒初始化) */
  getFileExistsCache(): LRUCache<boolean> {
    if (!this.fileExistsCache) {
      this.fileExistsCache = new LRUCache<boolean>(FILE_EXISTS_CACHE_MAX, FILE_EXISTS_CACHE_TTL)
    }
    return this.fileExistsCache
  }

  /** 获取元数据缓存(懒初始化) */
  getMetadataCache(): LRUCache<TrackMetadata> {
    if (!this.metadataCache) {
      this.metadataCache = new LRUCache<TrackMetadata>(METADATA_CACHE_MAX, METADATA_CACHE_TTL)
    }
    return this.metadataCache
  }

  /** 启动定时清理任务(5 分钟周期) */
  startCleanupTask(): void {
    if (this.cleanupTimerId) return
    this.cleanupTimerId = setInterval(() => {
      this.cleanup().catch((err) => logger.error('Cache cleanup failed:', err))
    }, CLEANUP_INTERVAL)
  }

  /** 停止定时清理任务 */
  stopCleanupTask(): void {
    if (this.cleanupTimerId) {
      clearInterval(this.cleanupTimerId)
      this.cleanupTimerId = null
    }
  }

  /**
   * 惰性清理:遍历所有 key 触发 get(),让过期的条目被自动删除
   * 分块处理避免阻塞主线程
   */
  async cleanup(): Promise<void> {
    if (this.fileExistsCache) {
      const keys = Array.from(this.fileExistsCache.keys())
      for (let i = 0; i < keys.length; i++) {
        this.fileExistsCache.get(keys[i]!)
        if (i > 0 && i % CLEANUP_CHUNK_SIZE === 0) {
          await new Promise((resolve) => setTimeout(resolve, 0))
        }
      }
    }
    if (this.metadataCache) {
      const keys = Array.from(this.metadataCache.keys())
      for (let i = 0; i < keys.length; i++) {
        this.metadataCache.get(keys[i]!)
        if (i > 0 && i % CLEANUP_CHUNK_SIZE === 0) {
          await new Promise((resolve) => setTimeout(resolve, 0))
        }
      }
    }
    logger.debug('Cache cleanup completed')
  }

  /** 销毁所有缓存和定时器 */
  destroy(): void {
    this.stopCleanupTask()
    this.fileExistsCache?.clear()
    this.metadataCache?.clear()
    this.fileExistsCache = null
    this.metadataCache = null
  }
}
