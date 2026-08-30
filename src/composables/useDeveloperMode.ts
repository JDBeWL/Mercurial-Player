import { ref } from 'vue'

const STORAGE_KEY = 'mercurial-player.developer-mode'

// Settings.vue(决定 tab 可见性)与 AboutSettings/DeveloperSettings 共享同一状态
const developerMode = ref(localStorage.getItem(STORAGE_KEY) === 'true')

/**
 * 开发者模式开关
 *
 * 面向调试场景,刻意不进入正式配置体系(AppConfig),
 * 仅持久化到 localStorage,避免污染配置导入/导出与迁移逻辑。
 */
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
