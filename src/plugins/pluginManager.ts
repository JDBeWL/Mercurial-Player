/** 插件生命周期管理器:注册/激活/停用/卸载,并持有扩展注册表,事件总线与插件存储 */

import { reactive, markRaw, watch, type WatchStopHandle } from 'vue'
import logger from '../utils/logger'
import { createPluginAPI } from './pluginAPI'
import { createPluginSandbox, type PluginSandbox } from './pluginSandbox'
import type { PluginWorkerHost } from './sandbox/workerSandboxHost'
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

// 纯类型契约在 pluginTypes.ts;此处 re-export 保持既有 `from './pluginManager'` 的 import 路径可用
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

interface PluginInstanceData {
  instance: PluginInstance
  api: PluginAPI
  sandbox: PluginSandbox
}

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

interface EventListener {
  pluginId: string
  callback: EventCallback
}

class PluginManager {
  plugins: Map<string, Plugin>
  private instances: Map<string, PluginInstanceData>
  extensions: Extensions
  private eventListeners: Map<string, EventListener[]>
  private storage: Map<string, PluginPersistentStorage>
  private _playerWatcherStop: WatchStopHandle | null
  // 外置插件的 Worker 宿主:停用即 terminate,卸载时移除引用
  private workerHosts: Map<string, PluginWorkerHost>
  // 每个插件的激活代数:deactivate/uninstall 递增它,作废在途的 activate 结果(提交点校验见 activate)
  private activationGens: Map<string, number>

  constructor() {
    this.plugins = reactive(new Map()) as Map<string, Plugin>
    this.instances = new Map()
    this.activationGens = new Map()
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
    this.workerHosts = new Map()
  }

  /** 安装播放器状态 watcher,把曲目/播放态变化转发为插件事件 */
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
   * 释放管理器占用的全部资源(应用关闭路径调用)
   *
   * 单个插件 deactivate 抛错不影响其余插件,存储落盘始终执行
   */
  async cleanup(): Promise<void> {
    if (this._playerWatcherStop) {
      this._playerWatcherStop()
      this._playerWatcherStop = null
    }

    // 顺序停用避免并发资源竞争;先快照 id,防止迭代时修改 Map
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

    // deactivate 可能漏清,这里兜底
    this.eventListeners.clear()

    // 覆盖 deactivate 未处理到的存储
    for (const [pluginId, storage] of this.storage) {
      try {
        void storage.flush()
      } catch (e) {
        logger.warn(`强制保存插件 ${pluginId} 存储失败:`, e)
      }
    }
  }

  /** 登记插件元数据并置为 INACTIVE;同 id 重复注册抛错 */
  async register(pluginDef: PluginDefinition | BuiltinPluginDefinition): Promise<Plugin> {
    const { id, name, version, author, description, permissions = [], main } = pluginDef

    if (!id || !name || !main) {
      throw new Error('插件必须包含 id, name 和 main')
    }

    if (this.plugins.has(id)) {
      throw new Error(`插件 ${id} 已存在`)
    }

    const workerHost = (pluginDef as PluginDefinition).workerHost
    if (workerHost) {
      this.workerHosts.set(id, workerHost)
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

  /** 创建该插件的 API 与沙箱,执行 main 并置为 ACTIVE;已 ACTIVE 时为空操作 */
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
    // 取本次激活的代数快照,提交前据此作废(机制见 activationGens 字段)
    const generation = (this.activationGens.get(pluginId) ?? 0) + 1
    this.activationGens.set(pluginId, generation)

    try {
      const api = createPluginAPI(pluginId, plugin.permissions, this)

      // 外置插件走 Worker 宿主;内置插件是受信任代码,直接在主窗口执行(隔离原因见 pluginLoader)
      const workerHost = this.workerHosts.get(pluginId)
      let sandbox: PluginSandbox
      let instance: PluginInstance
      if (workerHost) {
        sandbox = workerHost.getSandboxAdapter()
        instance = await workerHost.runMain(api)
      } else {
        sandbox = createPluginSandbox(api)
        instance = await sandbox.execute(plugin.main)
      }

      if (instance && typeof instance.activate === 'function') {
        await sandbox.execute(() => instance.activate!())
      }

      // 内置插件的 await 打不断(只有 Worker 能被 terminate),只能在提交点验收:
      // 代数已变说明激活期间被停用过,这份实例登记下去就是带活定时器的僵尸插件
      if (this.activationGens.get(pluginId) !== generation) {
        logger.warn(`插件 ${pluginId} 在激活期间已被停用，丢弃这次激活结果`)
        try {
          if (instance && typeof instance.deactivate === 'function') {
            await sandbox.execute(() => instance.deactivate!())
          }
        } catch (deactivateError) {
          logger.warn(`丢弃插件实例时 deactivate 失败: ${pluginId}`, deactivateError)
        }
        sandbox.cleanup()
        this.cleanupPluginExtensions(pluginId)
        return
      }

      this.instances.set(pluginId, { instance, api, sandbox })
      plugin.state = PluginState.ACTIVE
      plugin.error = null

      logger.info(`插件已激活: ${plugin.name}`)
      this.emit('plugin:activated', { pluginId, plugin })
    } catch (error) {
      // 激活失败时终止 Worker 避免泄漏 (下次激活会由宿主自动重建)
      const workerHost = this.workerHosts.get(pluginId)
      if (workerHost) {
        try {
          workerHost.terminate()
        } catch (terminateError) {
          logger.warn(`终止沙箱宿主失败: ${pluginId}`, terminateError)
        }
      }
      plugin.state = PluginState.ERROR
      plugin.error = error instanceof Error ? error.message : String(error)
      logger.error(`插件激活失败: ${plugin.name}`, error)
      throw error
    }
  }

  /** 停用插件:执行插件自身 deactivate,回收扩展与沙箱,落盘存储;非 ACTIVE 状态直接返回 */
  async deactivate(pluginId: string): Promise<void> {
    const plugin = this.plugins.get(pluginId)
    if (!plugin) return

    if (plugin.state === PluginState.LOADING) {
      // 递增代数作废在途激活(验收分支见 activate)
      this.activationGens.set(pluginId, (this.activationGens.get(pluginId) ?? 0) + 1)
      // 走到这里说明激活挂起(init/runMain 超时或死循环),强制 terminate;
      // terminate 会让在途 activate 的 catch 置 ERROR,这里同步置一次保证返回后状态立即可用
      const workerHost = this.workerHosts.get(pluginId)
      if (workerHost) {
        try {
          workerHost.terminate()
        } catch (error) {
          logger.warn(`强制终止沙箱宿主失败: ${pluginId}`, error)
        }
      }
      this.cleanupPluginExtensions(pluginId)
      plugin.state = PluginState.ERROR
      plugin.error = '插件激活挂起，已被强制终止'
      logger.warn(`插件 ${pluginId} 在加载中被强制终止`)
      return
    }

    if (plugin.state === PluginState.UNLOADING) {
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
        // 即使插件 deactivate 抛错,沙箱清理也必须执行
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

    // 停用即落盘:flush 会取消防抖并立即写入
    if (this.storage.has(pluginId)) {
      try {
        void this.storage.get(pluginId)!.flush()
      } catch (e) {
        logger.warn(`插件停用时保存存储失败: ${pluginId}`, e)
      }
    }

    plugin.state = PluginState.INACTIVE
    logger.info(`插件已停用: ${plugin.name}`)
    this.emit('plugin:deactivated', { pluginId, plugin })
  }

  /** 卸载插件:先 deactivate,再释放引用;clearStorage 默认 false,即保留插件存储 */
  async uninstall(pluginId: string, clearStorage = false): Promise<void> {
    await this.deactivate(pluginId)
    // deactivate 已 terminate Worker,这里只释放宿主引用
    this.workerHosts.delete(pluginId)
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

  /** 摘除该插件在所有扩展表与事件监听表中的条目 */
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

  /** 登记扩展并绑定 pluginId,卸载时才能按插件回收 */
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

  /** 取某类扩展的当前列表,返回的是内部响应式数组引用 */
  getExtensions<K extends keyof Extensions>(type: K): Extensions[K] {
    return this.extensions[type] || []
  }

  /** 登记事件监听;插件侧的白名单与权限校验在 pluginAPI.events.on */
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
          // 回调多返回 Promise,同步 catch 捕不到拒绝;包一层 Promise.resolve 挂 .catch,避免 unhandledrejection
          void Promise.resolve(callback(data)).catch((error) => {
            logger.error(`事件处理出错: ${event}`, error)
          })
        } catch (error) {
          logger.error(`事件处理出错: ${event}`, error)
        }
      }
    }
  }

  /** 按插件惰性创建持久化存储;1MB 限额,紧急清理与防抖规则见 pluginStorage.ts */
  getStorage(pluginId: string): PluginPersistentStorage {
    if (!this.storage.has(pluginId)) {
      this.storage.set(pluginId, createPluginStorage(pluginId))
    }
    return this.storage.get(pluginId)!
  }

  getAllPlugins(): Plugin[] {
    return Array.from(this.plugins.values())
  }

  getActivePlugins(): Plugin[] {
    return this.getAllPlugins().filter((p) => p.state === PluginState.ACTIVE)
  }
}

export const pluginManager = new PluginManager()
export { PluginManager }
export default pluginManager
