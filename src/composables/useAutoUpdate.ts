import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import logger from '@/utils/logger'

/**
 * 自动更新 Composable
 *
 * 检查/安装流程复用 Tauri v2 plugin-updater（自动读取 tauri.conf.json 中的
 * updater.endpoints/pubkey），下载阶段走后端 updater_download 命令的
 * 多线程分片下载（HTTP Range 并发请求），签名校验通过后安装。
 */

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

/** 下载进度事件名（与 src-tauri/src/updater.rs 保持一致） */
const PROGRESS_EVENT = 'updater://download-progress'

// 状态（模块级单例）
const isChecking = ref(false)
const updateAvailable = ref(false)
const newVersion = ref('')
const downloadProgress = ref(0)
const isDownloading = ref(false)
const error = ref<string | null>(null)
const releaseNotes = ref<string | null>(null)
const downloadFinished = ref(false)
// 已下载字节数 / 总字节数（未知时为 0）/ 平滑后的下载速度（字节/秒）
const downloadedBytes = ref(0)
const totalBytes = ref(0)
const downloadSpeed = ref(0)

const hasError = computed(() => error.value !== null)
const isUpdateProcessing = computed(() => isChecking.value || isDownloading.value)

/** 尽可能从 Tauri 命令错误中提取可读信息，避免显示 "Unknown error occurred" */
const extractErrorMessage = (err: unknown): string =>
  err instanceof Error
    ? err.message
    : typeof err === 'string'
      ? err
      : ((err as { message?: string })?.message ?? JSON.stringify(err))

/**
 * 检查更新（后端读取 tauri.conf.json 中的 endpoints 配置）
 */
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

/**
 * 下载并安装更新
 *
 * updater_download 多线程分片下载并校验 minisign 签名，
 * updater_install 委托 plugin-updater 原生安装
 * （Windows 下拉起安装器并退出进程，由安装器完成替换和重启）。
 */
const downloadAndInstall = async () => {
  if (!updateAvailable.value) {
    error.value = 'No update available to download'
    return
  }

  isDownloading.value = true
  downloadProgress.value = 0
  downloadedBytes.value = 0
  totalBytes.value = 0
  downloadSpeed.value = 0
  error.value = null

  // 速度估算：记录上次进度事件的时间与字节数，按差值计算瞬时速度并做指数平滑
  let lastTime = 0
  let lastBytes = 0

  try {
    const unlisten = await listen<DownloadProgressPayload>(PROGRESS_EVENT, (e) => {
      const { downloaded, total } = e.payload
      const now = performance.now()

      if (lastTime > 0) {
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

      // Windows 上安装器拉起后进程退出，invoke 不会返回
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

/**
 * 重启应用以应用更新（安装失败时的兜底入口）
 */
const runInstaller = async () => {
  try {
    const { relaunch } = await import('@tauri-apps/plugin-process')
    await relaunch()
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
    logger.error('Failed to relaunch:', err)
  }
}

/**
 * 重置更新状态
 */
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
    // 状态
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
    // 方法
    checkForUpdates,
    downloadAndInstall,
    runInstaller,
    resetUpdateState,
  }
}
