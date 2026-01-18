/**
 * 插件加载器
 * 负责从文件系统加载和解析插件
 */

import { invoke } from '@tauri-apps/api/core'
import logger from '../utils/logger'
import pluginManager, { type PluginAPI, type PluginPermissionType, PluginPermission } from './pluginManager'
import { validatePluginCode } from './pluginSandbox'

// 插件清单类型
interface PluginManifest {
  id: string
  name: string
  version?: string
  author?: string
  description?: string
  permissions?: PluginPermissionType[]
  main?: string
  autoActivate?: boolean
}

// 安装结果类型
interface InstallResult {
  success: boolean
  path?: string
  error?: string
}

/**
 * 加载所有插件
 */
export async function loadAllPlugins(): Promise<void> {
  try {
    const pluginDirs = await invoke<string[]>('list_plugins')
    
    logger.info(`发现 ${pluginDirs.length} 个插件`)

    for (const pluginDir of pluginDirs) {
      try {
        await loadPlugin(pluginDir)
      } catch (error) {
        logger.error(`加载插件失败: ${pluginDir}`, error)
      }
    }
  } catch (error) {
    logger.error('加载插件列表失败:', error)
  }
}

/**
 * 验证插件清单
 */
function validateManifest(manifest: PluginManifest): void {
  if (!manifest.id || typeof manifest.id !== 'string') {
    throw new Error('插件清单缺少有效的 id 字段')
  }
  
  if (!manifest.name || typeof manifest.name !== 'string') {
    throw new Error('插件清单缺少有效的 name 字段')
  }
  
  // 验证ID格式（只允许字母、数字、连字符、下划线）
  if (!/^[a-zA-Z0-9_-]+$/.test(manifest.id)) {
    throw new Error('插件ID只能包含字母、数字、连字符和下划线')
  }
  
  // 验证权限
  if (manifest.permissions) {
    const validPermissions = Object.values(PluginPermission)
    for (const permission of manifest.permissions) {
      if (!validPermissions.includes(permission as PluginPermissionType)) {
        throw new Error(`无效的权限: ${permission}`)
      }
    }
  }
  
  // 验证版本格式（如果提供）
  if (manifest.version && !/^\d+\.\d+\.\d+/.test(manifest.version)) {
    throw new Error('版本号格式无效，应为 x.y.z 格式')
  }
}

/**
 * 加载单个插件
 */
export async function loadPlugin(pluginPath: string): Promise<void> {
  try {
    const manifest = await invoke<PluginManifest | null>('read_plugin_manifest', { path: pluginPath })
    
    if (!manifest) {
      throw new Error('无法读取插件清单')
    }

    // 验证清单
    validateManifest(manifest)

    // 如果插件已存在，先卸载它
    if (pluginManager.plugins.has(manifest.id)) {
      logger.info(`插件 ${manifest.id} 已存在，正在重新加载`)
      try {
        await pluginManager.deactivate(manifest.id)
        await pluginManager.uninstall(manifest.id, false) // 不清除存储
      } catch (error) {
        logger.warn(`卸载现有插件失败: ${manifest.id}`, error)
      }
    }

    const mainCode = await invoke<string>('read_plugin_main', { 
      path: pluginPath, 
      main: manifest.main || 'index.js' 
    })

    try {
      validatePluginCode(mainCode)
    } catch (error) {
      logger.error(`插件代码安全检查失败: ${manifest.id}`, error)
      throw new Error(`插件安全检查失败: ${(error as Error).message}`)
    }

    const mainFn = createPluginFunction(mainCode, manifest.id)

    await pluginManager.register({
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      author: manifest.author,
      description: manifest.description,
      permissions: manifest.permissions || [],
      main: mainFn,
    })

    if (manifest.autoActivate !== false) {
      await pluginManager.activate(manifest.id)
    }

    logger.info(`插件加载成功: ${manifest.name}`)
  } catch (error) {
    logger.error(`插件加载失败: ${pluginPath}`, error)
    throw error
  }
}

// 安全参数类型
interface SafeParams {
  [key: string]: unknown
  console: {
    log: (...args: unknown[]) => void
    info: (...args: unknown[]) => void
    warn: (...args: unknown[]) => void
    error: (...args: unknown[]) => void
    debug: (...args: unknown[]) => void
  }
  setTimeout: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearTimeout: (id: number) => void
  setInterval: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearInterval: (id: number) => void
  api: PluginAPI
}

/**
 * 创建插件函数
 */
function createPluginFunction(code: string, pluginId: string) {
  return async (api: PluginAPI) => {
    try {
      const safeConsole = {
        log: api.log.info,
        info: api.log.info,
        warn: api.log.warn,
        error: api.log.error,
        debug: api.log.debug,
      }

      const timers = new Set<number>()
      const intervals = new Set<number>()

      const safeSetTimeout = (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]): number => {
        const id = setTimeout(() => {
          timers.delete(id as unknown as number)
          try { fn(...args) } catch (e) { api.log.error('定时器错误:', e) }
        }, Math.min(delay || 0, 60000)) as unknown as number
        timers.add(id)
        return id
      }

      const safeClearTimeout = (id: number): void => {
        timers.delete(id)
        clearTimeout(id)
      }

      const safeSetInterval = (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]): number => {
        const safeDelay = Math.max(delay || 100, 100)
        const id = setInterval(() => {
          try { fn(...args) } catch (e) { api.log.error('定时器错误:', e) }
        }, safeDelay) as unknown as number
        intervals.add(id)
        return id
      }

      const safeClearInterval = (id: number): void => {
        intervals.delete(id)
        clearInterval(id)
      }

      const safeParams: SafeParams = {
        Object, Array, String, Number, Boolean, Date, RegExp,
        Error, TypeError, RangeError, SyntaxError,
        Map, Set, WeakMap, WeakSet,
        JSON: Object.freeze({ parse: JSON.parse, stringify: JSON.stringify }),
        Math,
        Promise,
        encodeURIComponent, decodeURIComponent, encodeURI, decodeURI,
        btoa, atob,
        isNaN, isFinite, parseInt, parseFloat,
        NaN, Infinity,
        console: safeConsole,
        setTimeout: safeSetTimeout,
        clearTimeout: safeClearTimeout,
        setInterval: safeSetInterval,
        clearInterval: safeClearInterval,
        api,
        window: undefined,
        document: undefined,
        globalThis: undefined,
        self: undefined,
        top: undefined,
        parent: undefined,
        frames: undefined,
        eval: undefined,
        Function: undefined,
        process: undefined,
        require: undefined,
        module: undefined,
        exports: undefined,
        __dirname: undefined,
        __filename: undefined,
        XMLHttpRequest: undefined,
        fetch: undefined,
        WebSocket: undefined,
        Worker: undefined,
        SharedWorker: undefined,
        localStorage: undefined,
        sessionStorage: undefined,
        indexedDB: undefined,
        navigator: undefined,
        location: undefined,
        history: undefined,
        alert: undefined,
        confirm: undefined,
        prompt: undefined,
        open: undefined,
        close: undefined,
        Proxy: undefined,
        Reflect: undefined,
        importScripts: undefined,
      }

      const paramNames = Object.keys(safeParams)
      const paramValues = Object.values(safeParams)

      const wrappedCode = `
        ${code}
        return typeof plugin !== 'undefined' ? plugin : {};
      `

      // eslint-disable-next-line @typescript-eslint/no-implied-eval
      const fn = new Function(...paramNames, wrappedCode)
      const result = fn(...paramValues)

      if (result && typeof result.then === 'function') {
        return await result
      }
      return result
    } catch (error) {
      logger.error(`插件执行失败: ${pluginId}`, error)
      throw error
    }
  }
}

/**
 * 安装插件
 */
export async function installPlugin(source: string): Promise<InstallResult> {
  try {
    const result = await invoke<InstallResult>('install_plugin', { source })
    if (result.success && result.path) {
      await loadPlugin(result.path)
      return result
    }
    throw new Error(result.error || '安装失败')
  } catch (error) {
    logger.error('安装插件失败:', error)
    throw error
  }
}

/**
 * 卸载插件
 */
export async function uninstallPlugin(pluginId: string): Promise<void> {
  try {
    await pluginManager.uninstall(pluginId)
    await invoke('uninstall_plugin', { pluginId })
    logger.info(`插件已卸载: ${pluginId}`)
  } catch (error) {
    logger.error('卸载插件失败:', error)
    throw error
  }
}
