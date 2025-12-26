/**
 * 插件管理器
 * 提供插件的加载、卸载、生命周期管理
 */

import { ref, reactive, markRaw } from 'vue'
import logger from '../utils/logger'
import { createPluginAPI } from './pluginAPI'
import { createPluginSandbox } from './pluginSandbox'

// 插件存储的 localStorage key 前缀
const STORAGE_PREFIX = 'mercurial-plugin-storage-'

// 插件状态
export const PluginState = {
  INACTIVE: 'inactive',
  ACTIVE: 'active',
  ERROR: 'error',
  DISABLED: 'disabled'
}

// 插件权限
export const PluginPermission = {
  PLAYER_READ: 'player:read',       // 读取播放器状态
  PLAYER_CONTROL: 'player:control', // 控制播放器
  LIBRARY_READ: 'library:read',     // 读取音乐库
  LYRICS_PROVIDER: 'lyrics:provider', // 提供歌词源
  UI_EXTEND: 'ui:extend',           // 扩展 UI
  VISUALIZER: 'visualizer',         // 可视化效果
  THEME: 'theme',                   // 主题
  STORAGE: 'storage',               // 本地存储
  NETWORK: 'network',               // 网络请求
}

class PluginManager {
  constructor() {
    // 已注册的插件
    this.plugins = reactive(new Map())
    // 插件实例
    this.instances = new Map()
    // 扩展点注册表
    this.extensions = reactive({
      lyricsProviders: [],      // 歌词源
      visualizers: [],          // 可视化效果
      themes: [],               // 主题
      menuItems: [],            // 菜单项
      settingsPanels: [],       // 设置面板
      playerDecorators: [],     // 播放器装饰器
      commands: [],             // 命令
      shortcuts: [],            // 快捷键
      actionButtons: [],        // 操作按钮（显示在歌词区域等位置）
    })
    // 事件监听器
    this.eventListeners = new Map()
    // 插件存储
    this.storage = new Map()
    // 播放器状态监听器
    this._playerWatcherStop = null
    this._lastPlayerState = null
  }

  /**
   * 初始化插件管理器
   * 设置播放器状态监听，向插件发送事件
   */
  async init() {
    // 延迟导入避免循环依赖
    const { watch } = await import('vue')
    const { usePlayerStore } = await import('../stores/player')
    
    const playerStore = usePlayerStore()
    
    // 监听播放器状态变化
    this._playerWatcherStop = watch(
      () => ({
        track: playerStore.currentTrack,
        isPlaying: playerStore.isPlaying,
        currentTime: playerStore.currentTime,
      }),
      (newState, oldState) => {
        const newTrackPath = newState.track?.path
        const oldTrackPath = oldState?.track?.path
        
        // 歌曲切换事件
        if (newTrackPath !== oldTrackPath) {
          this.emit('player:trackChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }
        
        // 播放状态变化事件
        if (newState.isPlaying !== oldState?.isPlaying) {
          this.emit('player:stateChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }
      },
      { immediate: false }
    )
    
    logger.info('插件管理器已初始化')
  }

  /**
   * 清理插件管理器
   */
  cleanup() {
    if (this._playerWatcherStop) {
      this._playerWatcherStop()
      this._playerWatcherStop = null
    }
  }

  /**
   * 注册插件
   * @param {Object} pluginDef 插件定义
   */
  async register(pluginDef) {
    const { id, name, version, author, description, permissions = [], main } = pluginDef

    if (!id || !name || !main) {
      throw new Error('插件必须包含 id, name 和 main')
    }

    if (this.plugins.has(id)) {
      throw new Error(`插件 ${id} 已存在`)
    }

    const plugin = reactive({
      id,
      name,
      version: version || '1.0.0',
      author: author || 'Unknown',
      description: description || '',
      permissions,
      state: PluginState.INACTIVE,
      error: null,
      main: markRaw(main),
    })

    this.plugins.set(id, plugin)
    logger.info(`插件已注册: ${name} (${id})`)

    return plugin
  }

  /**
   * 激活插件
   * @param {string} pluginId 插件 ID
   */
  async activate(pluginId) {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) {
      throw new Error(`插件 ${pluginId} 不存在`)
    }

    if (plugin.state === PluginState.ACTIVE) {
      return
    }

    try {
      // 创建插件 API
      const api = createPluginAPI(pluginId, plugin.permissions, this)
      
      // 创建沙箱环境
      const sandbox = createPluginSandbox(api)
      
      // 在沙箱中执行插件主函数
      const instance = await sandbox.execute(plugin.main)
      
      // 调用激活钩子（也在沙箱中执行）
      if (instance && typeof instance.activate === 'function') {
        await sandbox.execute(() => instance.activate())
      }

      this.instances.set(pluginId, { instance, api, sandbox })
      plugin.state = PluginState.ACTIVE
      plugin.error = null

      logger.info(`插件已激活: ${plugin.name}`)
      this.emit('plugin:activated', { pluginId, plugin })

    } catch (error) {
      plugin.state = PluginState.ERROR
      plugin.error = error.message
      logger.error(`插件激活失败: ${plugin.name}`, error)
      throw error
    }
  }

  /**
   * 停用插件
   * @param {string} pluginId 插件 ID
   */
  async deactivate(pluginId) {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) return

    const instanceData = this.instances.get(pluginId)
    if (instanceData) {
      try {
        // 调用停用钩子（在沙箱中执行）
        if (instanceData.instance && typeof instanceData.instance.deactivate === 'function') {
          if (instanceData.sandbox) {
            await instanceData.sandbox.execute(() => instanceData.instance.deactivate())
          } else {
            await instanceData.instance.deactivate()
          }
        }
        
        // 清理沙箱中的定时器
        if (instanceData.sandbox && typeof instanceData.sandbox.cleanup === 'function') {
          instanceData.sandbox.cleanup()
        }
      } catch (error) {
        logger.error(`插件停用出错: ${plugin.name}`, error)
      }

      this.instances.delete(pluginId)
    }

    // 清理该插件注册的所有扩展
    this.cleanupPluginExtensions(pluginId)

    plugin.state = PluginState.INACTIVE
    logger.info(`插件已停用: ${plugin.name}`)
    this.emit('plugin:deactivated', { pluginId, plugin })
  }

  /**
   * 卸载插件
   * @param {string} pluginId 插件 ID
   * @param {boolean} clearStorage 是否清除存储数据，默认 false
   */
  async uninstall(pluginId, clearStorage = false) {
    await this.deactivate(pluginId)
    this.plugins.delete(pluginId)
    this.storage.delete(pluginId)
    
    // 如果需要清除存储数据
    if (clearStorage) {
      try {
        localStorage.removeItem(STORAGE_PREFIX + pluginId)
        logger.info(`插件存储已清除: ${pluginId}`)
      } catch (e) {
        logger.warn(`清除插件存储失败: ${pluginId}`, e)
      }
    }
    
    logger.info(`插件已卸载: ${pluginId}`)
    this.emit('plugin:uninstalled', { pluginId })
  }

  /**
   * 清理插件注册的扩展
   */
  cleanupPluginExtensions(pluginId) {
    for (const key of Object.keys(this.extensions)) {
      this.extensions[key] = this.extensions[key].filter(ext => ext.pluginId !== pluginId)
    }
    
    // 清理事件监听器
    for (const [event, listeners] of this.eventListeners) {
      this.eventListeners.set(
        event,
        listeners.filter(l => l.pluginId !== pluginId)
      )
    }
  }

  /**
   * 注册扩展
   */
  registerExtension(type, pluginId, extension) {
    if (!this.extensions[type]) {
      this.extensions[type] = []
    }
    this.extensions[type].push({ ...extension, pluginId })
    logger.debug(`插件 ${pluginId} 注册了 ${type} 扩展`)
  }

  /**
   * 获取扩展
   */
  getExtensions(type) {
    return this.extensions[type] || []
  }

  /**
   * 事件系统
   */
  on(event, pluginId, callback) {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, [])
    }
    this.eventListeners.get(event).push({ pluginId, callback })
  }

  off(event, pluginId, callback) {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      const index = listeners.findIndex(l => l.pluginId === pluginId && l.callback === callback)
      if (index > -1) {
        listeners.splice(index, 1)
      }
    }
  }

  emit(event, data) {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      for (const { callback } of listeners) {
        try {
          callback(data)
        } catch (error) {
          logger.error(`事件处理出错: ${event}`, error)
        }
      }
    }
  }

  /**
   * 插件存储 - 带持久化和大小限制
   */
  getStorage(pluginId) {
    if (!this.storage.has(pluginId)) {
      const storageKey = STORAGE_PREFIX + pluginId
      let savedData = {}
      
      try {
        const saved = localStorage.getItem(storageKey)
        if (saved) {
          savedData = JSON.parse(saved)
        }
      } catch (e) {
        logger.warn(`加载插件 ${pluginId} 存储失败:`, e)
      }
      
      const storage = reactive(savedData)
      const maxStorageSize = 1024 * 1024 // 每个插件最多 1MB
      
      // 安全保存函数，带大小检查
      const safeSave = (target) => {
        try {
          const json = JSON.stringify(target)
          if (json.length > maxStorageSize) {
            logger.warn(`插件 ${pluginId} 存储超过限制 (${(json.length / 1024).toFixed(1)}KB > 1MB)，将清理旧数据`)
            // 如果是数组类型的数据，保留最新的一半
            for (const key of Object.keys(target)) {
              if (Array.isArray(target[key]) && target[key].length > 10) {
                target[key] = target[key].slice(-Math.floor(target[key].length / 2))
              }
            }
          }
          localStorage.setItem(storageKey, JSON.stringify(target))
        } catch (e) {
          if (e.name === 'QuotaExceededError') {
            logger.error(`插件 ${pluginId} 存储空间不足，清理数据`)
            // 清理所有数组数据
            for (const key of Object.keys(target)) {
              if (Array.isArray(target[key])) {
                target[key] = target[key].slice(-10) // 只保留最新10条
              }
            }
            try {
              localStorage.setItem(storageKey, JSON.stringify(target))
            } catch {
              // 如果还是失败，清空存储
              localStorage.removeItem(storageKey)
            }
          } else {
            logger.warn(`保存插件 ${pluginId} 存储失败:`, e)
          }
        }
      }
      
      // 使用防抖保存，避免频繁写入
      let saveTimeout = null
      const debouncedSave = (target) => {
        if (saveTimeout) clearTimeout(saveTimeout)
        saveTimeout = setTimeout(() => safeSave(target), 500)
      }
      
      const persistentStorage = new Proxy(storage, {
        set(target, key, value) {
          target[key] = value
          debouncedSave(target)
          return true
        },
        deleteProperty(target, key) {
          delete target[key]
          debouncedSave(target)
          return true
        }
      })
      
      this.storage.set(pluginId, persistentStorage)
    }
    return this.storage.get(pluginId)
  }

  /**
   * 获取所有插件
   */
  getAllPlugins() {
    return Array.from(this.plugins.values())
  }

  /**
   * 获取活跃插件
   */
  getActivePlugins() {
    return this.getAllPlugins().filter(p => p.state === PluginState.ACTIVE)
  }
}

// 单例
export const pluginManager = new PluginManager()
export default pluginManager
