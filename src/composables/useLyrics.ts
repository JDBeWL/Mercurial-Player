import { ref, watch, markRaw, type Ref } from 'vue'
import { usePlayerStore } from '@/stores/player'
import { useConfigStore } from '@/stores/config'
import { FileUtils } from '@/utils/fileUtils'
import { neteaseApi } from '@/utils/neteaseApi'
import { LyricsParser, findLyricIndex } from '@/utils/lyricsParser'
import { LRUCache } from '@/utils/lruCache'
import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { LyricLine, Track } from '@/types'

// 模块级别的在线歌词缓存，限制最多50首，避免内存泄漏。
// 复用通用 LRU 实现;TTL 传 Infinity 表示本会话内不过期(歌词内容不可变)。
const onlineLyricsCache = new LRUCache<{ lrc: string; parsed: LyricLine[]; source: string }>(
  50,
  Infinity,
)

// 模块级别的共享状态：
// 所有 useLyrics 实例共享同一份 lyrics / loading / activeIndex / lyricsSource / onlineLyricsError,
// 以及同一套 watcher,避免多个调用方各自创建 watcher 导致重复 IPC 调用和内存泄漏。
const sharedLyrics = ref<LyricLine[]>([])
const sharedLoading = ref(false)
const sharedActiveIndex = ref(-1)
const sharedLyricsSource: Ref<'local' | 'online'> = ref('local')
const sharedOnlineLyricsError = ref<string | null>(null)

// 在线歌词请求经由 Tauri invoke 后端代理,前端的 AbortSignal 无法取消后端 HTTP 请求;
// 过期结果的丢弃由 store 的统一歌词请求守卫负责:
// player.beginLyricsRequest / isLyricsRequestCurrent (store.loadLyrics 与本模块共享同一计数器)。

// 模块级别的初始化标记
let isInitialized = false

// 共享 watcher 的停止函数,在 cleanup 时用于停止所有 watcher (HMR 场景)
const sharedWatchStopFns: Array<() => void> = []

// 模块级别的 store 引用（在 initializeSharedWatchers 中赋值）
let _playerStore: ReturnType<typeof usePlayerStore> | null = null
let _configStore: ReturnType<typeof useConfigStore> | null = null

/**
 * 获取在线歌词
 */
async function fetchOnlineLyrics(track: Track | null): Promise<string | null> {
  if (!track || !_configStore) return null
  try {
    const title = track.title || track.name || FileUtils.getFileNameWithoutExtension(track.path)
    const artist = track.artist || ''
    const duration = track.duration ? track.duration * 1000 : 0
    logger.debug('Fetching online lyrics for: ' + title + ' - ' + artist)
    const lyricsData = await neteaseApi.searchAndGetLyrics(title, artist, duration)
    if (!lyricsData || !lyricsData.lrc) {
      logger.debug('No online lyrics found')
      return null
    }
    let lrcContent = lyricsData.lrc
    if (_configStore.lyrics?.preferTranslation && lyricsData.tlyric) {
      lrcContent = neteaseApi.mergeLyrics(lyricsData.lrc, lyricsData.tlyric)
    }
    return lrcContent
  } catch (error) {
    logger.error('Failed to fetch online lyrics:', error)
    sharedOnlineLyricsError.value = (error as Error).message
    return null
  }
}

/**
 * 保存歌词到本地
 */
async function saveLyricsToLocal(trackPath: string, lrcContent: string): Promise<boolean> {
  if (!trackPath || !lrcContent) return false
  try {
    const baseName = FileUtils.getFileNameWithoutExtension(trackPath)
    const directory = FileUtils.getDirectoryPath(trackPath)
    const lyricsPath = FileUtils.joinPath(directory, `${baseName}.lrc`)
    await invoke('write_lyrics_file', { path: lyricsPath, content: lrcContent })
    logger.info('Lyrics saved to: ' + lyricsPath)
    return true
  } catch (error) {
    logger.error('Failed to save lyrics:', error)
    return false
  }
}

/**
 * 加载歌词（本地优先,失败时尝试在线获取）
 *
 * 写入统一走 _playerStore.lyrics (唯一事实源),
 * sharedLyrics 由 initializeSharedWatchers 中的同步 watcher 跟随更新。
 */
async function loadLyrics(trackPath: string | undefined): Promise<void> {
  if (!_playerStore || !_configStore) return
  // 序号守卫: 快速切歌时并发请求,只有最新一次的结果允许写入共享状态
  const seq = _playerStore.beginLyricsRequest()
  if (!trackPath) {
    _playerStore.lyrics = null
    sharedLyricsSource.value = 'local'
    sharedOnlineLyricsError.value = null
    return
  }

  // 先检查缓存中是否有这首歌的在线歌词
  const cached = onlineLyricsCache.get(trackPath)
  if (cached) {
    logger.debug('Using cached online lyrics for:', trackPath)
    _playerStore.lyrics = cached.parsed
    sharedLyricsSource.value = cached.source as 'local' | 'online'
    sharedLoading.value = false
    return
  }

  sharedLoading.value = true
  _playerStore.lyrics = null
  sharedLyricsSource.value = 'local'
  sharedOnlineLyricsError.value = null
  try {
    const lyricsPath = await FileUtils.findLyricsFile(trackPath)
    if (!_playerStore.isLyricsRequestCurrent(seq)) return
    if (lyricsPath) {
      const content = await FileUtils.readFile(lyricsPath)
      const ext = FileUtils.getFileExtension(lyricsPath) as 'lrc' | 'ass' | 'srt'
      // 使用统一的异步解析器
      // markRaw: 歌词只整体替换、不修改内部字段,无需深度响应式代理
      const parsed = markRaw(await LyricsParser.parseAsync(content, ext))
      if (!_playerStore.isLyricsRequestCurrent(seq)) return
      _playerStore.lyrics = parsed
      sharedLyricsSource.value = 'local'
    } else if (_configStore.lyrics?.enableOnlineFetch) {
      logger.debug('No local lyrics found, trying online fetch...')
      const track = _playerStore.currentTrack
      const onlineLrc = await fetchOnlineLyrics(track)
      if (!_playerStore.isLyricsRequestCurrent(seq)) return
      if (onlineLrc) {
        // markRaw: 歌词只整体替换、不修改内部字段,无需深度响应式代理
        const parsed = markRaw(await LyricsParser.parseAsync(onlineLrc, 'lrc'))
        if (!_playerStore.isLyricsRequestCurrent(seq)) return
        _playerStore.lyrics = parsed
        sharedLyricsSource.value = 'online'

        // 缓存在线歌词
        onlineLyricsCache.set(trackPath, {
          lrc: onlineLrc,
          parsed,
          source: 'online',
        })

        if (_configStore.lyrics?.autoSaveOnlineLyrics) {
          const saved = await saveLyricsToLocal(trackPath, onlineLrc)
          if (saved && _playerStore.isLyricsRequestCurrent(seq)) {
            sharedLyricsSource.value = 'local'
            // 保存成功后从缓存中移除，下次会从本地加载
            onlineLyricsCache.delete(trackPath)
          }
        }
      }
    }
  } catch (e) {
    logger.error('Error loading lyrics:', e)
    if (_playerStore.isLyricsRequestCurrent(seq)) {
      sharedOnlineLyricsError.value = (e as Error).message
    }
  } finally {
    // 只有最新一次请求有权结束 loading 状态,避免旧请求过早关闭新请求的 loading
    if (_playerStore.isLyricsRequestCurrent(seq)) {
      sharedLoading.value = false
    }
  }
}

/**
 * 初始化共享 watcher（只执行一次）
 *
 * 所有 useLyrics 调用方共享同一套 watcher 和状态,
 * 避免 LyricsDisplay / VisualizerPanel / App.vue 各自创建 watcher
 * 导致切歌时触发 3 次 loadLyrics 和每帧 3 次二分查找。
 */
function initializeSharedWatchers(): void {
  if (isInitialized) return
  isInitialized = true

  _playerStore = usePlayerStore()
  _configStore = useConfigStore()

  const stopWatchTrackPath = watch(() => _playerStore!.currentTrack?.path, loadLyrics, {
    immediate: true,
  })
  sharedWatchStopFns.push(stopWatchTrackPath)

  // store.lyrics 是唯一事实源:任何写入路径 (本模块 / store.loadLyrics / 插件 API)
  // 都经由该同步 watcher 反映到 sharedLyrics,保证两个状态视图一致。
  // flush: 'sync' 保持与直接赋值相同的时机语义。
  const stopWatchStoreLyrics = watch(
    () => _playerStore!.lyrics,
    (lyrics) => {
      sharedLyrics.value = lyrics ?? []
    },
    { immediate: true, flush: 'sync' },
  )
  sharedWatchStopFns.push(stopWatchStoreLyrics)

  // activeIndex 更新逻辑 - 使用节流避免高频更新
  let lastActiveIndexUpdate = 0
  const ACTIVE_INDEX_THROTTLE = 100 // 每 100ms 更新一次

  const stopWatchCurrentTime = watch(
    () => _playerStore!.currentTime,
    (currentTime) => {
      if (!sharedLyrics.value.length) {
        if (sharedActiveIndex.value !== -1) {
          sharedActiveIndex.value = -1
          _playerStore!.currentLyricIndex = -1
        }
        return
      }

      // 节流：避免每次 currentTime 变化都计算
      const now = Date.now()
      if (now - lastActiveIndexUpdate < ACTIVE_INDEX_THROTTLE) return
      lastActiveIndexUpdate = now

      // 应用歌词偏移
      const offset = _playerStore!.lyricsOffset || 0
      const adjustedTime = currentTime - offset

      // 二分查找当前歌词索引
      const idx = findLyricIndex(sharedLyrics.value, adjustedTime)

      if (idx !== sharedActiveIndex.value) {
        sharedActiveIndex.value = idx
        _playerStore!.currentLyricIndex = idx
      }
    },
    { immediate: true },
  )
  sharedWatchStopFns.push(stopWatchCurrentTime)
}

export function useLyrics() {
  // 首次调用时初始化共享 watcher
  initializeSharedWatchers()

  const playerStore = usePlayerStore()
  const configStore = useConfigStore()

  const fetchAndSaveLyrics = async (): Promise<boolean> => {
    const track = playerStore.currentTrack
    if (!track) return false
    // 序号守卫:手动刷新也纳入统一计数,快速切歌时旧请求的结果不写入状态
    const seq = playerStore.beginLyricsRequest()
    sharedLoading.value = true
    sharedOnlineLyricsError.value = null
    try {
      const onlineLrc = await fetchOnlineLyrics(track)
      if (!playerStore.isLyricsRequestCurrent(seq)) return false
      if (onlineLrc) {
        // markRaw: 歌词只整体替换、不修改内部字段,无需深度响应式代理
        const parsed = markRaw(await LyricsParser.parseAsync(onlineLrc, 'lrc'))
        if (!playerStore.isLyricsRequestCurrent(seq)) return false
        playerStore.lyrics = parsed
        sharedLyricsSource.value = 'online'

        // 缓存在线歌词
        onlineLyricsCache.set(track.path, {
          lrc: onlineLrc,
          parsed,
          source: 'online',
        })

        // 只有在启用自动保存时才保存到本地
        if (configStore.lyrics?.autoSaveOnlineLyrics) {
          const saved = await saveLyricsToLocal(track.path, onlineLrc)
          if (saved && playerStore.isLyricsRequestCurrent(seq)) {
            sharedLyricsSource.value = 'local'
            // 保存成功后从缓存中移除
            onlineLyricsCache.delete(track.path)
          }
        }
        return true
      }
      return false
    } catch (e) {
      logger.error('Error fetching lyrics:', e)
      if (playerStore.isLyricsRequestCurrent(seq)) {
        sharedOnlineLyricsError.value = (e as Error).message
      }
      return false
    } finally {
      if (playerStore.isLyricsRequestCurrent(seq)) {
        sharedLoading.value = false
      }
    }
  }

  // cleanup 用于显式全量清理 (如 HMR 重建 store 时手动调用):
  // 停止共享 watcher、重置初始化标记与 store 引用、清空共享状态,
  // 以便下次 useLyrics() 调用时重新初始化并引用新的 store。
  // 注意:不要在单个组件 onUnmounted 中调用 cleanup,
  // 否则会停掉其他调用方共享的 watcher 导致丢失更新。
  const cleanup = (): void => {
    // 作废所有进行中的歌词请求 (过期结果由统一序号守卫丢弃)
    _playerStore?.beginLyricsRequest()
    // 停止所有共享 watcher,避免 HMR 重建 store 后旧 watcher 仍引用旧 store
    sharedWatchStopFns.forEach((fn) => fn())
    sharedWatchStopFns.length = 0
    // 重置初始化标记,允许下次 useLyrics() 调用重新初始化 watchers(引用新的 store)
    isInitialized = false
    // 注意:不置空 _playerStore / _configStore,避免 cleanup 后 loadLyrics /
    // fetchOnlineLyrics 因守卫检查而静默失效。下次 initializeSharedWatchers()
    // 会重新赋值为最新的 store 实例。
    // 清空共享状态
    sharedLyrics.value = []
    sharedLoading.value = false
    sharedActiveIndex.value = -1
    sharedLyricsSource.value = 'local'
    sharedOnlineLyricsError.value = null
  }

  return {
    lyrics: sharedLyrics,
    loading: sharedLoading,
    activeIndex: sharedActiveIndex,
    lyricsSource: sharedLyricsSource,
    onlineLyricsError: sharedOnlineLyricsError,
    fetchAndSaveLyrics,
    loadLyrics,
    cleanup,
  }
}
