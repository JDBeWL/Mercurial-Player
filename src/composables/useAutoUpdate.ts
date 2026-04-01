import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'
import logger from '@/utils/logger'

/**
 * 自动更新 Composable（使用 Tauri v2 plugin-updater）
 *
 * plugin-updater 会自动根据 tauri.conf.json 中的 updater.endpoints 检查更新、
 * 下载差分/全量包并原地替换，无需手动下载 .exe 安装程序。
 */

// 状态（模块级单例）
const isChecking = ref(false)
const updateAvailable = ref(false)
const newVersion = ref('')
const downloadProgress = ref(0)
const isDownloading = ref(false)
const error = ref<string | null>(null)
const lastCheckTime = ref<string | null>(null)
const releaseNotes = ref<string | null>(null)
const downloadFinished = ref(false)
const updateLog = ref<string | null>(null)

// 保留一份 Update 对象引用，用于后续安装
let pendingUpdate: Update | null = null

const hasError = computed(() => error.value !== null)
const isUpdateProcessing = computed(() => isChecking.value || isDownloading.value)

/**
 * 获取当前应用版本
 */
const getCurrentVersion = async (): Promise<string> => {
  try {
    return await invoke('get_app_version')
  } catch (err) {
    logger.error('Failed to get app version:', err)
    throw err
  }
}

/**
 * 检查更新（通过 plugin-updater 自动读取 tauri.conf.json 中的 endpoints）
 */
const checkForUpdates = async () => {
  isChecking.value = true
  error.value = null
  lastCheckTime.value = 'Checking...'

  try {
    const update = await check()

    if (update) {
      pendingUpdate = update
      updateAvailable.value = true
      newVersion.value = update.version
      releaseNotes.value = update.body ?? ''
      logger.info(`[auto-update] New version available: ${update.version}`)
    } else {
      pendingUpdate = null
      updateAvailable.value = false
      logger.info('[auto-update] Already up to date')
    }

    lastCheckTime.value = new Date().toLocaleString()
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Unknown error occurred'
    lastCheckTime.value = new Date().toLocaleString()
    logger.error('Update check failed:', err)
  } finally {
    isChecking.value = false
  }
}

/**
 * 下载并安装更新
 *
 * plugin-updater 的 downloadAndInstall() 会自动下载差分包、校验签名、
 * 替换二进制并在关闭时生效。
 */
const downloadAndInstall = async () => {
  if (!pendingUpdate) {
    error.value = 'No update available to download'
    return
  }

  isDownloading.value = true
  downloadProgress.value = 0
  error.value = null

  try {
    let contentLength = 0

    await pendingUpdate.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          contentLength = event.data.contentLength ?? 0
          updateLog.value = `Download started, total size: ${contentLength}`
          logger.info(`[auto-update] Download started, size: ${contentLength}`)
          break
        case 'Progress': {
          const chunkLength = event.data.chunkLength
          if (contentLength > 0) {
            // 累加进度
            downloadProgress.value = Math.min(
              100,
              downloadProgress.value + Math.round((chunkLength / contentLength) * 100)
            )
          }
          break
        }
        case 'Finished':
          downloadProgress.value = 100
          downloadFinished.value = true
          isDownloading.value = false
          updateLog.value = 'Download finished, update will apply on next restart'
          logger.info('[auto-update] Download and install finished')
          break
      }
    })

    // downloadAndInstall 完成后，更新已准备好，下次重启应用即可生效
    downloadFinished.value = true
    isDownloading.value = false
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Download/install failed'
    logger.error('Download/install failed:', err)
    isDownloading.value = false
  }
}

/**
 * 重启应用以应用更新（plugin-updater 自动处理）
 */
const runInstaller = async () => {
  try {
    // plugin-updater 在 downloadAndInstall 完成后，重启应用即可生效
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
  pendingUpdate = null
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
    lastCheckTime,
    releaseNotes,
    downloadFinished,
    updateLog,
    hasError,
    isUpdateProcessing,
    // 方法
    checkForUpdates,
    downloadAndInstall,
    runInstaller,
    resetUpdateState,
    getCurrentVersion,
  }
}
