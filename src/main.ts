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
// Roboto 字体自托管 ,仅导入使用到的权重
import '@fontsource/roboto/300.css'
import '@fontsource/roboto/400.css'
import '@fontsource/roboto/500.css'
import '@fontsource/roboto/700.css'
// 歌词可选字体：霞鹜文楷屏幕版（unicode-range 分片按需加载）
import 'lxgw-wenkai-screen-webfont/lxgwwenkaiscreen.css'
// 注册 assets/fonts/lyrics/ 下手动引入的打包字体
import './utils/bundledFonts'
import { loadExternalFonts } from './utils/bundledFonts'
import './style.css'
import i18n from './i18n'
import logger from './utils/logger'
import { setupThemeContrastValidation } from './utils/themeContrastValidator'
import type { BuiltinPluginDefinition } from './plugins/pluginManager'

// 初始化日志系统
logger.info('应用程序启动中...')

const app = createApp(App)
const pinia = createPinia()

// 全局错误兜底:生产构建会 drop console,未捕获异常若不落盘则完全不可诊断
// Vue 组件渲染 / watcher / 生命周期钩子中的同步异常统一进入此处理器
app.config.errorHandler = (err, _instance, info) => {
  logger.error(`[全局错误] ${info}:`, err)
}

// 未处理的 Promise 拒绝 (fire-and-forget 的 async 调用等)
window.addEventListener('unhandledrejection', (event) => {
  logger.error('[未处理的 Promise 拒绝]:', event.reason)
})

app.use(pinia)
app.use(i18n)
app.mount('#app')

// 扫描软件同级 fonts/ 目录，注册外部字体（应用运行中放入的字体在打开设置页时还会重新扫描）
void loadExternalFonts()

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

// 按需加载歌词样式 CSS:
// 根据用户配置 (modern/classic) 只加载对应的一种样式,
// 避免两种样式都被解析并常驻内存。
// 注意: 此处需要 pinia 已初始化,故放在 app.mount 之后。
import { useConfigStore } from './stores/config'
async function loadLyricsStyleCss(): Promise<void> {
  try {
    const configStore = useConfigStore()
    const style = configStore.lyrics?.lyricsStyle || 'modern'
    if (style === 'classic') {
      await import('./assets/css/lyrics-classic.css')
    } else {
      await import('./assets/css/lyrics-modern.css')
    }
  } catch (e) {
    logger.error('加载歌词样式 CSS 失败:', e)
  }
}
loadLyricsStyleCss()

// 加载内置插件
import { pluginManager } from './plugins'
import builtinPlugins from './plugins/builtins'
import { loadAllPlugins } from './plugins/pluginLoader'
import { shortcutManager } from './plugins/shortcutManager'

const loadBuiltinPlugins = async (): Promise<void> => {
  // 先初始化插件管理器（设置播放器状态监听）
  try {
    await pluginManager.init()
  } catch (error) {
    logger.error('插件管理器初始化失败:', error)
  }

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

loadBuiltinPlugins().catch((error) => {
  logger.error('插件加载流程异常终止:', error)
})
