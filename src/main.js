import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './style.css'
import './assets/css/lyrics-modern.css'
import './assets/css/lyrics-classic.css'
import i18n from './i18n'
import logger from './utils/logger'
import { setupThemeContrastValidation } from './utils/themeContrastValidator'

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