/**
 * 插件存储
 * 提供基于 localStorage 的插件持久化存储:
 * 1MB 限额(超限裁剪大型数组)、QuotaExceeded 紧急清理、防抖保存。
 */

import { reactive } from 'vue'
import logger from '../utils/logger'

// 插件存储的 localStorage key 前缀
export const PLUGIN_STORAGE_PREFIX = 'mercurial-plugin-storage-'

// 持久化插件存储对象:
// 除任意数据键外,还带两个生命周期方法:
// - flush: 清除防抖定时器并立即保存(用于插件停用/应用关闭时)
// - cleanup: 清除防抖定时器但不保存(用于插件卸载时)
export interface PluginPersistentStorage {
  [key: string]: unknown
  flush: () => Promise<void>
  cleanup: () => void
}

/**
 * 创建插件持久化存储
 */
export function createPluginStorage(pluginId: string): PluginPersistentStorage {
  const storageKey = PLUGIN_STORAGE_PREFIX + pluginId
  let savedData: Record<string, unknown> = {}

  try {
    const saved = localStorage.getItem(storageKey)
    if (saved) {
      const parsed: unknown = JSON.parse(saved)
      // 校验类型:localStorage 中可能残留 "null" / "5" / "[]" 等非普通对象值,
      // 传入 reactive 会导致存储行为异常
      if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
        savedData = parsed as Record<string, unknown>
      } else {
        logger.warn(`插件 ${pluginId} 存储数据格式异常,已重置`)
      }
    }
  } catch (e) {
    logger.warn(`加载插件 ${pluginId} 存储失败:`, e)
  }

  const storage = reactive(savedData)
  const maxStorageSize = 1024 * 1024
  let saveTimeout: ReturnType<typeof setTimeout> | null = null
  // 保存串行化队列: 保证写入按顺序执行,flush 可保证追加一次保存
  let saveQueue: Promise<void> = Promise.resolve()

  const doSave = async (target: Record<string, unknown>) => {
    try {
      const json = JSON.stringify(target)
      if (json.length > maxStorageSize) {
        logger.warn(`插件 ${pluginId} 存储超过限制`)
        // 清理大型数组数据
        for (const key of Object.keys(target)) {
          if (Array.isArray(target[key]) && (target[key] as unknown[]).length > 10) {
            target[key] = (target[key] as unknown[]).slice(
              -Math.floor((target[key] as unknown[]).length / 2),
            )
          }
        }
      }
      localStorage.setItem(storageKey, JSON.stringify(target))
    } catch (e) {
      if ((e as Error).name === 'QuotaExceededError') {
        logger.error(`插件 ${pluginId} 存储空间不足`)
        // 紧急清理策略
        for (const key of Object.keys(target)) {
          if (Array.isArray(target[key])) {
            target[key] = (target[key] as unknown[]).slice(-10)
          }
        }
        try {
          localStorage.setItem(storageKey, JSON.stringify(target))
        } catch {
          localStorage.removeItem(storageKey)
        }
      } else {
        logger.warn(`保存插件 ${pluginId} 存储失败:`, e)
      }
    }
  }

  const enqueueSave = (target: Record<string, unknown>): Promise<void> => {
    // 吞掉错误: doSave 内部已处理,避免队列因单次失败而断裂
    saveQueue = saveQueue.then(
      () => doSave(target),
      () => doSave(target),
    )
    return saveQueue
  }

  const debouncedSave = (target: Record<string, unknown>) => {
    if (saveTimeout) clearTimeout(saveTimeout)
    saveTimeout = setTimeout(() => void enqueueSave(target), 300) // 减少延迟
  }

  const cancelPendingSave = () => {
    if (saveTimeout) {
      clearTimeout(saveTimeout)
      saveTimeout = null
    }
  }

  /**
   * 生命周期方法: flush / cleanup
   *
   * 必须作为不可枚举、不可覆盖的能力暴露,而不是写成 storage 上的数据键。
   * 早期实现直接 `persistentStorage.flush = ...`,由于 storage 是 Proxy,
   * 赋值会走 set 陷阱 → 变成 reactive 对象的可枚举自有属性,带来两个后果:
   *   1. api.storage.getAll() 的 `{ ...storage }` 展开会带上这两个函数,
   *      宿主 postMessage 时结构化克隆抛 DataCloneError,而 pushMirror 只
   *      logger.warn 吞掉 → 整份状态镜像 (playerState/theme/tracks) 静默丢失;
   *   2. 插件可 `api.storage.set('flush', 1)` 覆盖,使停用时的落盘静默失败。
   */
  const lifecycle = {
    flush: (): Promise<void> => {
      cancelPendingSave()
      return enqueueSave(storage)
    },
    cleanup: (): void => {
      cancelPendingSave()
    },
  }

  const LIFECYCLE_KEYS: ReadonlySet<string> = new Set(Object.keys(lifecycle))

  const persistentStorage = new Proxy(storage, {
    get(target, key) {
      if (typeof key === 'string' && LIFECYCLE_KEYS.has(key)) {
        return lifecycle[key as keyof typeof lifecycle]
      }
      return Reflect.get(target, key)
    },

    set(target, key, value) {
      // 生命周期键不是数据键,允许覆盖会让 flush/cleanup 静默失效
      if (typeof key === 'string' && LIFECYCLE_KEYS.has(key)) return false
      const ok = Reflect.set(target, key, value)
      debouncedSave(target)
      return ok
    },

    deleteProperty(target, key) {
      if (typeof key === 'string' && LIFECYCLE_KEYS.has(key)) return false
      const ok = Reflect.deleteProperty(target, key)
      debouncedSave(target)
      return ok
    },

    has(target, key) {
      if (typeof key === 'string' && LIFECYCLE_KEYS.has(key)) return true
      return Reflect.has(target, key)
    },

    // 从自有键中剔除生命周期方法:使 `{ ...storage }` / Object.keys 只看到数据键
    ownKeys(target) {
      return Reflect.ownKeys(target).filter(
        (key) => typeof key !== 'string' || !LIFECYCLE_KEYS.has(key),
      )
    },

    getOwnPropertyDescriptor(target, key) {
      if (typeof key === 'string' && LIFECYCLE_KEYS.has(key)) {
        return {
          value: lifecycle[key as keyof typeof lifecycle],
          writable: false,
          enumerable: false,
          // 必须为 true:Proxy 不变量禁止把目标上不存在属性报为不可配置
          configurable: true,
        }
      }
      return Reflect.getOwnPropertyDescriptor(target, key)
    },
  }) as PluginPersistentStorage

  return persistentStorage
}
