import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import i18n from '@/i18n'
import logger from '@/utils/logger'

/** 自动更新: 检查与安装委托 tauri-plugin-updater, 只有下载换成多线程分片下载并自校 minisign 签名; endpoints 与 pubkey 读 tauri.conf.json */

/** updater_check 命令返回的更新信息 */
interface UpdateInfo {
  version: string
  notes: string | null
  date: string | null
  currentVersion: string
}

/** updater://download-progress 事件负载 */
interface DownloadProgressPayload {
  downloaded: number
  total: number
}

/** 下载进度事件名 (与 src-tauri/src/updater.rs 的事件名一致, 改一边必须改另一边) */
const PROGRESS_EVENT = 'updater://download-progress'

// 更新状态 (模块级单例, 各调用方共享同一份)
const isChecking = ref(false)
const updateAvailable = ref(false)
const newVersion = ref('')
const downloadProgress = ref(0)
const isDownloading = ref(false)
const error = ref<string | null>(null)
const releaseNotes = ref<string | null>(null)
const downloadFinished = ref(false)
// 已下载字节数 / 总字节数 (未知时为 0) / 平滑后的下载速度 (字节每秒)
const downloadedBytes = ref(0)
const totalBytes = ref(0)
const downloadSpeed = ref(0)

const hasError = computed(() => error.value !== null)
const isUpdateProcessing = computed(() => isChecking.value || isDownloading.value)

/** 从 Tauri 命令错误里尽量取出可读文本, 否则界面会显示 "Unknown error occurred" */
const extractErrorMessage = (err: unknown): string =>
  err instanceof Error
    ? err.message
    : typeof err === 'string'
      ? err
      : ((err as { message?: string })?.message ?? JSON.stringify(err))

/** 检查更新: 版本比较在后端由 plugin-updater 按 tauri.conf.json 的 endpoints 完成, 返回 null 即已是最新 */
const checkForUpdates = async () => {
  isChecking.value = true
  error.value = null

  try {
    const update = await invoke<UpdateInfo | null>('updater_check')

    if (update) {
      updateAvailable.value = true
      newVersion.value = update.version
      releaseNotes.value = update.notes ?? ''
      logger.info(`[auto-update] New version available: ${update.version}`)
    } else {
      updateAvailable.value = false
      logger.info('[auto-update] Already up to date')
    }
  } catch (err) {
    error.value = extractErrorMessage(err)
    logger.error('Update check failed:', err)
  } finally {
    isChecking.value = false
  }
}

/** 下载与安装分两步: updater_download 分片下载并校验 minisign 签名, 校验通过才把临时文件交给 updater_install, 顺序不可颠倒 */
const downloadAndInstall = async () => {
  if (!updateAvailable.value) {
    error.value = i18n.global.t('config.update.noUpdateAvailable')
    return
  }

  isDownloading.value = true
  downloadProgress.value = 0
  downloadedBytes.value = 0
  totalBytes.value = 0
  downloadSpeed.value = 0
  error.value = null

  // 速度取相邻两次进度事件的字节差/时间差, 再按 0.6 旧值 + 0.4 瞬时值指数平滑
  // lastTime 用 null 而不是 0 表示"未取样": 时间戳 0 合法, 拿 0 当哨兵会让首个事件落在 0 时永不开始计时
  let lastTime: number | null = null
  let lastBytes = 0

  try {
    const unlisten = await listen<DownloadProgressPayload>(PROGRESS_EVENT, (e) => {
      const { downloaded, total } = e.payload
      const now = performance.now()

      if (lastTime !== null) {
        const seconds = (now - lastTime) / 1000
        if (seconds > 0) {
          const instant = (downloaded - lastBytes) / seconds
          downloadSpeed.value =
            downloadSpeed.value === 0 ? instant : downloadSpeed.value * 0.6 + instant * 0.4
        }
      }
      lastTime = now
      lastBytes = downloaded

      downloadedBytes.value = downloaded
      totalBytes.value = total
      if (total > 0) {
        downloadProgress.value = Math.min(100, Math.round((downloaded / total) * 100))
      }
    })

    try {
      await invoke('updater_download')
      downloadProgress.value = 100
      downloadFinished.value = true
      downloadSpeed.value = 0
      logger.info('[auto-update] Download finished, installing')

      // Windows 下安装器拉起后本进程退出, 这个 invoke 通常不会返回
      await invoke('updater_install')
    } finally {
      unlisten()
    }

    isDownloading.value = false
  } catch (err) {
    error.value = extractErrorMessage(err)
    logger.error('Download/install failed:', err)
    downloadSpeed.value = 0
    isDownloading.value = false
  }
}

/** 重启应用以应用更新 (安装失败时的兜底入口) */
const runInstaller = async () => {
  try {
    const { relaunch } = await import('@tauri-apps/plugin-process')
    await relaunch()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    logger.error('Failed to relaunch:', err)
  }
}

const resetUpdateState = () => {
  updateAvailable.value = false
  newVersion.value = ''
  downloadProgress.value = 0
  error.value = null
  downloadFinished.value = false
  downloadedBytes.value = 0
  totalBytes.value = 0
  downloadSpeed.value = 0
}

export function useAutoUpdate() {
  return {
    isChecking,
    updateAvailable,
    newVersion,
    downloadProgress,
    isDownloading,
    error,
    releaseNotes,
    downloadFinished,
    downloadedBytes,
    totalBytes,
    downloadSpeed,
    hasError,
    isUpdateProcessing,
    checkForUpdates,
    downloadAndInstall,
    runInstaller,
    resetUpdateState,
  }
}
