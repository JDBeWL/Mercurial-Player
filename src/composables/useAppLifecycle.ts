import { onMounted, onUnmounted, type Ref, type WatchStopHandle } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { usePlayerStore } from '@/stores/player'
import { useThemeStore } from '@/stores/theme'
import { useConfigStore } from '@/stores/config'
import { setLocale } from '@/i18n'
import { pluginManager } from '@/plugins'
import logger from '@/utils/logger'

type ErrorSeverity = 'error' | 'warning' | 'info'

interface UseAppLifecycleOptions {
  // 自动更新相关
  checkForUpdates: () => Promise<void>
  updateAvailable: Ref<boolean>
  newVersion: Ref<string>
  // 错误通知
  showError: (message: string, severity: ErrorSeverity, duration?: number) => number
  // 错误通知取消订阅函数（用于卸载时清理 errorHandler 桥接监听器）
  unsubscribeErrorNotification: () => void
  // 窗口状态同步（在初始化完成后调用，确保 isFullscreen/isMaximized 与实际一致）
  syncWindowState: () => Promise<void>
  // 音轨监听器清理函数（由 useTrackInfo 返回）
  stopWatchTrack: WatchStopHandle | null
}

/**
 * 通过 requestAnimationFrame 测量屏幕刷新率（持续 1 秒）。
 * 用于动态调整后端 FFT 计算频率，避免在不必要的高频下浪费 CPU。
 */
function getScreenRefreshRate(): Promise<number> {
  return new Promise((resolve) => {
    let frames = 0
    const lastTime = performance.now()
    const duration = 1000 // 测量1秒

    const countFrame = () => {
      frames++
      const currentTime = performance.now()
      if (currentTime - lastTime < duration) {
        requestAnimationFrame(countFrame)
      } else {
        // 返回测得的刷新率，至少是1
        resolve(Math.max(1, frames))
      }
    }
    requestAnimationFrame(countFrame)
  })
}

/**
 * 应用生命周期 Composable
 *
 * 集中处理 App 启动初始化序列与卸载清理：
 *
 * onMounted 顺序：
 *   1. 注册 beforeunload（关闭前 flush 配置 + 清理播放器 + 清理插件）
 *   2. 加载配置文件（允许重置 UI 状态）
 *   3. 应用语言设置
 *   4. 应用主题设置
 *   5. 设置封面缓存路径到后端
 *   6. 初始化音频播放器（注册监听器、启动清理任务）
 *   7. 恢复上次播放会话
 *   8. 测量屏幕刷新率并同步到后端（用于 FFT 频率）
 *   9. 检查更新（若启用），通过错误通知容器提示用户
 *   10. 同步窗口全屏/最大化状态
 *
 * onUnmounted：flush 配置 → 移除 beforeunload → 取消错误通知桥接 → 停止音轨监听 → 清理播放器
 *
 * 注意：键盘监听器由 useGlobalKeyboard 自行注册/注销，不在此处管理。
 */
export function useAppLifecycle(options: UseAppLifecycleOptions): void {
  const playerStore = usePlayerStore()
  const configStore = useConfigStore()
  const themeStore = useThemeStore()
  const { t } = useI18n()

  // 应用关闭前强制保存配置并清理资源
  const handleBeforeUnload = async (): Promise<void> => {
    await configStore.flushPendingSave()
    // 清理播放器资源（包括全局快捷键）
    await playerStore.cleanup()
    // 清理插件管理器（停用所有插件、保存存储、清理沙箱）
    await pluginManager.cleanup()
  }

  onMounted(async () => {
    // 注册 beforeunload 事件，确保关闭前保存配置
    window.addEventListener('beforeunload', handleBeforeUnload)

    // 加载配置文件（启动时允许重置 UI 状态）
    try {
      await configStore.loadConfig(true)
    } catch (error) {
      logger.warn('Failed to load configuration:', error)
    }

    // 设置语言
    try {
      setLocale(configStore.general.language || 'zh')
    } catch (error) {
      logger.error('Failed to apply language from config:', error)
    }

    // 从配置加载主题设置
    try {
      const savedTheme = configStore.general.theme
      if (savedTheme) {
        themeStore.setThemePreference(savedTheme)
      }
    } catch (error) {
      logger.error('Failed to apply theme from config:', error)
    }

    // 应用主题
    themeStore.applyTheme()

    // 初始化封面缓存路径
    try {
      await invoke('set_cover_cache_path_command', {
        path: configStore.general.coverCachePath,
      })
    } catch (error) {
      logger.warn('Failed to set cover cache path:', error)
    }

    // 初始化音频播放器
    await playerStore.initAudio()

    // 尝试恢复上次播放会话 (启动时根据 last_session 校验并恢复)
    // 失败静默忽略 (用户选择不弹提示)
    try {
      await playerStore.resumeLastSession()
    } catch (error) {
      logger.warn('Failed to resume last session:', error)
    }

    // 获取屏幕刷新率并设置到后端，用于动态调整FFT计算频率
    try {
      const refreshRate = await getScreenRefreshRate()
      await invoke('set_target_fps', { fps: refreshRate })
      logger.info(`Screen refresh rate set to ${refreshRate}Hz`)
    } catch (error) {
      logger.warn('Failed to set target FPS:', error)
    }

    // 如果用户启用了自动检查更新，则在启动时执行一次检查（仅检查，不自动安装）
    try {
      if (configStore.general.enableAutoUpdate) {
        await options.checkForUpdates()
        if (options.updateAvailable.value) {
          // 通知用户有新版本可用（非阻塞信息通知）
          options.showError(
            `${t('config.updateAvailable')} v${options.newVersion.value}`,
            'info',
            10000,
          )
        }
      }
    } catch (err) {
      logger.warn('Auto update check failed:', err)
    }

    // 同步窗口全屏/最大化状态（按钮图标依赖此状态）
    await options.syncWindowState()
  })

  onUnmounted(async () => {
    // 强制保存待处理的配置
    await configStore.flushPendingSave()
    // 移除 beforeunload 事件监听器
    window.removeEventListener('beforeunload', handleBeforeUnload)
    // 清理错误通知监听器
    options.unsubscribeErrorNotification()
    // 清理音轨监听器
    if (options.stopWatchTrack) {
      options.stopWatchTrack()
    }
    // 清理播放器资源
    playerStore.cleanup()
  })
}
