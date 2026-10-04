/**
 * 播放器缓存管理模块:文件存在性缓存与元数据缓存的懒初始化、惰性 TTL 清理和定时清理任务。
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

/** 文件存在性缓存:maxSize=200, TTL=30s(常量单位为毫秒) */
const FILE_EXISTS_CACHE_MAX = 200
const FILE_EXISTS_CACHE_TTL = 30_000

/** 元数据缓存:maxSize=500, TTL=5min(常量单位为毫秒) */
const METADATA_CACHE_MAX = 500
const METADATA_CACHE_TTL = 300_000

/** 定时清理间隔:5 分钟 */
const CLEANUP_INTERVAL = 300_000

/** 清理时的分块条数,避免遍历阻塞主线程 */
const CLEANUP_CHUNK_SIZE = 50

/** 播放器缓存管理器:两个独立 LRU(存在性检查 / 元数据查询结果)的懒初始化与清理入口 */
export class PlayerCacheManager {
  private fileExistsCache: LRUCache<boolean> | null = null
  private metadataCache: LRUCache<TrackMetadata> | null = null
  private cleanupTimerId: ReturnType<typeof setInterval> | null = null

  getFileExistsCache(): LRUCache<boolean> {
    if (!this.fileExistsCache) {
      this.fileExistsCache = new LRUCache<boolean>(FILE_EXISTS_CACHE_MAX, FILE_EXISTS_CACHE_TTL)
    }
    return this.fileExistsCache
  }

  getMetadataCache(): LRUCache<TrackMetadata> {
    if (!this.metadataCache) {
      this.metadataCache = new LRUCache<TrackMetadata>(METADATA_CACHE_MAX, METADATA_CACHE_TTL)
    }
    return this.metadataCache
  }

  startCleanupTask(): void {
    if (this.cleanupTimerId) return
    this.cleanupTimerId = setInterval(() => {
      this.cleanup().catch((err) => logger.error('Cache cleanup failed:', err))
    }, CLEANUP_INTERVAL)
  }

  stopCleanupTask(): void {
    if (this.cleanupTimerId) {
      clearInterval(this.cleanupTimerId)
      this.cleanupTimerId = null
    }
  }

  /** 惰性清理:遍历所有 key 触发 get() 让过期条目自删,按 CLEANUP_CHUNK_SIZE 分块让出主线程 */
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

  destroy(): void {
    this.stopCleanupTask()
    this.fileExistsCache?.clear()
    this.metadataCache?.clear()
    this.fileExistsCache = null
    this.metadataCache = null
  }
}
