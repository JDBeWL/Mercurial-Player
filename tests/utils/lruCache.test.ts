import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { LRUCache } from '@/utils/lruCache'

describe('LRUCache', () => {
  let cache: LRUCache<string>

  beforeEach(() => {
    cache = new LRUCache(3, 60000)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('基本操作', () => {
    it('set/get 应正确存取值', () => {
      cache.set('a', 'value-a')
      expect(cache.get('a')).toBe('value-a')
    })

    it('get 不存在的 key 应返回 null', () => {
      expect(cache.get('nonexistent')).toBeNull()
    })

    it('delete 后 get 应返回 null', () => {
      cache.set('a', 'value-a')
      cache.delete('a')
      expect(cache.get('a')).toBeNull()
    })

    it('clear 应清空所有缓存', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.clear()
      expect(cache.size).toBe(0)
      expect(cache.get('a')).toBeNull()
    })

    it('size 应返回当前缓存数量', () => {
      expect(cache.size).toBe(0)
      cache.set('a', '1')
      expect(cache.size).toBe(1)
      cache.set('b', '2')
      expect(cache.size).toBe(2)
    })

    it('keys 应返回所有 key 的迭代器', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      const keys = Array.from(cache.keys())
      expect(keys).toEqual(['a', 'b'])
    })
  })

  describe('LRU 淘汰逻辑', () => {
    it('超过 maxSize 时应淘汰最久未使用的条目', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')
      cache.set('d', '4') // 'a' 应被淘汰

      expect(cache.get('a')).toBeNull()
      expect(cache.get('b')).toBe('2')
      expect(cache.get('c')).toBe('3')
      expect(cache.get('d')).toBe('4')
      expect(cache.size).toBe(3)
    })

    it('get 应将条目移到最近使用位置', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')

      // 访问 'a'，使其成为最近使用
      cache.get('a')

      // 插入 'd'，应淘汰 'b'（最久未使用），而非 'a'
      cache.set('d', '4')

      expect(cache.get('a')).toBe('1')
      expect(cache.get('b')).toBeNull()
      expect(cache.get('c')).toBe('3')
      expect(cache.get('d')).toBe('4')
    })

    it('set 已存在的 key 应更新值并移到最近使用位置', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')

      // 更新 'a'，使其成为最近使用
      cache.set('a', 'updated')

      // 插入 'd'，应淘汰 'b'
      cache.set('d', '4')

      expect(cache.get('a')).toBe('updated')
      expect(cache.get('b')).toBeNull()
      expect(cache.get('c')).toBe('3')
      expect(cache.get('d')).toBe('4')
    })

    it('maxSize=1 时只保留最后一个条目', () => {
      const tiny = new LRUCache<string>(1, 60000)
      tiny.set('a', '1')
      tiny.set('b', '2')
      expect(tiny.get('a')).toBeNull()
      expect(tiny.get('b')).toBe('2')
      expect(tiny.size).toBe(1)
    })

    it('连续 set 超过 maxSize 多个条目应正确淘汰', () => {
      const c = new LRUCache<number>(3, 60000)
      c.set('a', 1)
      c.set('b', 2)
      c.set('c', 3)
      c.set('d', 4)
      c.set('e', 5)
      c.set('f', 6)

      // 应只保留最后 3 个
      expect(c.size).toBe(3)
      expect(c.get('a')).toBeNull()
      expect(c.get('b')).toBeNull()
      expect(c.get('c')).toBeNull()
      expect(c.get('d')).toBe(4)
      expect(c.get('e')).toBe(5)
      expect(c.get('f')).toBe(6)
    })
  })

  describe('TTL 过期', () => {
    it('过期条目 get 时应返回 null 并删除', () => {
      vi.useFakeTimers()
      const shortTtl = new LRUCache<string>(10, 1000) // 1 秒过期

      shortTtl.set('a', '1')
      expect(shortTtl.get('a')).toBe('1')

      vi.advanceTimersByTime(1001)

      expect(shortTtl.get('a')).toBeNull()
      expect(shortTtl.size).toBe(0)
    })

    it('未过期条目应正常返回', () => {
      vi.useFakeTimers()
      const cache = new LRUCache<string>(10, 5000)

      cache.set('a', '1')
      vi.advanceTimersByTime(4000)
      expect(cache.get('a')).toBe('1')
    })

    it('TTL 边界：恰好 ttl 毫秒后应过期', () => {
      vi.useFakeTimers()
      const cache = new LRUCache<string>(10, 1000)

      cache.set('a', '1')
      vi.advanceTimersByTime(1000)
      // Date.now() - timestamp > ttl → 1000 > 1000 为 false，未过期
      expect(cache.get('a')).toBe('1')

      vi.advanceTimersByTime(1)
      // 1001 > 1000 为 true，过期
      expect(cache.get('a')).toBeNull()
    })

    it('set 更新已存在 key 应刷新 timestamp', () => {
      vi.useFakeTimers()
      const cache = new LRUCache<string>(10, 1000)

      cache.set('a', '1')
      vi.advanceTimersByTime(800)
      cache.set('a', 'updated') // 刷新 timestamp
      vi.advanceTimersByTime(800) // 距离第一次 set 1600ms，但距离第二次 set 只有 800ms

      expect(cache.get('a')).toBe('updated')
    })
  })

  describe('has 方法', () => {
    it('存在且未过期应返回 true', () => {
      cache.set('a', '1')
      expect(cache.has('a')).toBe(true)
    })

    it('不存在应返回 false', () => {
      expect(cache.has('nonexistent')).toBe(false)
    })

    it('过期应返回 false', () => {
      vi.useFakeTimers()
      const shortTtl = new LRUCache<string>(10, 1000)
      shortTtl.set('a', '1')
      vi.advanceTimersByTime(1001)
      expect(shortTtl.has('a')).toBe(false)
    })

    it('has 会触发 LRU 顺序更新（副作用）', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')

      // has('a') 应将 'a' 移到末尾
      cache.has('a')

      // 插入 'd'，应淘汰 'b' 而非 'a'
      cache.set('d', '4')

      expect(cache.has('a')).toBe(true)
      expect(cache.has('b')).toBe(false)
    })
  })

  describe('边界条件', () => {
    it('默认构造参数应正常工作', () => {
      const defaultCache = new LRUCache<string>()
      defaultCache.set('a', '1')
      expect(defaultCache.get('a')).toBe('1')
    })

    it('存储对象引用类型应保持引用', () => {
      const objCache = new LRUCache<{ id: number }>(10, 60000)
      const obj = { id: 42 }
      objCache.set('key', obj)
      expect(objCache.get('key')).toBe(obj) // 同一引用
    })

    it('get 已删除的 key 应返回 null', () => {
      cache.set('a', '1')
      cache.delete('a')
      expect(cache.get('a')).toBeNull()
    })

    it('delete 不存在的 key 不应报错', () => {
      expect(() => cache.delete('nonexistent')).not.toThrow()
    })

    it('set 覆盖不应增加 size', () => {
      cache.set('a', '1')
      cache.set('a', '2')
      cache.set('a', '3')
      expect(cache.size).toBe(1)
    })

    it('空缓存的 size 应为 0', () => {
      expect(cache.size).toBe(0)
    })

    it('空缓存的 keys 应返回空迭代器', () => {
      expect(Array.from(cache.keys())).toEqual([])
    })
  })

  describe('LRU 顺序验证', () => {
    it('keys() 应按最近使用顺序返回', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')

      // 顺序: a -> b -> c
      expect(Array.from(cache.keys())).toEqual(['a', 'b', 'c'])

      // 访问 a，顺序变为: b -> c -> a
      cache.get('a')
      expect(Array.from(cache.keys())).toEqual(['b', 'c', 'a'])

      // 访问 b，顺序变为: c -> a -> b
      cache.get('b')
      expect(Array.from(cache.keys())).toEqual(['c', 'a', 'b'])
    })

    it('淘汰顺序应为 LRU（最久未使用先淘汰）', () => {
      cache.set('a', '1')
      cache.set('b', '2')
      cache.set('c', '3')

      // 访问顺序: a, c（b 最久未使用）
      cache.get('a')
      cache.get('c')

      // 插入 d，淘汰 b
      cache.set('d', '4')
      expect(cache.has('b')).toBe(false)
      expect(Array.from(cache.keys())).toEqual(['a', 'c', 'd'])
    })
  })
})
