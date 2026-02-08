import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, UnlistenFn } from '@tauri-apps/api/event'

/**
 * 自动更新 Composable（模块级单例，避免多个组件重复监听事件）
 */
// 状态（模块级单例）
const isChecking = ref(false)
const updateAvailable = ref(false)
const newVersion = ref('')
const downloadUrl = ref('')
const downloadProgress = ref(0)
const isDownloading = ref(false)
const error = ref<string | null>(null)
const lastCheckTime = ref<string | null>(null)
const releaseNotes = ref<string | null>(null)
// 下载完成信息（installer 路径）
const installerPath = ref<string | null>(null)
const downloadFinished = ref(false)
// 后端日志（最后一条）
const updateLog = ref<string | null>(null)

let listenersStarted = false
let unlistenFns: UnlistenFn[] = []

/** GitHub 仓库信息 */
const GITHUB_REPO = 'JDBeWL/Mercurial-Player' // 替换为你的仓库
const GITHUB_API = `https://api.github.com/repos/${GITHUB_REPO}`

/** 启动 Tauri 事件监听（只启动一次） */
async function startListeners() {
  if (listenersStarted) return
  listenersStarted = true

  // 更新开始事件（可带 total size）
  const un0 = await listen<number>('update-started', event => {
    console.info('[auto-update] started, total size', event.payload)
    downloadProgress.value = 0
    isDownloading.value = true
  })
  unlistenFns.push(un0)

  // 进度事件
  const un1 = await listen<number>('update-progress', event => {
    console.debug('[auto-update] progress', event.payload)
    downloadProgress.value = event.payload as number
    isDownloading.value = true
  })
  unlistenFns.push(un1)

  // 错误事件
  const un2 = await listen<string>('update-error', event => {
    console.warn('[auto-update] error', event.payload)
    error.value = event.payload as string
    isDownloading.value = false
  })
  unlistenFns.push(un2)

  // 日志事件（后端会发送下载路径、开始/完成/失败消息）
  const un_log = await listen<string>('update-log', event => {
    console.info('[auto-update] log', event.payload)
    updateLog.value = event.payload as string
  })
  unlistenFns.push(un_log)

  // 完成事件
  const un3 = await listen<string>('update-finished', event => {
    console.info('[auto-update] finished', event.payload)
    downloadProgress.value = 100
    isDownloading.value = false
    downloadFinished.value = true
    // event.payload 可能是 installer path 或简短消息
    installerPath.value = typeof event.payload === 'string' && event.payload.length > 0 ? event.payload : null
  })
  unlistenFns.push(un3)
}

/** 清理监听器 */
export async function stopListeners() {
  for (const u of unlistenFns) {
    try { await u() } catch (_) {}
  }
  unlistenFns = []
  listenersStarted = false
}

/**
 * 获取当前应用版本
 */
const getCurrentVersion = async (): Promise<string> => {
  try {
    return await invoke('get_app_version')
  } catch (err) {
    console.error('Failed to get app version:', err)
    throw err
  }
}

/**
 * 获取 GitHub Releases 列表并选择一个合适的 release
 * 选择规则：跳过 draft；跳过带非数字 prerelease 后缀的 tag（例如 beta）；
 * 在基础版本（major.minor.patch）优先比较，基础相同则：
 *   - 稳定版本（无 prerelease）优先于任何 prerelease；
 *   - 若双方均为数字型 prerelease，则取数字更大的那个（例如 1.0.0-2 > 1.0.0-1）。
 */
const getLatestRelease = async (): Promise<any> => {
  try {
    const response = await fetch(`${GITHUB_API}/releases?per_page=50`)
    if (!response.ok) {
      throw new Error('Failed to fetch releases list')
    }

    const releases = await response.json()

    type Parsed = {
      baseNums: number[]
      prereleaseNum: number | null
      raw: string
    }

    const parseTag = (tag: string): { valid: boolean; parsed?: Parsed } => {
      const s = String(tag).replace(/^v/i, '')
      if (!s) return { valid: false }
      const [base, pre] = s.split('-', 2)
      const baseParts = base.split('.').slice(0, 3).map(p => parseInt(p.replace(/[^0-9]/g, ''), 10) || 0)
      while (baseParts.length < 3) baseParts.push(0)

      if (pre === undefined || pre === '') {
        return { valid: true, parsed: { baseNums: baseParts, prereleaseNum: null, raw: s } }
      }

      // 要求 prerelease 为全部数字（例如 1），否则视为不合规并跳过
      if (/^\d+$/.test(pre)) {
        return { valid: true, parsed: { baseNums: baseParts, prereleaseNum: parseInt(pre, 10), raw: s } }
      }

      // 非数字的 prerelease（比如 beta）视为无效（按你的要求）
      return { valid: false }
    }

    const compareParsed = (a: Parsed, b: Parsed): number => {
      for (let i = 0; i < 3; i++) {
        if (a.baseNums[i] !== b.baseNums[i]) return a.baseNums[i] - b.baseNums[i]
      }
      // base 相同：稳定版本（prereleaseNum === null）优先
      if (a.prereleaseNum === null && b.prereleaseNum === null) return 0
      if (a.prereleaseNum === null && b.prereleaseNum !== null) return 1
      if (a.prereleaseNum !== null && b.prereleaseNum === null) return -1
      // 双方均为数字型 prerelease，比较数字
      return (a.prereleaseNum! - b.prereleaseNum!)
    }

    let best: { release: any; parsed: Parsed } | null = null

    for (const r of releases) {
      if (r.draft) continue
      const { valid, parsed } = parseTag(r.tag_name || '')
      if (!valid || !parsed) continue

      if (!best) {
        best = { release: r, parsed }
        continue
      }

      if (compareParsed(parsed, best.parsed) > 0) {
        best = { release: r, parsed }
      }
    }

    if (!best) throw new Error('No suitable release found')
    return best.release
  } catch (err) {
    console.error('Failed to get latest release:', err)
    throw err
  }
}

/**
 * 将版本规范化为前三位数字（major.minor.patch），忽略任何预发布/构建编号
 */
const normalizeToThree = (v: string): number[] => {
  const s = String(v).replace(/^v/i, '')
  const parts = s.split('.')
  const nums = parts.slice(0, 3).map(p => parseInt(p.replace(/[^0-9]/g, ''), 10) || 0)
  while (nums.length < 3) nums.push(0)
  return nums
}

/**
 * 解析 tag，返回基础三段及可选的数字型 prerelease（null 表示稳定版）
 */
const parseVersionTag = (tag: string): { baseNums: number[]; prereleaseNum: number | null } => {
  const s = String(tag).replace(/^v/i, '')
  const [base, pre] = s.split('-', 2)
  const baseNums = base.split('.').slice(0, 3).map(p => parseInt(p.replace(/[^0-9]/g, ''), 10) || 0)
  while (baseNums.length < 3) baseNums.push(0)

  if (pre === undefined || pre === '') return { baseNums, prereleaseNum: null }
  if (/^\d+$/.test(pre)) return { baseNums, prereleaseNum: parseInt(pre, 10) }
  // 非数字 prerelease 视为无效，返回 prereleaseNum = null 但上层会依赖合法性检查
  return { baseNums, prereleaseNum: null }
}

/**
 * 比较版本（考虑数值型 prerelease）：
 * - 先比较前三位数字
 * - 若前三位不同则按大小判定
 * - 若前三位相同：稳定版本（无 prerelease）优先；若双方都为数字型 prerelease，则比较数字
 * 返回 true 如果 latest > current
 */
const isNewVersionAvailable = (current: string, latestTag: string): boolean => {
  const cur = parseVersionTag(current)
  const lat = parseVersionTag(latestTag)

  for (let i = 0; i < 3; i++) {
    if (cur.baseNums[i] < lat.baseNums[i]) return true
    if (cur.baseNums[i] > lat.baseNums[i]) return false
  }

  // base 相同
  if (cur.prereleaseNum === null && lat.prereleaseNum === null) return false
  // 当前稳定，远端为 prerelease => prerelease 在语义上低于稳定，不认为是新版本
  if (cur.prereleaseNum === null && lat.prereleaseNum !== null) return false
  // 当前为 prerelease，远端为稳定 => 远端为新版本
  if (cur.prereleaseNum !== null && lat.prereleaseNum === null) return true
  // 双方均为数字型 prerelease，比较数字
  if (cur.prereleaseNum !== null && lat.prereleaseNum !== null) return lat.prereleaseNum > cur.prereleaseNum

  return false
}

/**
 * 检查更新
 */
const checkForUpdates = async () => {
  isChecking.value = true
  error.value = null
  // 立即显示正在检查，给用户即时反馈
  lastCheckTime.value = 'Checking...'

  try {


    const currentVersion = await getCurrentVersion()
    const release = await getLatestRelease()

    const latestTagStr = String(release.tag_name || '').replace(/^v/i, '')

    if (isNewVersionAvailable(currentVersion, latestTagStr)) {
      updateAvailable.value = true
      // 仅展示前三位版本供用户参考
      newVersion.value = normalizeToThree(latestTagStr).join('.')
      lastCheckTime.value = new Date().toLocaleString()
      releaseNotes.value = release.body || ''

      // 查找 Windows 的安装包：优先选择 .exe，其次 .msi
      const windowsAssets = release.assets.filter((asset: any) =>
        asset.name.endsWith('.exe') || asset.name.endsWith('.msi')
      )
      const windowsAsset = windowsAssets.find((asset: any) => asset.name.endsWith('.exe'))
        || windowsAssets.find((asset: any) => asset.name.endsWith('.msi')) || null

      if (windowsAsset) {
        downloadUrl.value = windowsAsset.browser_download_url
      } else {
        throw new Error('No Windows installer found in release')
      }
    } else {
      lastCheckTime.value = new Date().toLocaleString()
    }
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Unknown error occurred'
    // 也记录检查时间，方便用户知道检查已完成但失败
    lastCheckTime.value = new Date().toLocaleString()
    console.error('Update check failed:', err)
  } finally {
    isChecking.value = false
  }
}

/**
 * 下载更新（不自动执行安装，安装由前端确认后触发）
 */
const downloadAndInstall = async () => {
  if (!downloadUrl.value) {
    error.value = 'No download URL available'
    return
  }

  // 开始监听进度/错误事件
  await startListeners()

  isDownloading.value = true
  downloadProgress.value = 0
  error.value = null

  try {
    // 旧命令名保持不变以兼容前端调用，但后端已改为只执行下载
    await invoke('download_and_install_update', {
      downloadUrl: downloadUrl.value,
    })
  } catch (err) {
    error.value = err instanceof Error ? err.message : 'Download failed'
    console.error('Download failed:', err)
    isDownloading.value = false
  }
}

/**
 * 在用户确认后执行安装程序（会在 release 构建中运行）
 */
const runInstaller = async () => {
  if (!installerPath.value) {
    error.value = 'No installer path available'
    return
  }
  try {
    await invoke('run_installer', { installerPath: installerPath.value })
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}

/**
 * 重置更新状态
 */
const resetUpdateState = () => {
  updateAvailable.value = false
  newVersion.value = ''
  downloadUrl.value = ''
  downloadProgress.value = 0
  error.value = null
  installerPath.value = null
  downloadFinished.value = false
}

const hasError = computed(() => error.value !== null)
const isUpdateProcessing = computed(() => isChecking.value || isDownloading.value)

export function useAutoUpdate() {
  return {
    // 状态
    isChecking,
    updateAvailable,
    newVersion,
    downloadUrl,
    downloadProgress,
    isDownloading,
    error,
    lastCheckTime,
    releaseNotes,
    installerPath,
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
