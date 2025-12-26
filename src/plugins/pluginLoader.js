/**
 * 插件加载器
 * 负责从文件系统加载和解析插件
 */

import { invoke } from '@tauri-apps/api/core'
import logger from '../utils/logger'
import pluginManager from './pluginManager'
import { validatePluginCode } from './pluginSandbox'

// 插件目录
const PLUGINS_DIR = 'plugins'

/**
 * 加载所有插件
 */
export async function loadAllPlugins() {
  try {
    // 获取插件目录列表
    const pluginDirs = await invoke('list_plugins')
    
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
 * 加载单个插件
 * @param {string} pluginPath 插件路径
 */
export async function loadPlugin(pluginPath) {
  try {
    // 读取插件清单
    const manifest = await invoke('read_plugin_manifest', { path: pluginPath })
    
    if (!manifest) {
      throw new Error('无法读取插件清单')
    }

    // 读取插件主文件
    const mainCode = await invoke('read_plugin_main', { 
      path: pluginPath, 
      main: manifest.main || 'index.js' 
    })

    // 验证插件代码安全性
    try {
      validatePluginCode(mainCode)
    } catch (error) {
      logger.error(`插件代码安全检查失败: ${manifest.id}`, error)
      throw new Error(`插件安全检查失败: ${error.message}`)
    }

    // 创建插件主函数
    const mainFn = createPluginFunction(mainCode, manifest.id)

    // 注册插件
    await pluginManager.register({
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      author: manifest.author,
      description: manifest.description,
      permissions: manifest.permissions || [],
      main: mainFn,
    })

    // 如果插件配置为自动激活
    if (manifest.autoActivate !== false) {
      await pluginManager.activate(manifest.id)
    }

    logger.info(`插件加载成功: ${manifest.name}`)
  } catch (error) {
    logger.error(`插件加载失败: ${pluginPath}`, error)
    throw error
  }
}

/**
 * 创建插件函数
 * 插件代码在受限的沙箱环境中执行
 * @param {string} code 插件代码
 * @param {string} pluginId 插件 ID
 */
function createPluginFunction(code, pluginId) {
  // 返回一个接收 api 参数的函数
  return async (api) => {
    try {
      // 创建安全的 console
      const safeConsole = {
        log: api.log.info,
        info: api.log.info,
        warn: api.log.warn,
        error: api.log.error,
        debug: api.log.debug,
      }

      // 创建安全的定时器
      const timers = new Set()
      const intervals = new Set()

      const safeSetTimeout = (fn, delay, ...args) => {
        const id = setTimeout(() => {
          timers.delete(id)
          try { fn(...args) } catch (e) { api.log.error('定时器错误:', e) }
        }, Math.min(delay || 0, 60000))
        timers.add(id)
        return id
      }

      const safeClearTimeout = (id) => {
        timers.delete(id)
        clearTimeout(id)
      }

      const safeSetInterval = (fn, delay, ...args) => {
        const safeDelay = Math.max(delay || 100, 100)
        const id = setInterval(() => {
          try { fn(...args) } catch (e) { api.log.error('定时器错误:', e) }
        }, safeDelay)
        intervals.add(id)
        return id
      }

      const safeClearInterval = (id) => {
        intervals.delete(id)
        clearInterval(id)
      }

      // 构建参数名和值列表
      // 通过显式传递参数来覆盖全局变量
      const safeParams = {
        // 安全的全局对象
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
        // 安全的定时器
        console: safeConsole,
        setTimeout: safeSetTimeout,
        clearTimeout: safeClearTimeout,
        setInterval: safeSetInterval,
        clearInterval: safeClearInterval,
        // 插件 API
        api,
        // 阻止危险的全局对象（设为 undefined）
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

      // 包装插件代码
      const wrappedCode = `
        ${code}
        return typeof plugin !== 'undefined' ? plugin : {};
      `

      // 使用 Function 构造函数，通过参数传递安全的全局对象
      const fn = new Function(...paramNames, wrappedCode)
      const result = fn(...paramValues)

      // 如果返回的是 Promise，等待它
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
 * 安装插件（从 URL 或本地文件）
 * @param {string} source 插件源
 */
export async function installPlugin(source) {
  try {
    const result = await invoke('install_plugin', { source })
    if (result.success) {
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
 * @param {string} pluginId 插件 ID
 */
export async function uninstallPlugin(pluginId) {
  try {
    await pluginManager.uninstall(pluginId)
    await invoke('uninstall_plugin', { pluginId })
    logger.info(`插件已卸载: ${pluginId}`)
  } catch (error) {
    logger.error('卸载插件失败:', error)
    throw error
  }
}
