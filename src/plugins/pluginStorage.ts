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
      savedData = JSON.parse(saved)
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

  const persistentStorage = new Proxy(storage, {
    set(target, key: string, value) {
      target[key] = value
      debouncedSave(target)
      return true
    },
    deleteProperty(target, key: string) {
      delete target[key]
      debouncedSave(target)
      return true
    },
  }) as PluginPersistentStorage

  // 添加强制保存方法（用于应用关闭/插件停用时）
  persistentStorage.flush = () => {
    if (saveTimeout) {
      clearTimeout(saveTimeout)
      saveTimeout = null
    }
    return enqueueSave(storage)
  }

  // 添加清理方法（用于插件卸载时）
  persistentStorage.cleanup = () => {
    if (saveTimeout) {
      clearTimeout(saveTimeout)
      saveTimeout = null
    }
  }

  return persistentStorage
}
