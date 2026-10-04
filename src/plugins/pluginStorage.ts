/**
 * 插件存储:按 `PLUGIN_STORAGE_PREFIX + pluginId` 给每个插件一份独立的 localStorage 命名空间
 *
 * 序列化上限 1MB,超限裁剪数组;QuotaExceeded 走紧急截断;写入经 300ms 防抖并串行排队
 */

import { reactive } from 'vue'
import logger from '../utils/logger'
import errorHandler, { ErrorType, ErrorSeverity } from '../utils/errorHandler'

export const PLUGIN_STORAGE_PREFIX = 'mercurial-plugin-storage-'

/**
 * 插件持久化存储:除数据键外带两个不可枚举的生命周期方法
 *
 * flush 取消防抖并立即落盘(停用/应用关闭),cleanup 只取消防抖不保存(卸载)
 */
export interface PluginPersistentStorage {
  [key: string]: unknown
  flush: () => Promise<void>
  cleanup: () => void
}

/** 为单个插件创建持久化存储实例;已有的旧数据格式异常则重置为空 */
export function createPluginStorage(pluginId: string): PluginPersistentStorage {
  const storageKey = PLUGIN_STORAGE_PREFIX + pluginId
  let savedData: Record<string, unknown> = {}

  try {
    const saved = localStorage.getItem(storageKey)
    if (saved) {
      const parsed: unknown = JSON.parse(saved)
      // 只接受纯对象:localStorage 可能残留 "null"/"5"/"[]" 等值,传进 reactive 会行为异常
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
  // 配额告急时每个数组至少保留的条数(保留最近的,丢弃靠前的历史)
  const QUOTA_KEEP_TAIL = 10
  let saveTimeout: ReturnType<typeof setTimeout> | null = null
  // 写入串行队列:保证按顺序落盘,flush 至少追加一次保存
  let saveQueue: Promise<void> = Promise.resolve()

  const doSave = async (target: Record<string, unknown>) => {
    try {
      const json = JSON.stringify(target)
      if (json.length > maxStorageSize) {
        logger.warn(`插件 ${pluginId} 存储超过限制`)
        // 超 1MB 时把长度大于 10 的数组各裁掉一半(保留后半)
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
        // 紧急清理:数组一律裁到 QUOTA_KEEP_TAIL
        const dropped: string[] = []
        for (const key of Object.keys(target)) {
          if (Array.isArray(target[key])) {
            const arr = target[key] as unknown[]
            if (arr.length > QUOTA_KEEP_TAIL) {
              target[key] = arr.slice(-QUOTA_KEEP_TAIL)
              dropped.push(`${key}(少了 ${arr.length - QUOTA_KEEP_TAIL} 条)`)
            }
          }
        }
        let lostEverything = false
        try {
          localStorage.setItem(storageKey, JSON.stringify(target))
        } catch {
          localStorage.removeItem(storageKey)
          lostEverything = true
        }
        // 截断会销毁插件自己的历史数据,只写 console 等于没人知道,必须走统一错误出口提示用户
        errorHandler.handle(
          new Error(lostEverything ? '存储已清空' : `已截断 ${dropped.length} 个数组`),
          {
            type: ErrorType.CONFIG_SAVE_ERROR,
            severity: ErrorSeverity.HIGH,
            context: { pluginId, truncated: dropped.join(', ') || '(无)' },
            userMessage: lostEverything
              ? `插件 ${pluginId} 的本地存储已满，数据已全部清除`
              : `插件 ${pluginId} 的本地存储已满，已丢弃部分历史数据：${dropped.join('、')}`,
            showToUser: true,
          },
        )
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
    saveTimeout = setTimeout(() => void enqueueSave(target), 300)
  }

  const cancelPendingSave = () => {
    if (saveTimeout) {
      clearTimeout(saveTimeout)
      saveTimeout = null
    }
  }

  /**
   * flush/cleanup 必须是不可枚举,不可覆盖的能力,不能写成 storage 数据键
   *
   * 写成数据键后 getAll() 展开会带出函数,postMessage 克隆抛错又被 warn 吞掉,整份状态镜像静默丢失;
   * 且插件可以 set('flush') 覆盖它,停用时的落盘就此失效
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
      // 生命周期键不可被插件覆盖,理由见 lifecycle 的说明
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
