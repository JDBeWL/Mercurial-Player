import { beforeEach, describe, expect, it, vi } from 'vitest'

const eventApi = vi.hoisted(() => ({
  listen: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: eventApi.listen,
}))

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { mockInvoke } = await import('../mocks/tauri')
const { useAutoUpdate } = await import('@/composables/useAutoUpdate')

/** 捕获 listen 注册的事件回调,便于在 invoke 进行中推送进度 */
let progressHandler: ((e: { payload: { downloaded: number; total: number } }) => void) | null = null
let unlisten = vi.fn()

/** 让 performance.now() 受控,否则瞬时速度计算依赖真实时钟,结果不稳定 */
let nowMs = 0

beforeEach(() => {
  vi.clearAllMocks()
  progressHandler = null
  unlisten = vi.fn()
  nowMs = 0
  vi.spyOn(performance, 'now').mockImplementation(() => nowMs)

  eventApi.listen.mockImplementation((_event: string, handler: typeof progressHandler) => {
    progressHandler = handler
    return Promise.resolve(unlisten)
  })

  // 默认:下载/安装都成功;download 期间推一次进度事件
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'updater_download') {
      progressHandler?.({ payload: { downloaded: 50, total: 100 } })
      return Promise.resolve(undefined)
    }
    return Promise.resolve(undefined)
  })

  useAutoUpdate().resetUpdateState()
})

interface DownloadSample {
  progress: number
  speed: number
  downloadedBytes: number
  totalBytes: number
}

/**
 * 下载成功后源码会把 downloadProgress 置 100、downloadSpeed 置 0,
 * 因此进度与速度只能在 updater_download 进行中取样,不能在 await 之后断言。
 */
const recordDuringDownload = (
  emit: (push: (payload: { downloaded: number; total: number }) => void) => void,
): DownloadSample => {
  const api = useAutoUpdate()
  const sample: DownloadSample = { progress: 0, speed: 0, downloadedBytes: 0, totalBytes: 0 }

  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'updater_download') {
      emit((payload) => progressHandler?.({ payload }))
      sample.progress = api.downloadProgress.value
      sample.speed = api.downloadSpeed.value
      sample.downloadedBytes = api.downloadedBytes.value
      sample.totalBytes = api.totalBytes.value
      return Promise.resolve(undefined)
    }
    return Promise.resolve(undefined)
  })

  return sample
}

describe('useAutoUpdate > checkForUpdates', () => {
  it('records the new version and release notes when an update exists', async () => {
    mockInvoke.mockResolvedValue({
      version: '2.0.0',
      notes: 'fixes',
      date: null,
      currentVersion: '1.0.0',
    })
    const { checkForUpdates, updateAvailable, newVersion, releaseNotes, isChecking } =
      useAutoUpdate()

    await checkForUpdates()

    expect(mockInvoke).toHaveBeenCalledWith('updater_check')
    expect(updateAvailable.value).toBe(true)
    expect(newVersion.value).toBe('2.0.0')
    expect(releaseNotes.value).toBe('fixes')
    expect(isChecking.value).toBe(false)
  })

  it('falls back to an empty string when the notes are null', async () => {
    mockInvoke.mockResolvedValue({
      version: '2.0.0',
      notes: null,
      date: null,
      currentVersion: '1.0.0',
    })
    const { checkForUpdates, releaseNotes } = useAutoUpdate()

    await checkForUpdates()

    expect(releaseNotes.value).toBe('')
  })

  it('clears the available flag when already up to date', async () => {
    mockInvoke.mockResolvedValue(null)
    const { checkForUpdates, updateAvailable, isChecking } = useAutoUpdate()

    await checkForUpdates()

    expect(updateAvailable.value).toBe(false)
    expect(isChecking.value).toBe(false)
  })

  it('clears isChecking even when the request rejects', async () => {
    mockInvoke.mockRejectedValue(new Error('offline'))
    const { checkForUpdates, isChecking, error, hasError } = useAutoUpdate()

    await checkForUpdates()

    expect(isChecking.value).toBe(false)
    expect(error.value).toBe('offline')
    expect(hasError.value).toBe(true)
  })

  it('extracts the message from a string rejection', async () => {
    mockInvoke.mockRejectedValue('boom')
    const { checkForUpdates, error } = useAutoUpdate()

    await checkForUpdates()

    expect(error.value).toBe('boom')
  })

  it('extracts the message from an error-like object', async () => {
    mockInvoke.mockRejectedValue({ message: 'wrapped' })
    const { checkForUpdates, error } = useAutoUpdate()

    await checkForUpdates()

    expect(error.value).toBe('wrapped')
  })

  it('stringifies unknown rejection payloads', async () => {
    mockInvoke.mockRejectedValue({ code: 42 })
    const { checkForUpdates, error } = useAutoUpdate()

    await checkForUpdates()

    expect(error.value).toBe('{"code":42}')
  })
})

describe('useAutoUpdate > downloadAndInstall', () => {
  it('refuses to download when no update is available', async () => {
    const { downloadAndInstall, error, isDownloading } = useAutoUpdate()

    await downloadAndInstall()

    expect(error.value).toBe('No update available to download')
    expect(isDownloading.value).toBe(false)
    expect(mockInvoke).not.toHaveBeenCalledWith('updater_download')
  })

  it('downloads then installs, and reports completion', async () => {
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(mockInvoke).toHaveBeenCalledWith('updater_download')
    expect(mockInvoke).toHaveBeenCalledWith('updater_install')
    expect(api.downloadFinished.value).toBe(true)
    expect(api.downloadProgress.value).toBe(100)
    expect(api.downloadSpeed.value).toBe(0)
    expect(api.isDownloading.value).toBe(false)
  })

  it('unlistens the progress event after a successful install', async () => {
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('tracks downloaded and total bytes from progress events', async () => {
    const samples = recordDuringDownload((push) => push({ downloaded: 50, total: 100 }))
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(samples.downloadedBytes).toBe(50)
    expect(samples.totalBytes).toBe(100)
    expect(samples.progress).toBe(50)
    expect(api.downloadedBytes.value).toBe(50)
    expect(api.totalBytes.value).toBe(100)
  })

  it('keeps the progress at zero when the total size is unknown', async () => {
    const samples = recordDuringDownload((push) => push({ downloaded: 10, total: 0 }))
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(samples.progress).toBe(0)
    expect(api.downloadedBytes.value).toBe(10)
  })

  it('caps the reported progress at 100 percent', async () => {
    const samples = recordDuringDownload((push) => push({ downloaded: 200, total: 100 }))
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(samples.progress).toBe(100)
  })

  it('uses the instantaneous speed for the first measured interval', async () => {
    // 首个事件只记录基准,第二个事件(间隔 1s,新增 100B)才算速度 = 100B/s
    const samples = recordDuringDownload((push) => {
      nowMs = 0
      push({ downloaded: 0, total: 100 })
      nowMs = 1000
      push({ downloaded: 100, total: 100 })
    })
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(samples.speed).toBeCloseTo(100)
  })

  it('smooths the speed with exponential moving average on later intervals', async () => {
    // 第一次测得 100B/s,第二次瞬时 200B/s → 0.6 * 100 + 0.4 * 200 = 140
    const samples = recordDuringDownload((push) => {
      nowMs = 0
      push({ downloaded: 0, total: 300 })
      nowMs = 1000
      push({ downloaded: 100, total: 300 })
      nowMs = 2000
      push({ downloaded: 300, total: 300 })
    })
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(samples.speed).toBeCloseTo(140)
  })

  it('ignores zero-length intervals so the speed stays finite', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'updater_download') {
        nowMs = 500
        progressHandler?.({ payload: { downloaded: 0, total: 100 } })
        // 时钟未推进 → seconds === 0,应跳过速度计算
        progressHandler?.({ payload: { downloaded: 100, total: 100 } })
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(api.downloadSpeed.value).toBe(0)
    expect(api.downloadedBytes.value).toBe(100)
  })

  it('records the failure and stops downloading when the download rejects', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'updater_download') return Promise.reject(new Error('disk full'))
      return Promise.resolve(undefined)
    })
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(api.error.value).toBe('disk full')
    expect(api.isDownloading.value).toBe(false)
    expect(api.downloadFinished.value).toBe(false)
    expect(mockInvoke).not.toHaveBeenCalledWith('updater_install')
  })

  it('unlistens the progress event even when the download fails', async () => {
    mockInvoke.mockRejectedValue(new Error('nope'))
    const api = useAutoUpdate()
    api.updateAvailable.value = true

    await api.downloadAndInstall()

    expect(unlisten).toHaveBeenCalledTimes(1)
  })

  it('reports isUpdateProcessing while checking or downloading', async () => {
    const api = useAutoUpdate()
    expect(api.isUpdateProcessing.value).toBe(false)

    api.isChecking.value = true
    expect(api.isUpdateProcessing.value).toBe(true)

    api.isChecking.value = false
    api.isDownloading.value = true
    expect(api.isUpdateProcessing.value).toBe(true)
  })
})

describe('useAutoUpdate > runInstaller', () => {
  it('relaunches the app', async () => {
    const { mockRelaunch } = await import('../mocks/tauri')
    mockRelaunch.mockResolvedValue(undefined)
    const api = useAutoUpdate()

    await api.runInstaller()

    expect(mockRelaunch).toHaveBeenCalled()
    expect(api.error.value).toBeNull()
  })

  it('records the error when relaunch fails', async () => {
    const { mockRelaunch } = await import('../mocks/tauri')
    mockRelaunch.mockRejectedValue(new Error('relaunch denied'))
    const api = useAutoUpdate()

    await api.runInstaller()

    expect(api.error.value).toBe('relaunch denied')
  })

  it('stringifies non-Error relaunch failures', async () => {
    const { mockRelaunch } = await import('../mocks/tauri')
    mockRelaunch.mockRejectedValue('plain string')
    const api = useAutoUpdate()

    await api.runInstaller()

    expect(api.error.value).toBe('plain string')
  })
})

describe('useAutoUpdate > resetUpdateState', () => {
  it('clears every piece of update state', () => {
    const api = useAutoUpdate()
    api.updateAvailable.value = true
    api.newVersion.value = '9.9.9'
    api.downloadProgress.value = 42
    api.error.value = 'old'
    api.downloadFinished.value = true
    api.downloadedBytes.value = 10
    api.totalBytes.value = 20
    api.downloadSpeed.value = 30

    api.resetUpdateState()

    expect(api.updateAvailable.value).toBe(false)
    expect(api.newVersion.value).toBe('')
    expect(api.downloadProgress.value).toBe(0)
    expect(api.error.value).toBeNull()
    expect(api.downloadFinished.value).toBe(false)
    expect(api.downloadedBytes.value).toBe(0)
    expect(api.totalBytes.value).toBe(0)
    expect(api.downloadSpeed.value).toBe(0)
    expect(api.hasError.value).toBe(false)
  })
})
