/**
 * 插件管理器
 * 提供插件的加载、卸载、生命周期管理
 */

import { reactive, markRaw, watch, type WatchStopHandle } from 'vue'
import logger from '../utils/logger'
import { createPluginAPI } from './pluginAPI'
import { createPluginSandbox, type PluginSandbox } from './pluginSandbox'
import {
  createPluginStorage,
  PLUGIN_STORAGE_PREFIX,
  type PluginPersistentStorage,
} from './pluginStorage'
import { usePlayerStore } from '../stores/player'
import {
  PluginState,
  type PluginAPI,
  type Plugin,
  type ActionButton,
  type BuiltinPluginDefinition,
  type Command,
  type EventCallback,
  type LyricsProvider,
  type MenuItem,
  type PlayerDecorator,
  type PluginDefinition,
  type PluginInstance,
  type PluginMainFunction,
  type PluginPermissionType,
  type SettingsPanel,
  type Shortcut,
  type Visualizer,
} from './pluginTypes'

// 纯类型契约定义已拆分至 pluginTypes.ts,此处 re-export 保持向后兼容
// (现有 `import { pluginManager, PluginState, PluginPermission } from './pluginManager'` 等不受影响)
export { PluginState, PluginPermission } from './pluginTypes'
export type {
  Track,
  PluginAPI,
  PluginStateType,
  PluginPermissionType,
  PlayerState,
  LyricLine,
  Playlist,
  ThemeInfo,
  SettingsPanel,
  MenuItem,
  PlayerDecorator,
  ActionButton,
  LyricsProvider,
  LyricsSearchQuery,
  LyricsSearchResult,
  Visualizer,
  Command,
  Shortcut,
  SaveAsOptions,
  EventCallback,
  PluginMainFunction,
  PluginInstance,
  PluginDefinition,
  BuiltinPluginDefinition,
  Plugin,
} from './pluginTypes'

// 插件实例数据
interface PluginInstanceData {
  instance: PluginInstance
  api: PluginAPI
  sandbox: PluginSandbox
}

// 扩展注册表类型
interface Extensions {
  lyricsProviders: (LyricsProvider & { pluginId: string })[]
  visualizers: (Visualizer & { pluginId: string })[]
  themes: { pluginId: string; [key: string]: unknown }[]
  menuItems: (MenuItem & { pluginId: string })[]
  settingsPanels: (SettingsPanel & { pluginId: string })[]
  playerDecorators: (PlayerDecorator & { pluginId: string })[]
  commands: (Command & { pluginId: string })[]
  shortcuts: (Shortcut & { pluginId: string })[]
  actionButtons: (ActionButton & { pluginId: string })[]
}

// 事件监听器类型
interface EventListener {
  pluginId: string
  callback: EventCallback
}

class PluginManager {
  // 已注册的插件
  plugins: Map<string, Plugin>
  // 插件实例
  private instances: Map<string, PluginInstanceData>
  // 扩展点注册表
  extensions: Extensions
  // 事件监听器
  private eventListeners: Map<string, EventListener[]>
  // 插件存储
  private storage: Map<string, PluginPersistentStorage>
  // 播放器状态监听器
  private _playerWatcherStop: WatchStopHandle | null

  constructor() {
    this.plugins = reactive(new Map()) as Map<string, Plugin>
    this.instances = new Map()
    this.extensions = reactive({
      lyricsProviders: [],
      visualizers: [],
      themes: [],
      menuItems: [],
      settingsPanels: [],
      playerDecorators: [],
      commands: [],
      shortcuts: [],
      actionButtons: [],
    })
    this.eventListeners = new Map()
    this.storage = new Map()
    this._playerWatcherStop = null
  }

  /**
   * 初始化插件管理器
   */
  async init(): Promise<void> {
    const playerStore = usePlayerStore()

    this._playerWatcherStop = watch(
      () => ({
        track: playerStore.currentTrack,
        isPlaying: playerStore.isPlaying,
      }),
      (newState, oldState) => {
        const newTrackPath = newState.track?.path
        const oldTrackPath = oldState?.track?.path

        if (newTrackPath !== oldTrackPath) {
          this.emit('player:trackChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }

        if (newState.isPlaying !== oldState?.isPlaying) {
          this.emit('player:stateChanged', {
            track: newState.track ? { ...newState.track } : null,
            isPlaying: newState.isPlaying,
          })
        }
      },
      { immediate: false },
    )

    logger.info('插件管理器已初始化')
  }

  /**
   * 清理插件管理器
   *
   * 完整资源释放流程:
   * 1. 停止 player watcher
   * 2. 逐个停用所有 active 插件(触发插件的 deactivate、沙箱 cleanup、扩展清理)
   * 3. 清理所有事件监听器
   * 4. 强制保存所有插件存储
   */
  async cleanup(): Promise<void> {
    if (this._playerWatcherStop) {
      this._playerWatcherStop()
      this._playerWatcherStop = null
    }

    // 停用所有 active 插件(顺序执行,避免并发资源竞争)
    // 收集 active 插件 id 后再调用 deactivate,避免迭代时修改 Map
    const activePluginIds = Array.from(this.plugins.values())
      .filter((p) => p.state === PluginState.ACTIVE)
      .map((p) => p.id)

    for (const pluginId of activePluginIds) {
      try {
        await this.deactivate(pluginId)
      } catch (error) {
        logger.error(`清理时停用插件 ${pluginId} 失败:`, error)
      }
    }

    // 清理所有残留的事件监听器(防止 deactivate 遗漏)
    this.eventListeners.clear()

    // 强制保存所有插件存储(覆盖未在 deactivate 中处理的场景)
    for (const [pluginId, storage] of this.storage) {
      try {
        storage.flush()
      } catch (e) {
        logger.warn(`强制保存插件 ${pluginId} 存储失败:`, e)
      }
    }
  }

  /**
   * 注册插件
   */
  async register(pluginDef: PluginDefinition | BuiltinPluginDefinition): Promise<Plugin> {
    const { id, name, version, author, description, permissions = [], main } = pluginDef

    if (!id || !name || !main) {
      throw new Error('插件必须包含 id, name 和 main')
    }

    if (this.plugins.has(id)) {
      throw new Error(`插件 ${id} 已存在`)
    }

    const plugin: Plugin = reactive({
      id,
      name,
      version: version || '1.0.0',
      author: author || 'Unknown',
      description: description || '',
      permissions: permissions as PluginPermissionType[],
      state: PluginState.INACTIVE,
      error: null,
      main: markRaw(main as PluginMainFunction),
    })

    this.plugins.set(id, plugin)
    logger.info(`插件已注册: ${name} (${id})`)

    return plugin
  }

  /**
   * 激活插件
   */
  async activate(pluginId: string): Promise<void> {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) {
      throw new Error(`插件 ${pluginId} 不存在`)
    }

    if (plugin.state === PluginState.ACTIVE) {
      return
    }

    if (plugin.state === PluginState.LOADING || plugin.state === PluginState.UNLOADING) {
      throw new Error(`插件 ${pluginId} 正在处理中，请稍后再试`)
    }

    plugin.state = PluginState.LOADING

    try {
      const api = createPluginAPI(pluginId, plugin.permissions, this)
      const sandbox = createPluginSandbox(api)
      const instance = await sandbox.execute(plugin.main)

      if (instance && typeof instance.activate === 'function') {
        await sandbox.execute(() => instance.activate!())
      }

      this.instances.set(pluginId, { instance, api, sandbox })
      plugin.state = PluginState.ACTIVE
      plugin.error = null

      logger.info(`插件已激活: ${plugin.name}`)
      this.emit('plugin:activated', { pluginId, plugin })
    } catch (error) {
      plugin.state = PluginState.ERROR
      plugin.error = error instanceof Error ? error.message : String(error)
      logger.error(`插件激活失败: ${plugin.name}`, error)
      throw error
    }
  }

  /**
   * 停用插件
   */
  async deactivate(pluginId: string): Promise<void> {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) return

    if (plugin.state === PluginState.LOADING || plugin.state === PluginState.UNLOADING) {
      throw new Error(`插件 ${pluginId} 正在处理中，请稍后再试`)
    }

    if (plugin.state !== PluginState.ACTIVE) {
      return
    }

    plugin.state = PluginState.UNLOADING

    const instanceData = this.instances.get(pluginId)
    if (instanceData) {
      try {
        if (instanceData.instance && typeof instanceData.instance.deactivate === 'function') {
          if (instanceData.sandbox) {
            await instanceData.sandbox.execute(() => instanceData.instance.deactivate!())
          } else {
            await instanceData.instance.deactivate()
          }
        }
      } catch (error) {
        logger.error(`插件停用出错: ${plugin.name}`, error)
      } finally {
        // 确保沙箱清理总是被执行，即使 deactivate 抛出错误
        if (instanceData.sandbox && typeof instanceData.sandbox.cleanup === 'function') {
          try {
            instanceData.sandbox.cleanup()
          } catch (cleanupError) {
            logger.error(`插件沙箱清理出错: ${plugin.name}`, cleanupError)
          }
        }
      }

      this.instances.delete(pluginId)
    }

    this.cleanupPluginExtensions(pluginId)

    // 确保插件存储立即保存（清除 debounce 并立即保存）
    if (this.storage.has(pluginId)) {
      try {
        this.storage.get(pluginId)!.flush()
      } catch (e) {
        logger.warn(`插件停用时保存存储失败: ${pluginId}`, e)
      }
    }

    plugin.state = PluginState.INACTIVE
    logger.info(`插件已停用: ${plugin.name}`)
    this.emit('plugin:deactivated', { pluginId, plugin })
  }

  /**
   * 卸载插件
   */
  async uninstall(pluginId: string, clearStorage = false): Promise<void> {
    await this.deactivate(pluginId)
    this.plugins.delete(pluginId)
    this.storage.delete(pluginId)

    if (clearStorage) {
      try {
        localStorage.removeItem(PLUGIN_STORAGE_PREFIX + pluginId)
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
  cleanupPluginExtensions(pluginId: string): void {
    for (const key of Object.keys(this.extensions) as (keyof Extensions)[]) {
      const filtered = this.extensions[key].filter((ext) => ext.pluginId !== pluginId)

      ;(this.extensions as unknown as Record<string, unknown[]>)[key] = filtered
    }

    for (const [event, listeners] of this.eventListeners) {
      const filtered = listeners.filter((l) => l.pluginId !== pluginId)
      this.eventListeners.set(event, filtered)
    }
  }

  /**
   * 注册扩展
   */
  registerExtension<K extends keyof Extensions>(
    type: K,
    pluginId: string,
    extension: Omit<Extensions[K][number], 'pluginId'>,
  ): void {
    if (!this.extensions[type]) {
      ;(this.extensions as unknown as Record<string, unknown[]>)[type] = []
    }
    ;(this.extensions[type] as { pluginId: string }[]).push({ ...extension, pluginId })
    logger.debug(`插件 ${pluginId} 注册了 ${type} 扩展`)
  }

  /**
   * 获取扩展
   */
  getExtensions<K extends keyof Extensions>(type: K): Extensions[K] {
    return this.extensions[type] || []
  }

  /**
   * 事件系统
   */
  on(event: string, pluginId: string, callback: EventCallback): void {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, [])
    }
    this.eventListeners.get(event)!.push({ pluginId, callback })
  }

  off(event: string, pluginId: string, callback: EventCallback): void {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      const index = listeners.findIndex((l) => l.pluginId === pluginId && l.callback === callback)
      if (index > -1) {
        listeners.splice(index, 1)
      }
    }
  }

  emit(event: string, data?: unknown): void {
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
   * 具体的存储实现(1MB 限额、紧急清理、防抖保存)在 pluginStorage.ts
   */
  getStorage(pluginId: string): PluginPersistentStorage {
    if (!this.storage.has(pluginId)) {
      this.storage.set(pluginId, createPluginStorage(pluginId))
    }
    return this.storage.get(pluginId)!
  }

  /**
   * 获取所有插件
   */
  getAllPlugins(): Plugin[] {
    return Array.from(this.plugins.values())
  }

  /**
   * 获取活跃插件
   */
  getActivePlugins(): Plugin[] {
    return this.getAllPlugins().filter((p) => p.state === PluginState.ACTIVE)
  }
}

// 单例
export const pluginManager = new PluginManager()
export { PluginManager }
export default pluginManager
