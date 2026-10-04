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
  checkForUpdates: () => Promise<void>
  updateAvailable: Ref<boolean>
  newVersion: Ref<string>
  showError: (message: string, severity: ErrorSeverity, duration?: number) => number
  // errorHandler 桥接监听器是模块级单例, 卸载时必须显式退订才会复位
  unsubscribeErrorNotification: () => void
  // 初始化序列结束后同步一次窗口状态, 理由见 useWindowControls.syncWindowState
  syncWindowState: () => Promise<void>
  // useTrackInfo 返回的 watch 停止句柄
  stopWatchTrack: WatchStopHandle | null
}

/** 应用生命周期: 集中 App.vue 的启动初始化序列与卸载清理; 键盘监听由 useGlobalKeyboard 自行注册/注销 */
export function useAppLifecycle(options: UseAppLifecycleOptions): void {
  const playerStore = usePlayerStore()
  const configStore = useConfigStore()
  const themeStore = useThemeStore()
  const { t } = useI18n()

  // 关闭前保存配置并清理资源。beforeunload 里 WebView 不保证等异步 IPC 完成, 所以这只是兜底:
  // fire-and-forget 发出 IPC, 消息一旦发出后端就会处理; 真正的关闭路径见下面的 onCloseRequested
  const flushResourcesOnClose = (): void => {
    void configStore.flushPendingSave()
    // playerStore.cleanup 顺带注销全局快捷键
    void playerStore.cleanup()
    void pluginManager.cleanup()
  }

  const handleBeforeUnload = (): void => {
    flushResourcesOnClose()
  }

  // 这两个 unlisten 与防抖定时器都在 onUnmounted 里清理
  let unlistenWindowMove: (() => void) | null = null
  let unlistenCloseRequested: (() => void) | null = null
  let moveDebounceTimer: ReturnType<typeof setTimeout> | null = null
  // 歌词字号 -> 根元素 CSS 变量的 watch, 在 onMounted 里才建立故类型可空
  let lyricsFontScaleWatch: WatchStopHandle | null = null
  // 防止清理期间用户再次触发关闭
  let isClosing = false

  onMounted(async () => {
    window.addEventListener('beforeunload', handleBeforeUnload)

    // 主关闭路径: 拦下关闭请求, 等异步清理 (配置 flush / 播放器 / 插件) 完成再销毁, 确保数据落盘
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
          // destroy 不会再触发 CloseRequested, 是真正的退出通道
          await getCurrentWindow().destroy()
        }
      })
    } catch (error) {
      logger.warn('Failed to listen close requested:', error)
    }

    // 加载配置文件, 参数 true 表示启动场景 (允许重置 UI 状态)
    try {
      await configStore.loadConfig(true)
    } catch (error) {
      logger.warn('Failed to load configuration:', error)
    }

    try {
      setLocale(configStore.general.language || 'zh')
    } catch (error) {
      logger.error('Failed to apply language from config:', error)
    }

    // 界面字号: 配置就绪后先对齐一次 (抵消机制见 useAppFontScale), 之后改配置由设置页在滑块松手时下发
    await applyAppFontScale(configStore.ui?.fontScale ?? 1)

    // 歌词字号的 watch 放在配置加载之后, immediate 首帧拿到的就是已加载值
    lyricsFontScaleWatch = watch(() => configStore.lyrics?.fontScale ?? 1, applyLyricsFontScale, {
      immediate: true,
    })

    try {
      const savedTheme = configStore.general.theme
      if (savedTheme) {
        themeStore.setThemePreference(savedTheme)
      }
    } catch (error) {
      logger.error('Failed to apply theme from config:', error)
    }

    themeStore.applyTheme()

    // 封面缓存路径: Android 已由后端固定到应用沙箱, 前端不能覆盖
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

    await playerStore.initAudio()

    // 按 last_session 校验并恢复上次播放; 失败只记日志, 按产品要求不弹提示
    try {
      await playerStore.resumeLastSession()
    } catch (error) {
      logger.warn('Failed to resume last session:', error)
    }

    // 目标帧率要下发给后端算 FFT 频率, 取值规则见 utils/visualizerFps
    try {
      const result = await applyVisualizerFps(configStore.visualizer)
      if (result) {
        logger.info(`Target FPS set to ${result.fps}`)
      }
    } catch (error) {
      logger.warn('Failed to set target FPS:', error)
    }

    // 窗口跨屏后所在显示器可能变, 要重算帧率限制; onMoved 拖动期间高频触发, 防抖 500ms
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

    // 启动时只检查更新, 不自动安装
    try {
      if (configStore.general.enableAutoUpdate) {
        await options.checkForUpdates()
        if (options.updateAvailable.value) {
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

    // 全屏/最大化按钮图标依赖这两个状态, 见 useWindowControls.syncWindowState
    await options.syncWindowState()
  })

  onUnmounted(async () => {
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
    // 每步独立兜错: 任一清理失败都不能中断后续步骤, 否则监听器泄漏
    try {
      await configStore.flushPendingSave()
    } catch (error) {
      logger.warn('Failed to flush config on unmount:', error)
    }
    window.removeEventListener('beforeunload', handleBeforeUnload)
    options.unsubscribeErrorNotification()
    if (options.stopWatchTrack) {
      options.stopWatchTrack()
    }
    try {
      await playerStore.cleanup()
    } catch (error) {
      logger.warn('Failed to cleanup player on unmount:', error)
    }
  })
}
