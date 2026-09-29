import { onMounted, onUnmounted, watch, type Ref, type WatchStopHandle } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { usePlayerStore } from '@/stores/player'
import { useThemeStore } from '@/stores/theme'
import { useConfigStore } from '@/stores/config'
import { setLocale } from '@/i18n'
import { applyAppFontScale, applyLyricsFontScale } from '@/composables/useAppFontScale'
import { pluginManager } from '@/plugins'
import logger from '@/utils/logger'
import { applyVisualizerFps } from '@/utils/visualizerFps'
import { getCurrentWindow } from '@tauri-apps/api/window'

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

/** 应用生命周期 Composable：集中处理 App 的启动初始化序列与卸载清理。onUnmounted 要 flush 配置、
 *  移除 beforeunload、取消错误通知桥接、停止音轨监听、清理播放器。
 *  注意：键盘监听由 useGlobalKeyboard 自行注册/注销，不在此处管理。 */
export function useAppLifecycle(options: UseAppLifecycleOptions): void {
  const playerStore = usePlayerStore()
  const configStore = useConfigStore()
  const themeStore = useThemeStore()
  const { t } = useI18n()

  // 应用关闭前强制保存配置并清理资源。注意 beforeunload 中 WebView 不保证等待异步 IPC 完成，
  // 因此真实关闭路径走 onCloseRequested（见下），此处仅作兜底：fire-and-forget 发出 IPC，
  // 消息一旦发出后端即会处理。
  const flushResourcesOnClose = (): void => {
    void configStore.flushPendingSave()
    // 清理播放器资源（包括全局快捷键）
    void playerStore.cleanup()
    // 清理插件管理器（停用所有插件、保存存储、清理沙箱）
    void pluginManager.cleanup()
  }

  const handleBeforeUnload = (): void => {
    flushResourcesOnClose()
  }

  // 窗口移动监听的取消函数与防抖定时器（onUnmounted 清理）
  let unlistenWindowMove: (() => void) | null = null
  let unlistenCloseRequested: (() => void) | null = null
  let moveDebounceTimer: ReturnType<typeof setTimeout> | null = null
  // 歌词字号 → 根元素 CSS 变量的 watch（在 onMounted 里才建立，故类型可空）
  let lyricsFontScaleWatch: WatchStopHandle | null = null
  // 防止 CloseRequested 清理期间用户再次触发关闭
  let isClosing = false

  onMounted(async () => {
    // 注册 beforeunload 事件，确保关闭前保存配置（兜底路径,见 flushResourcesOnClose 注释）
    window.addEventListener('beforeunload', handleBeforeUnload)

    // 主关闭路径：拦截关闭请求，等异步清理（配置 flush / 播放器 / 插件）完成后再销毁窗口，
    // 确保数据落盘 —— beforeunload 的局限见 flushResourcesOnClose 注释
    try {
      unlistenCloseRequested = await getCurrentWindow().onCloseRequested(async (event) => {
        if (isClosing) {
          event.preventDefault()
          return
        }
        isClosing = true
        event.preventDefault()
        try {
          await configStore.flushPendingSave()
          await playerStore.cleanup()
          await pluginManager.cleanup()
        } catch (error) {
          logger.error('Failed to flush resources on close:', error)
        } finally {
          // 清理完成后真正关闭窗口 (destroy 不再触发 CloseRequested)
          await getCurrentWindow().destroy()
        }
      })
    } catch (error) {
      logger.warn('Failed to listen close requested:', error)
    }

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

    // 界面字号：Android 上原生侧会抵消 WebView 对系统字号的放大（见 applyAppFontScale），
    // 这里在配置就绪后对齐一次；之后改配置由设置页在滑块松手时下发
    await applyAppFontScale(configStore.ui?.fontScale ?? 1)

    // 歌词字号：写成根元素上的 CSS 变量，主歌词面板与可视化面板的单行歌词共用。
    // 放在配置加载之后 watch，immediate 那一次拿到的就是已加载的值。
    lyricsFontScaleWatch = watch(() => configStore.lyrics?.fontScale ?? 1, applyLyricsFontScale, {
      immediate: true,
    })

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

    // 初始化封面缓存路径（Android 已由后端固定到应用沙箱，前端不覆盖）
    try {
      const platform = await invoke<string>('get_platform')
      if (platform !== 'android') {
        await invoke('set_cover_cache_path_command', {
          path: configStore.general.coverCachePath,
        })
      }
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

    // 同步可视化目标帧率到后端（用于 FFT 计算频率）。
    // 与设置页共用逻辑：限制开启时取 min(目标帧率, 实时屏幕刷新率)。
    try {
      const result = await applyVisualizerFps(configStore.visualizer)
      if (result) {
        logger.info(`Target FPS set to ${result.fps}`)
      }
    } catch (error) {
      logger.warn('Failed to set target FPS:', error)
    }

    // 监听窗口移动：窗口跨屏后所在显示器可能变化，重新应用帧率限制。
    // onMoved 拖动期间高频触发，防抖后再查询。
    try {
      unlistenWindowMove = await getCurrentWindow().onMoved(() => {
        if (moveDebounceTimer) {
          clearTimeout(moveDebounceTimer)
        }
        moveDebounceTimer = setTimeout(() => {
          moveDebounceTimer = null
          if (configStore.visualizer?.enableVerticalSync) {
            applyVisualizerFps(configStore.visualizer).catch((error) => {
              logger.warn('Failed to re-apply target FPS after window move:', error)
            })
          }
        }, 500)
      })
    } catch (error) {
      logger.warn('Failed to listen window move:', error)
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
    // 清理窗口移动监听
    if (moveDebounceTimer) {
      clearTimeout(moveDebounceTimer)
      moveDebounceTimer = null
    }
    if (unlistenWindowMove) {
      unlistenWindowMove()
      unlistenWindowMove = null
    }
    if (unlistenCloseRequested) {
      unlistenCloseRequested()
      unlistenCloseRequested = null
    }
    if (lyricsFontScaleWatch) {
      lyricsFontScaleWatch()
      lyricsFontScaleWatch = null
    }
    // 每步异步清理独立兜错：任一失败都不能中断后续清理，否则监听器会泄漏
    try {
      await configStore.flushPendingSave()
    } catch (error) {
      logger.warn('Failed to flush config on unmount:', error)
    }
    // 移除 beforeunload 事件监听器
    window.removeEventListener('beforeunload', handleBeforeUnload)
    // 清理错误通知监听器
    options.unsubscribeErrorNotification()
    // 清理音轨监听器
    if (options.stopWatchTrack) {
      options.stopWatchTrack()
    }
    // 清理播放器资源
    try {
      await playerStore.cleanup()
    } catch (error) {
      logger.warn('Failed to cleanup player on unmount:', error)
    }
  })
}
