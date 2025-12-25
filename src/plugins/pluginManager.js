/**
 * 插件管理器
 * 提供插件的加载、卸载、生命周期管理
 */

import { ref, reactive, markRaw } from 'vue'
import logger from '../utils/logger'
import { createPluginAPI } from './pluginAPI'
import { createPluginSandbox } from './pluginSandbox'

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
    })
    // 事件监听器
    this.eventListeners = new Map()
    // 插件存储
    this.storage = new Map()
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
      
      // 执行插件主函数
      const instance = await plugin.main(api)
      
      // 调用激活钩子
      if (instance && typeof instance.activate === 'function') {
        await instance.activate()
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
        // 调用停用钩子
        if (instanceData.instance && typeof instanceData.instance.deactivate === 'function') {
          await instanceData.instance.deactivate()
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
   */
  async uninstall(pluginId) {
    await this.deactivate(pluginId)
    this.plugins.delete(pluginId)
    this.storage.delete(pluginId)
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
   * 插件存储
   */
  getStorage(pluginId) {
    if (!this.storage.has(pluginId)) {
      this.storage.set(pluginId, reactive({}))
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
