/**
 * 插件系统入口
 */

export { pluginManager, PluginState, PluginPermission } from './pluginManager'
export { createPluginAPI } from './pluginAPI'
export { createPluginSandbox, validatePluginCode } from './pluginSandbox'
export { loadAllPlugins, loadPlugin, installPlugin, uninstallPlugin } from './pluginLoader'

// 内置插件
export { default as builtinPlugins } from './builtins'
