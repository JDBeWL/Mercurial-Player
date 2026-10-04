/** 插件系统入口:统一 re-export 管理器,API,沙箱,加载器与内置插件 */

export { pluginManager, PluginState, PluginPermission } from './pluginManager'
export { createPluginAPI } from './pluginAPI'
export { createPluginSandbox, validatePluginCode } from './pluginSandbox'
export { loadAllPlugins, loadPlugin, uninstallPlugin } from './pluginLoader'

export { default as builtinPlugins } from './builtins'
