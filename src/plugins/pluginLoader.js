/**
 * 插件加载器
 * 负责从文件系统加载和解析插件
 */

import { invoke } from '@tauri-apps/api/core'
import logger from '../utils/logger'
import pluginManager from './pluginManager'

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
 * @param {string} code 插件代码
 * @param {string} pluginId 插件 ID
 */
function createPluginFunction(code, pluginId) {
  // 包装插件代码，注入 API
  return async (api) => {
    try {
      // 使用 AsyncFunction 创建异步函数
      const AsyncFunction = Object.getPrototypeOf(async function(){}).constructor
      const wrappedCode = `
        return (async () => {
          ${code}
          return typeof plugin !== 'undefined' ? plugin : {};
        })();
      `
      const fn = new AsyncFunction('api', wrappedCode)
      return await fn(api)
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
