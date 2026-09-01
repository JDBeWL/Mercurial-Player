import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { LRUCache } from '@/utils/lruCache'
import { PlayerCacheManager, type TrackMetadata } from '@/stores/playerCache'

const metadata = (title: string): TrackMetadata => ({
  title,
  artist: 'artist',
  album: 'album',
  duration: 100,
  bitrate: 320,
  sampleRate: 44100,
  channels: 2,
  bitDepth: 16,
  format: 'mp3',
})

describe('PlayerCacheManager', () => {
  let manager: PlayerCacheManager

  beforeEach(() => {
    vi.useFakeTimers()
    manager = new PlayerCacheManager()
  })

  afterEach(() => {
    manager.destroy()
    vi.useRealTimers()
  })

  describe('lazy caches', () => {
    it('creates the file-exists cache on demand and reuses it', () => {
      const first = manager.getFileExistsCache()
      expect(first).toBeInstanceOf(LRUCache)
      expect(manager.getFileExistsCache()).toBe(first)
    })

    it('creates the metadata cache on demand and reuses it', () => {
      const first = manager.getMetadataCache()
      expect(first).toBeInstanceOf(LRUCache)
      expect(manager.getMetadataCache()).toBe(first)
    })

    it('keeps the two caches independent', () => {
      manager.getFileExistsCache().set('a', true)
      expect(manager.getMetadataCache().has('a')).toBe(false)
    })
  })

  describe('startCleanupTask / stopCleanupTask', () => {
    it('runs cleanup on a 5 minute interval', () => {
      const spy = vi.spyOn(manager, 'cleanup')
      manager.startCleanupTask()

      vi.advanceTimersByTime(299_999)
      expect(spy).not.toHaveBeenCalled()
      vi.advanceTimersByTime(1)
      expect(spy).toHaveBeenCalledTimes(1)

      vi.advanceTimersByTime(300_000)
      expect(spy).toHaveBeenCalledTimes(2)
    })

    it('does not stack timers when started twice', () => {
      const spy = vi.spyOn(manager, 'cleanup')
      manager.startCleanupTask()
      manager.startCleanupTask()

      vi.advanceTimersByTime(300_000)
      expect(spy).toHaveBeenCalledTimes(1)
    })

    it('stops the timer and allows a restart', () => {
      const spy = vi.spyOn(manager, 'cleanup')
      manager.startCleanupTask()
      manager.stopCleanupTask()

      vi.advanceTimersByTime(600_000)
      expect(spy).not.toHaveBeenCalled()

      manager.startCleanupTask()
      vi.advanceTimersByTime(300_000)
      expect(spy).toHaveBeenCalledTimes(1)
    })

    it('is safe to stop a task that was never started', () => {
      expect(() => manager.stopCleanupTask()).not.toThrow()
    })

    it('swallows cleanup rejections so the interval survives', async () => {
      const error = vi.spyOn(manager, 'cleanup').mockRejectedValue(new Error('boom'))
      const logger = (await import('@/utils/logger')).default
      const logError = vi.spyOn(logger, 'error').mockImplementation(() => {})

      manager.startCleanupTask()
      await vi.advanceTimersByTimeAsync(300_000)

      expect(error).toHaveBeenCalled()
      expect(logError).toHaveBeenCalledWith('Cache cleanup failed:', expect.any(Error))
    })
  })

  describe('cleanup', () => {
    it('is a no-op before any cache has been created', async () => {
      await expect(manager.cleanup()).resolves.toBeUndefined()
    })

    it('evicts expired entries from the file-exists cache', async () => {
      vi.useRealTimers()
      const cache = manager.getFileExistsCache()
      cache.set('stale', true)
      // LRUCache TTL 为 30s,直接把时间戳推到过去
      vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 31_000)

      await manager.cleanup()

      expect(cache.has('stale')).toBe(false)
      vi.mocked(Date.now).mockRestore()
    })

    it('evicts expired entries from the metadata cache', async () => {
      vi.useRealTimers()
      const cache = manager.getMetadataCache()
      cache.set('track.mp3', metadata('Song'))
      vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 301_000)

      await manager.cleanup()

      expect(cache.has('track.mp3')).toBe(false)
      vi.mocked(Date.now).mockRestore()
    })

    it('keeps fresh entries', async () => {
      const cache = manager.getMetadataCache()
      cache.set('fresh', metadata('Fresh'))
      await manager.cleanup()
      expect(cache.has('fresh')).toBe(true)
    })

    it('yields to the event loop once per 50 keys', async () => {
      vi.useRealTimers()
      const cache = manager.getMetadataCache()
      for (let i = 0; i < 120; i++) cache.set(`k${i}`, metadata(`t${i}`))
      const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')

      await manager.cleanup()

      // 120 keys => i=50 与 i=100 两次让出主线程
      expect(
        setTimeoutSpy.mock.calls.filter(([, delay]) => delay === 0).length,
      ).toBeGreaterThanOrEqual(2)
      expect(cache.size).toBe(120)
      setTimeoutSpy.mockRestore()
    })
  })

  describe('destroy', () => {
    it('stops the timer, clears both caches and drops the references', () => {
      const fileCache = manager.getFileExistsCache()
      const metaCache = manager.getMetadataCache()
      fileCache.set('a', true)
      metaCache.set('b', metadata('B'))
      manager.startCleanupTask()

      manager.destroy()

      expect(fileCache.size).toBe(0)
      expect(metaCache.size).toBe(0)
      // 引用被置空后再次获取会得到全新实例
      expect(manager.getFileExistsCache()).not.toBe(fileCache)
      expect(manager.getMetadataCache()).not.toBe(metaCache)
    })

    it('is safe to call twice', () => {
      expect(() => {
        manager.destroy()
        manager.destroy()
      }).not.toThrow()
    })
  })
})
