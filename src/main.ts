import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './style.css'
import './assets/css/lyrics-modern.css'
import './assets/css/lyrics-classic.css'
import i18n from './i18n'
import logger from './utils/logger'
import { setupThemeContrastValidation } from './utils/themeContrastValidator'
import type { BuiltinPluginDefinition } from './plugins/pluginManager'

// 初始化日志系统
logger.info('应用程序启动中...')

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(i18n)
app.mount('#app')

logger.info('应用程序已启动')

// 设置主题对比度验证
setupThemeContrastValidation()

// 加载内置插件
import { pluginManager } from './plugins'
import builtinPlugins from './plugins/builtins'
import { loadAllPlugins } from './plugins/pluginLoader'
import { shortcutManager } from './plugins/shortcutManager'

const loadBuiltinPlugins = async (): Promise<void> => {
  // 先初始化插件管理器（设置播放器状态监听）
  await pluginManager.init()
  
  for (const plugin of builtinPlugins as BuiltinPluginDefinition[]) {
    try {
      if (!pluginManager.plugins.has(plugin.id)) {
        await pluginManager.register(plugin)
        await pluginManager.activate(plugin.id)
      }
    } catch (error) {
      logger.error(`加载内置插件失败: ${plugin.name}`, error)
    }
  }
  logger.info('内置插件加载完成')
  
  // 加载外部插件
  try {
    await loadAllPlugins()
    logger.info('外部插件加载完成')
  } catch (error) {
    logger.error('加载外部插件失败:', error)
  }
  
  // 启动快捷键管理器
  shortcutManager.start()
}

loadBuiltinPlugins()
