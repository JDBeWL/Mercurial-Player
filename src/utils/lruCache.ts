import type { CacheItem } from '@/types'

/**
 * 基于 Map 的 LRU 缓存,支持 TTL 过期。
 * 利用 Map 保持插入顺序的特性,delete + set 实现 O(1) 的 LRU 更新。
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

    // 检查是否过期
    if (Date.now() - item.timestamp > this.ttl) {
      this.cache.delete(key)
      return null
    }

    // 移到末尾（最近使用）
    this.cache.delete(key)
    this.cache.set(key, item)
    return item.value
  }

  set(key: string, value: T): void {
    // 如果已存在,先删除
    if (this.cache.has(key)) {
      this.cache.delete(key)
    }

    // 如果达到最大大小,删除最旧的
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
