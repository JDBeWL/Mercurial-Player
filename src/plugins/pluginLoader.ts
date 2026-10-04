/** 插件加载器:从文件系统读取 manifest,解析入口并交给 pluginManager 注册 */

import { invoke } from '@tauri-apps/api/core'
import logger from '../utils/logger'
import pluginManager from './pluginManager'
import {
  PluginPermission,
  type PluginAPI,
  type PluginInstance,
  type PluginManifest,
  type PluginMainFunction,
  type PluginPermissionType,
} from './pluginTypes'
import { validatePluginCode } from './pluginSandbox'
import { PluginWorkerHost } from './sandbox/workerSandboxHost'

const builtinPluginModules = import.meta.glob<{
  default: (api: PluginAPI) => Promise<PluginInstance> | PluginInstance
}>(['../../plugins/*/index.js', '../../plugins/*/index.ts'])

const builtinManifests = import.meta.glob<PluginManifest>('../../plugins/*/manifest.json', {
  eager: true,
  import: 'default',
})

const builtinPluginMap = new Map<string, (typeof builtinPluginModules)[string]>()
for (const [manifestPath, manifest] of Object.entries(builtinManifests)) {
  const pluginId = manifest?.id
  if (!pluginId) continue
  // 内置插件按目录配对:../../plugins/<id>/manifest.json 对应同目录的 index.js/index.ts
  const baseDir = manifestPath.replace(/manifest\.json$/, '')
  for (const modulePath of Object.keys(builtinPluginModules)) {
    if (modulePath.startsWith(baseDir)) {
      builtinPluginMap.set(pluginId, builtinPluginModules[modulePath]!)
      break
    }
  }
}

/**
 * 并发加载全部插件,单个插件故障不影响其余插件
 *
 * 各插件的 init/runMain/回调都有超时保护(见 workerSandboxHost.ts),一个插件挂起不会阻塞整批加载
 */
export async function loadAllPlugins(): Promise<void> {
  try {
    const pluginDirs = await invoke<string[]>('list_plugins')

    logger.info(`发现 ${pluginDirs.length} 个插件`)

    await Promise.allSettled(pluginDirs.map((pluginDir) => loadPlugin(pluginDir)))
  } catch (error) {
    logger.error('加载插件列表失败:', error)
  }
}

/** 校验 manifest 的必填字段,ID 字符集与版本号 x.y.z 格式;不合规即抛错 */
function validateManifest(manifest: PluginManifest): void {
  if (!manifest.id || typeof manifest.id !== 'string') {
    throw new Error('插件清单缺少有效的 id 字段')
  }

  if (!manifest.name || typeof manifest.name !== 'string') {
    throw new Error('插件清单缺少有效的 name 字段')
  }

  if (!/^[a-zA-Z0-9_-]+$/.test(manifest.id)) {
    throw new Error('插件ID只能包含字母、数字、连字符和下划线')
  }

  if (manifest.permissions) {
    const validPermissions = Object.values(PluginPermission)
    for (const permission of manifest.permissions) {
      if (!validPermissions.includes(permission as PluginPermissionType)) {
        throw new Error(`无效的权限: ${permission}`)
      }
    }
  }

  // 版本可选,提供了就必须是 x.y.z
  if (manifest.version && !/^\d+\.\d+\.\d+/.test(manifest.version)) {
    throw new Error('版本号格式无效，应为 x.y.z 格式')
  }
}

/** 读取 manifest 并注册单个插件;失败记录日志后抛出,隔离由 loadAllPlugins 保证 */
export async function loadPlugin(pluginPath: string): Promise<void> {
  try {
    const manifest = await invoke<PluginManifest | null>('read_plugin_manifest', {
      path: pluginPath,
    })

    if (!manifest) {
      throw new Error('无法读取插件清单')
    }

    validateManifest(manifest)

    if (pluginManager.plugins.has(manifest.id)) {
      logger.info(`插件 ${manifest.id} 已存在，正在重新加载`)
      try {
        await pluginManager.deactivate(manifest.id)
        await pluginManager.uninstall(manifest.id, false) // 重载不清插件存储
      } catch (error) {
        logger.warn(`卸载现有插件失败: ${manifest.id}`, error)
      }
    }

    let mainFn: PluginMainFunction
    let workerHost: PluginWorkerHost | undefined

    // 同 id 时内置(bundled)插件优先,忽略磁盘上的外置副本
    const builtinLoader = builtinPluginMap.get(manifest.id)
    if (builtinLoader) {
      logger.info(`加载内置插件 (bundled): ${manifest.id}`)
      const module = await builtinLoader()
      const pluginFactory = module.default
      mainFn = async (api: PluginAPI) => {
        return await pluginFactory(api)
      }
    } else {
      // 外置插件跑在独立 Dedicated Worker:无 DOM / localStorage / Tauri IPC,只能经 postMessage RPC 访问受权限控制的 PluginAPI
      // 这里建立的是与主窗口权限的物理隔离;内置插件不进 Worker,由 pluginSandbox 在主窗口执行
      const mainCode = await invoke<string>('read_plugin_main', {
        path: pluginPath,
        main: manifest.main || 'index.js',
      })

      // 静态检查只告警不阻断:正则黑名单可被等价变形绕过,阈值规则也会误伤正常插件
      // 安全边界由 Worker 沙箱 + CSP + 宿主侧 API 白名单承担,这里只有纵深观测价值
      try {
        validatePluginCode(mainCode)
      } catch (error) {
        logger.warn(
          `插件代码静态检查告警(不阻断加载,安全边界由 Worker 沙箱承担): ${manifest.id}`,
          error,
        )
      }

      const permissions = (manifest.permissions || []) as string[]
      workerHost = new PluginWorkerHost(manifest.id, mainCode, permissions)
      try {
        await workerHost.init()
      } catch (error) {
        workerHost.terminate()
        logger.error(`插件模块加载失败: ${manifest.id}`, error)
        throw error
      }

      mainFn = workerHost.createMainFunction() as PluginMainFunction
    }

    await pluginManager.register({
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      author: manifest.author,
      description: manifest.description,
      permissions: manifest.permissions || [],
      main: mainFn,
      workerHost,
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

/** 卸载插件:先释放运行态与存储映射,再让后端删除磁盘上的插件目录 */
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
