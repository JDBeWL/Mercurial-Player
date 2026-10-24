import { ref } from 'vue'

const STORAGE_KEY = 'mercurial-player.developer-mode'

// `Settings.vue` 与 `AboutSettings`/`DeveloperSettings` 共享这一模块级状态
const developerMode = ref(localStorage.getItem(STORAGE_KEY) === 'true')

/** 开发者模式只存 `localStorage`, 不进 `AppConfig`, 避免污染配置导入导出与迁移 */
export function useDeveloperMode() {
  const setDeveloperMode = (value: boolean): void => {
    developerMode.value = value
    if (value) {
      localStorage.setItem(STORAGE_KEY, 'true')
    } else {
      localStorage.removeItem(STORAGE_KEY)
    }
  }

  return { developerMode, setDeveloperMode }
}
