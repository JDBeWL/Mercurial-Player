/**
 * 插件加载器
 * 负责从文件系统加载和解析插件
 */

import { invoke } from '@tauri-apps/api/core'
import logger from '../utils/logger'
import pluginManager from './pluginManager'
import {
  PluginPermission,
  type PluginAPI,
  type PluginInstance,
  type PluginMainFunction,
  type PluginManifest,
  type PluginPermissionType,
} from './pluginTypes'
import { validatePluginCode, createSafeConsole, createSafeTimers } from './pluginSandbox'

const builtinPluginModules = import.meta.glob<{
  default: (api: PluginAPI) => Promise<PluginInstance> | PluginInstance
}>(['../../plugins/*/index.js', '../../plugins/*/index.ts'])

const builtinManifests = import.meta.glob<PluginManifest>('../../plugins/*/manifest.json', {
  eager: true,
  import: 'default',
})

// 构建 id -> 模块加载器 的映射
const builtinPluginMap = new Map<string, (typeof builtinPluginModules)[string]>()
for (const [manifestPath, manifest] of Object.entries(builtinManifests)) {
  const pluginId = manifest?.id
  if (!pluginId) continue
  // manifest 路径: ../../plugins/lyrics-share/manifest.json
  // 对应的模块路径: ../../plugins/lyrics-share/index.js (或 .ts)
  const baseDir = manifestPath.replace(/manifest\.json$/, '')
  for (const modulePath of Object.keys(builtinPluginModules)) {
    if (modulePath.startsWith(baseDir)) {
      builtinPluginMap.set(pluginId, builtinPluginModules[modulePath])
      break
    }
  }
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
    const manifest = await invoke<PluginManifest | null>('read_plugin_manifest', {
      path: pluginPath,
    })

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

    let mainFn: PluginMainFunction

    // 优先使用内置插件
    const builtinLoader = builtinPluginMap.get(manifest.id)
    if (builtinLoader) {
      logger.info(`加载内置插件 (bundled): ${manifest.id}`)
      const module = await builtinLoader()
      const pluginFactory = module.default
      mainFn = async (api: PluginAPI) => {
        return await pluginFactory(api)
      }
    } else {
      // 外置插件
      // 注意：production 环境 CSP 不允许 new Function，运行时安装的插件需改为 ES 模块格式
      const mainCode = await invoke<string>('read_plugin_main', {
        path: pluginPath,
        main: manifest.main || 'index.js',
      })

      try {
        validatePluginCode(mainCode)
      } catch (error) {
        logger.error(`插件代码安全检查失败: ${manifest.id}`, error)
        throw new Error(`插件安全检查失败: ${(error as Error).message}`)
      }

      mainFn = createPluginFunction(mainCode, manifest.id)
    }

    await pluginManager.register({
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      author: manifest.author,
      description: manifest.description,
      permissions: manifest.permissions || [],
      main: mainFn,
    })

    if (manifest.auto_activate !== false) {
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
      const safeConsole = createSafeConsole(api.log)

      // 安全的定时器（带清理追踪）
      const safeTimers = createSafeTimers((e) => api.log.error('定时器错误:', e))

      const safeParams: SafeParams = {
        Object,
        Array,
        String,
        Number,
        Boolean,
        Date,
        RegExp,
        Error,
        TypeError,
        RangeError,
        SyntaxError,
        Map,
        Set,
        WeakMap,
        WeakSet,
        JSON: Object.freeze({ parse: JSON.parse, stringify: JSON.stringify }),
        Math,
        Promise,
        encodeURIComponent,
        decodeURIComponent,
        encodeURI,
        decodeURI,
        btoa,
        atob,
        isNaN,
        isFinite,
        parseInt,
        parseFloat,
        NaN,
        Infinity,
        console: safeConsole,
        setTimeout: safeTimers.setTimeout,
        clearTimeout: safeTimers.clearTimeout,
        setInterval: safeTimers.setInterval,
        clearInterval: safeTimers.clearInterval,
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
