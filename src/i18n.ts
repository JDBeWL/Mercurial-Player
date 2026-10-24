import { createI18n } from 'vue-i18n'
import type { Ref } from 'vue'

import zh from './locales/zh.json'
import en from './locales/en.json'

const messages = {
  en,
  zh,
}

const i18n = createI18n({
  legacy: false,
  locale: 'zh',
  fallbackLocale: 'en',
  messages,
})

// 注入翻译函数：未注入前错误提示回退为 i18n key 本身
import { setErrorHandlerTranslator } from './utils/errorHandler'
setErrorHandlerTranslator(i18n.global.t)

export default i18n

export const setLocale = (locale: string): void => {
  ;(i18n.global.locale as Ref<string>).value = locale
}

export const getCurrentLocale = (): string => {
  return (i18n.global.locale as Ref<string>).value
}
