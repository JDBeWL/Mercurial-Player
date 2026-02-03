/**
 * Mercurial Player - 前端主入口
 *
 * Copyright (C) 2026  JDBeWL
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

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

// 生产环境禁用右键菜单
if (import.meta.env.PROD) {
  document.addEventListener('contextmenu', (e) => {
    e.preventDefault()
    return false
  })
  logger.info('生产环境：已禁用右键菜单')
}

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
