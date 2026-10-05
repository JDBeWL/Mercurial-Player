import { ref, watch, markRaw, type Ref } from 'vue'
import { usePlayerStore } from '@/stores/player'
import { useConfigStore } from '@/stores/config'
import { FileUtils } from '@/utils/fileUtils'
import { fetchBestLyrics, collectCandidates, buildFinalLyric } from '@/services/lyrics'
import type { LyricCandidate, LyricKind } from '@/services/lyrics'
import { LyricsParser, findLyricIndex } from '@/utils/lyricsParser'
import { LRUCache } from '@/utils/lruCache'
import { invoke } from '@tauri-apps/api/core'
import logger from '@/utils/logger'
import type { LyricLine, LyricsConfig, Track } from '@/types'

// 在线歌词缓存:上限 50 首;TTL 传 Infinity 表示会话内不过期 (歌词内容不可变)
const onlineLyricsCache = new LRUCache<{
  content: string
  format: 'lrc' | 'ass'
  parsed: LyricLine[]
  source: string
}>(50, Infinity)

// 模块级共享状态:所有 useLyrics 实例共用同一份,原因见 initializeSharedWatchers
const sharedLyrics = ref<LyricLine[]>([])
const sharedLoading = ref(false)
const sharedActiveIndex = ref(-1)
const sharedLyricsSource: Ref<'local' | 'online'> = ref('local')
const sharedOnlineLyricsError = ref<string | null>(null)

// 在线歌词经由 Tauri invoke 后端代理,前端 AbortSignal 取消不了后端 HTTP 请求;
// 过期结果的丢弃统一交给 store 的序号守卫 (player.beginLyricsRequest / isLyricsRequestCurrent,
// store.loadLyrics 与本模块共享同一计数器)

// 序号守卫用的小于一切的占位 id：歌词请求 id 从 1 起自增，所以它永远不等于当前有效请求。
// "只落盘、不碰在屏歌词"的分支用它占位，避免为了走同一套代码而作废当前曲目的加载请求
const NO_LYRICS_REQUEST = -1

// 模块级初始化标记:共享 watcher 只建立一次
let isInitialized = false

// 共享 watcher 的停止函数,cleanup 时用于全部停止 (HMR 重建 store 后旧 watcher 会引用旧实例)
const sharedWatchStopFns: Array<() => void> = []

// 模块级 store 引用,在 initializeSharedWatchers 中赋值
// cleanup 时刻意不置空:置空会让 loadLyrics / fetchOnlineLyrics 因守卫检查静默失效,
// 下次 initializeSharedWatchers() 会重新指向最新的 store 实例
let _playerStore: ReturnType<typeof usePlayerStore> | null = null
let _configStore: ReturnType<typeof useConfigStore> | null = null

/** 兜底歌词配置:store 配置缺字段时使用,避免多来源流程崩溃 */
function safeLyricsConfig(): LyricsConfig {
  return (
    _configStore?.lyrics ?? {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease',
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Noto Sans SC',
      lyricsStyle: 'modern',
      lyricProviderOrder: ['netease'],
      lyricProviderSettings: {},
    }
  )
}

/** 从当前曲目构造歌词查询参数;duration_ms 是毫秒,而 store 的 duration 以秒计 */
function buildQuery(track: Track | null): { title: string; artist: string; duration_ms: number } {
  const title =
    track?.title || track?.name || FileUtils.getFileNameWithoutExtension(track?.path || '')
  const artist = track?.artist || ''
  const duration_ms = track?.duration ? track.duration * 1000 : 0
  return { title, artist, duration_ms }
}

/** 获取在线歌词（按配置的来源顺延自动择优，best-effort） */
async function fetchOnlineLyrics(
  track: Track | null,
): Promise<{ content: string; format: 'lrc' | 'ass' } | null> {
  if (!track || !_configStore) return null
  try {
    logger.debug('Fetching online lyrics for: ' + track.title + ' - ' + track.artist)
    const result = await fetchBestLyrics(safeLyricsConfig(), buildQuery(track))
    if (!result || !result.content) {
      logger.debug('No online lyrics found')
      return null
    }
    return { content: result.content, format: result.format }
  } catch (error) {
    logger.error('Failed to fetch online lyrics:', error)
    sharedOnlineLyricsError.value = (error as Error).message
    return null
  }
}

/** 保存歌词到本地,按格式选扩展名 (ASS 逐字歌词 / LRC 逐行歌词) */
async function saveLyricsToLocal(
  trackPath: string,
  content: string,
  format: 'lrc' | 'ass',
): Promise<boolean> {
  if (!trackPath || !content) return false
  try {
    const baseName = FileUtils.getFileNameWithoutExtension(trackPath)
    const directory = FileUtils.getDirectoryPath(trackPath)
    const ext = format === 'ass' ? 'ass' : 'lrc'
    const lyricsPath = FileUtils.joinPath(directory, `${baseName}.${ext}`)
    await invoke('write_lyrics_file', { path: lyricsPath, content })
    logger.info('Lyrics saved to: ' + lyricsPath)
    return true
  } catch (error) {
    logger.error('Failed to save lyrics:', error)
    return false
  }
}

/**
 * 加载歌词:本地文件优先,缺失时按配置在线获取
 *
 * 写入统一走 _playerStore.lyrics,时机见 initializeSharedWatchers 中的 store.lyrics watcher
 */
async function loadLyrics(trackPath: string | undefined): Promise<void> {
  if (!_playerStore || !_configStore) return
  // 序号守卫:快速切歌时请求并发,只有最新一次的结果允许写入共享状态
  const seq = _playerStore.beginLyricsRequest()
  if (!trackPath) {
    _playerStore.lyrics = null
    sharedLyricsSource.value = 'local'
    sharedOnlineLyricsError.value = null
    return
  }

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
      // markRaw: 歌词行只整体替换、不改内部字段,无需深度响应式代理
      const parsed = markRaw(await LyricsParser.parseAsync(content, ext))
      if (!_playerStore.isLyricsRequestCurrent(seq)) return
      _playerStore.lyrics = parsed
      sharedLyricsSource.value = 'local'
    } else if (_configStore.lyrics?.enableOnlineFetch) {
      logger.debug('No local lyrics found, trying online fetch...')
      const track = _playerStore.currentTrack
      const onlineLyrics = await fetchOnlineLyrics(track)
      if (!_playerStore.isLyricsRequestCurrent(seq)) return
      if (onlineLyrics) {
        // markRaw: 同上
        const parsed = markRaw(
          await LyricsParser.parseAsync(onlineLyrics.content, onlineLyrics.format),
        )
        if (!_playerStore.isLyricsRequestCurrent(seq)) return
        _playerStore.lyrics = parsed
        sharedLyricsSource.value = 'online'

        onlineLyricsCache.set(trackPath, {
          content: onlineLyrics.content,
          format: onlineLyrics.format,
          parsed,
          source: 'online',
        })

        if (_configStore.lyrics?.autoSaveOnlineLyrics) {
          const saved = await saveLyricsToLocal(
            trackPath,
            onlineLyrics.content,
            onlineLyrics.format,
          )
          if (saved && _playerStore.isLyricsRequestCurrent(seq)) {
            sharedLyricsSource.value = 'local'
            // 缓存里存的是"仅在线可得"的歌词,落盘后本地文件才是来源,故清掉
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
    // 过期请求不得改动 loading 状态,否则会过早关掉新请求的 loading (序号守卫)
    if (_playerStore.isLyricsRequestCurrent(seq)) {
      sharedLoading.value = false
    }
  }
}

/**
 * 初始化共享 watcher (只执行一次)
 *
 * LyricsDisplay / VisualizerPanel / App.vue 共用同一套 watcher 与状态,
 * 否则切歌会触发 3 次 loadLyrics、每帧 3 次二分查找
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
  // 都经此 watcher 同步到 sharedLyrics,保证两个状态视图一致;flush: 'sync' 与直接赋值时机相同
  const stopWatchStoreLyrics = watch(
    () => _playerStore!.lyrics,
    (lyrics) => {
      sharedLyrics.value = lyrics ?? []
    },
    { immediate: true, flush: 'sync' },
  )
  sharedWatchStopFns.push(stopWatchStoreLyrics)

  // activeIndex 节流:currentTime 变化很频繁,高亮行只需 100ms 精度
  let lastActiveIndexUpdate = 0
  const ACTIVE_INDEX_THROTTLE = 100

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

      const now = Date.now()
      if (now - lastActiveIndexUpdate < ACTIVE_INDEX_THROTTLE) return
      lastActiveIndexUpdate = now

      // 单位是秒:store 的 currentTime / lyricsOffset 与 LyricLine.time 一致
      const offset = _playerStore!.lyricsOffset || 0
      const adjustedTime = currentTime - offset

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
  initializeSharedWatchers()

  const playerStore = usePlayerStore()
  const configStore = useConfigStore()

  /** 手动"获取歌词"：按配置顺延取最优并落盘。
   *  target 省略时作用于当前播放曲目；挑选弹窗会传入它打开时锁定的那首歌，
   *  目标已不是当前曲目时只落盘，不碰在屏歌词与共享加载状态 */
  const fetchAndSaveLyrics = async (target?: Track | null): Promise<boolean> => {
    const track = target ?? playerStore.currentTrack
    if (!track) return false
    const isCurrent = playerStore.currentTrack?.path === track.path
    // 序号守卫:手动刷新也纳入统一计数 (见 loadLyrics)
    const seq = isCurrent ? playerStore.beginLyricsRequest() : NO_LYRICS_REQUEST
    if (isCurrent) {
      sharedLoading.value = true
      sharedOnlineLyricsError.value = null
    }
    try {
      const onlineLyrics = await fetchOnlineLyrics(track)
      if (isCurrent && !playerStore.isLyricsRequestCurrent(seq)) return false
      if (onlineLyrics) {
        // markRaw: 见 loadLyrics
        const parsed = markRaw(
          await LyricsParser.parseAsync(onlineLyrics.content, onlineLyrics.format),
        )
        if (isCurrent && !playerStore.isLyricsRequestCurrent(seq)) return false
        if (isCurrent) {
          playerStore.lyrics = parsed
          sharedLyricsSource.value = 'online'
        }

        onlineLyricsCache.set(track.path, {
          content: onlineLyrics.content,
          format: onlineLyrics.format,
          parsed,
          source: 'online',
        })

        if (configStore.lyrics?.autoSaveOnlineLyrics) {
          const saved = await saveLyricsToLocal(
            track.path,
            onlineLyrics.content,
            onlineLyrics.format,
          )
          if (saved && isCurrent && playerStore.isLyricsRequestCurrent(seq)) {
            sharedLyricsSource.value = 'local'
            // 落盘后清掉在线缓存,理由见 loadLyrics
            onlineLyricsCache.delete(track.path)
          }
        }
        return true
      }
      return false
    } catch (e) {
      logger.error('Error fetching lyrics:', e)
      if (isCurrent && playerStore.isLyricsRequestCurrent(seq)) {
        sharedOnlineLyricsError.value = (e as Error).message
      }
      return false
    } finally {
      if (isCurrent && playerStore.isLyricsRequestCurrent(seq)) {
        sharedLoading.value = false
      }
    }
  }

  // 聚合各启用来源的候选歌词（供手动挑选弹窗使用）
  //
  // 不参与歌词显示序号：候选绑定的是弹窗打开时锁定的那首歌，之后切歌不该让这批结果作废；
  // 更要避免顺手把当前曲目正在进行的歌词加载判成过期 —— 那会让打开弹窗就歌词空白
  const fetchCandidates = async (track: Track | null): Promise<LyricCandidate[]> => {
    if (!track) return []
    try {
      return await collectCandidates(safeLyricsConfig(), buildQuery(track))
    } catch (e) {
      logger.error('Error collecting lyric candidates:', e)
      return []
    }
  }

  // 应用用户挑选的候选歌词：显示 + 可选写入本地文件
  //
  // target 是弹窗打开时锁定的曲目：候选内容属于它，落盘路径也必须是它。
  // 若用户中途切了歌，仍然只写那首歌的歌词文件，不覆盖在屏歌词、不作废新曲目的加载请求
  const applyCandidate = async (
    candidate: LyricCandidate,
    kind: LyricKind,
    target: Track | null,
  ): Promise<boolean> => {
    const track = target ?? playerStore.currentTrack
    if (!track || !configStore) return false
    const isCurrent = playerStore.currentTrack?.path === track.path
    const seq = isCurrent ? playerStore.beginLyricsRequest() : NO_LYRICS_REQUEST
    try {
      const final = buildFinalLyric(
        candidate.bundle,
        kind,
        configStore.lyrics?.preferTranslation ?? true,
      )
      if (!final.content) return false
      const parsed = markRaw(await LyricsParser.parseAsync(final.content, final.format))
      if (isCurrent && !playerStore.isLyricsRequestCurrent(seq)) return false
      if (isCurrent) {
        playerStore.lyrics = parsed
        sharedLyricsSource.value = 'online'
      }

      onlineLyricsCache.set(track.path, {
        content: final.content,
        format: final.format,
        parsed,
        source: 'online',
      })

      if (configStore.lyrics?.autoSaveOnlineLyrics) {
        const saved = await saveLyricsToLocal(track.path, final.content, final.format)
        if (saved && isCurrent && playerStore.isLyricsRequestCurrent(seq)) {
          sharedLyricsSource.value = 'local'
          onlineLyricsCache.delete(track.path)
        }
      }
      return true
    } catch (e) {
      logger.error('Error applying lyric candidate:', e)
      return false
    }
  }

  // cleanup 供显式全量清理 (如 HMR 重建 store 时手动调用):停 watcher、复位标记、清空共享状态
  // 不要在单个组件 onUnmounted 里调用,那会停掉其他调用方共享的 watcher
  const cleanup = (): void => {
    // 作废所有进行中的歌词请求 (过期结果由序号守卫丢弃)
    _playerStore?.beginLyricsRequest()
    sharedWatchStopFns.forEach((fn) => fn())
    sharedWatchStopFns.length = 0
    isInitialized = false
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
    fetchCandidates,
    applyCandidate,
    loadLyrics,
    cleanup,
  }
}
