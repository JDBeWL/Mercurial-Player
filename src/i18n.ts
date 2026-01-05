import { createI18n } from 'vue-i18n'
import type { Ref } from 'vue'

// 导入翻译文件
import zh from './locales/zh.json'
import en from './locales/en.json'

// 定义支持的语言
const messages = {
  en,
  zh
}

// 创建i18n实例
const i18n = createI18n({
  legacy: false, // 使用Composition API模式
  locale: 'zh', // 默认语言设置为中文
  fallbackLocale: 'en', // 回退语言
  messages, // 翻译信息
})

// 导出i18n实例和设置语言的方法
export default i18n

// 导出设置语言的方法
export const setLocale = (locale: string): void => {
  (i18n.global.locale as Ref<string>).value = locale
}

// 导出获取当前语言的方法
export const getCurrentLocale = (): string => {
  return (i18n.global.locale as Ref<string>).value
}
