import type { CacheItem } from '@/types'

/**
 * 基于 Map 插入顺序的 LRU 缓存, 支持 TTL 过期。
 *
 * 队首即最久未用, 淘汰只发生在队首; delete + set 是把条目移到队尾的 O(1) 手法。
 */
export class LRUCache<T> {
  private maxSize: number
  private ttl: number
  private cache: Map<string, CacheItem<T>>

  constructor(maxSize: number = 100, ttl: number = 60000) {
    this.maxSize = maxSize
    this.ttl = ttl
    this.cache = new Map()
  }

  get(key: string): T | null {
    const item = this.cache.get(key)
    if (!item) return null

    if (Date.now() - item.timestamp > this.ttl) {
      this.cache.delete(key)
      return null
    }

    // 移到末尾, 见类注释的 LRU 约定
    this.cache.delete(key)
    this.cache.set(key, item)
    return item.value
  }

  set(key: string, value: T): void {
    if (this.cache.has(key)) {
      this.cache.delete(key)
    }

    // 满载时从队首淘汰, 条目数不会超过 maxSize
    while (this.cache.size >= this.maxSize) {
      const firstKey = this.cache.keys().next().value
      if (firstKey) this.cache.delete(firstKey)
    }

    this.cache.set(key, {
      value,
      timestamp: Date.now(),
    })
  }

  has(key: string): boolean {
    return this.get(key) !== null
  }

  delete(key: string): void {
    this.cache.delete(key)
  }

  clear(): void {
    this.cache.clear()
  }

  get size(): number {
    return this.cache.size
  }

  keys(): IterableIterator<string> {
    return this.cache.keys()
  }
}
