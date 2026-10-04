/**
 * 播放统计插件:数据分三层(单曲 tracks / 每日 daily / 明细 history)
 * 阈值与口径见下方常量,由 tests/plugins/playCount.test.ts 守护
 */

import i18n from '@/i18n'
import { PluginPermission, type PluginAPI, type BuiltinPluginDefinition } from '../pluginManager'
import type { Track } from '@/types'

/** 持久化数据结构版本,用于旧数据迁移 */
const SCHEMA_VERSION = 2
/** 计入播放次数的最短收听时长(秒) */
const MIN_COUNTED_SECONDS = 30
/** 收听覆盖曲目时长的该比例即视为完播 */
const COMPLETE_RATIO = 0.9
/** 曲目时长未知时的完播兜底阈值(秒) */
const COMPLETE_FALLBACK_SECONDS = 300
/** 播放明细保留条数 */
export const HISTORY_LIMIT = 500
/** 每日聚合保留天数 */
const DAILY_RETENTION_DAYS = 400
/** 单曲聚合保留上限,超出后优先淘汰零计次的曲目 */
const MAX_TRACK_AGGREGATES = 3000
/** 单次结算最多计入的收听时长(秒),防止挂后台等异常场景写入离谱数值 */
const MAX_SESSION_SECONDS = 6 * 60 * 60
/** 轮询间隔(毫秒),需足够短以命中"播完自动切歌"的结算时机 */
const POLL_INTERVAL_MS = 3000
/** 未落盘时长达到该秒数时先做一次分段提交,避免异常退出丢掉整段 */
const CHECKPOINT_SECONDS = 300
/** 播放位置相对上一次回退超过该秒数时,视为新一轮播放(单曲循环或手动重播) */
const REWIND_THRESHOLD_SECONDS = 30
/** 单次会话低于该秒数视为误触,不写入统计 */
const MIN_SETTLE_SECONDS = 1
/** 趋势图默认跨度(天) */
export const TREND_DEFAULT_DAYS = 14

/** 已归档的旧版播放历史条目(v1.0 / v1.1) */
interface LegacyHistoryEntry {
  path?: unknown
  title?: unknown
  artist?: unknown
  timestamp?: unknown
}

/** 单条播放明细:落盘按时间升序(存储超限时宿主只保数组尾部),读取时倒序返回 */
export interface PlayHistoryEntry {
  path: string
  timestamp: number
  /** 本次实际收听秒数 */
  listenedSeconds: number
  /** 曲目总时长(秒),0 表示未知 */
  trackSeconds: number
  /** 本次是否完播 */
  completed: boolean
  /** 本次是否计入播放次数 */
  counted: boolean
  /** 曲名,读取时补全 */
  title: string
  /** 歌手,读取时补全 */
  artist: string
}

/** 落盘用的明细条目:不含 title / artist,避免与 tracks 重复占用存储 */
type StoredHistoryEntry = Omit<PlayHistoryEntry, 'title' | 'artist'>

interface TrackAggregate {
  plays: number
  seconds: number
  completed: number
  /** 参与完播判定的次数(等于计次次数),旧数据为 0 */
  judged: number
  title: string
  artist: string
  /** 曲目时长(秒),0 表示未知 */
  duration: number
  firstPlayedAt: number
  lastPlayedAt: number
}

interface DailyStat {
  plays: number
  seconds: number
  completed: number
}

interface PlayCountData {
  version: number
  tracks: Record<string, TrackAggregate>
  history: StoredHistoryEntry[]
  daily: Record<string, DailyStat>
  /** 累计收听时长(秒),包含不足一次计次门槛的收听 */
  totalPlayTime: number
}

/** 概览统计(供播放统计页面使用) */
export interface PlayCountStats {
  totalTracks: number
  totalPlays: number
  totalPlayTime: number
  totalPlayTimeFormatted: string
  completedPlays: number
  /** 参与完播判定的播放次数,0 表示暂无判定样本 */
  judgedPlays: number
  /** 完播率(0~1),无可判定样本时为 null */
  completionRate: number | null
  /** 平均每首/每次收听时长(秒) */
  averageSeconds: number
  averageSecondsFormatted: string
  /** 当前统计范围内的活跃天数 */
  activeDays: number
  /** 连续收听天数(含今天或截至昨天) */
  currentStreak: number
  /** 历史最长连续收听天数 */
  longestStreak: number
  todayPlays: number
  todaySeconds: number
  distinctArtists: number
  topArtist: { name: string; plays: number } | null
  firstPlayedAt: number
  lastPlayedAt: number
  /** 本次统计的时间范围(天),null 表示全部 */
  rangeDays: number | null
  /** 明细窗口是否已满,范围筛选可能不完整 */
  detailTruncated: boolean
}

export interface TopTrack {
  path: string
  title: string
  artist: string
  plays: number
  seconds: number
  secondsFormatted: string
  completed: number
  judged: number
  /** 完播率,口径见 PlayCountStats.completionRate */
  completionRate: number | null
  duration: number
  lastPlayedAt: number
}

export interface DailyPoint {
  /** YYYY-MM-DD */
  date: string
  plays: number
  seconds: number
  completed: number
}

export type TopTrackSortBy = 'plays' | 'seconds' | 'recent'

export interface TopTrackQuery {
  limit?: number
  sortBy?: TopTrackSortBy
  /** 统计范围(天),null / 不传表示全部 */
  days?: number | null
}

/** 取本地时区的 YYYY-MM-DD */
const dayKey = (timestamp: number): string => {
  const date = new Date(timestamp)
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${date.getFullYear()}-${month}-${day}`
}

/** 取某个日期键偏移若干天后的日期键与当天 0 点时间戳 */
const dayKeyOffset = (key: string, delta: number): { key: string; startAt: number } => {
  const parts = key.split('-').map((part) => Number(part))
  const date = new Date(parts[0] ?? 1970, (parts[1] ?? 1) - 1, parts[2] ?? 1)
  date.setHours(0, 0, 0, 0)
  date.setDate(date.getDate() + delta)
  const startAt = date.getTime()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return { key: `${date.getFullYear()}-${month}-${day}`, startAt }
}

/** 最近 N 天的日期键(升序,含今天) */
const recentDayKeys = (days: number): string[] => {
  const today = dayKey(Date.now())
  const keys: string[] = []
  for (let offset = days - 1; offset >= 0; offset -= 1) {
    keys.push(dayKeyOffset(today, -offset).key)
  }
  return keys
}

/** 格式化收听时长(秒 -> 本地化文案),页面侧复用同一口径 */
export function formatDuration(seconds: number): string {
  const safe = Math.max(0, Math.round(seconds))
  const hours = Math.floor(safe / 3600)
  const minutes = Math.floor((safe % 3600) / 60)
  const secs = safe % 60

  if (hours > 0) {
    return i18n.global.t('player.durationHm', { hours, minutes })
  }
  if (minutes > 0) {
    return i18n.global.t('player.durationM', { minutes })
  }
  return i18n.global.t('player.durationS', { seconds: secs })
}

/** 计次口径:满最短收听时长,或覆盖 COMPLETE_RATIO 的曲目时长 */
const meetsCountThreshold = (listened: number, trackSeconds: number): boolean =>
  listened >= MIN_COUNTED_SECONDS || (trackSeconds > 0 && listened >= trackSeconds * COMPLETE_RATIO)

const meetsCompleteThreshold = (listened: number, trackSeconds: number): boolean =>
  trackSeconds > 0
    ? Math.min(listened, trackSeconds) >= trackSeconds * COMPLETE_RATIO
    : listened >= COMPLETE_FALLBACK_SECONDS

/** 播放位置距曲目末尾 2 秒内即视为听完 */
const isTrackFinished = (position: number, trackSeconds: number): boolean =>
  trackSeconds > 0 && position >= trackSeconds - 2

/** 完播率:无可判定样本时返回 null,而不是误导性的 0 */
const toCompletionRate = (completed: number, judged: number): number | null =>
  judged > 0 ? Math.min(1, completed / judged) : null

export const playCountPlugin: BuiltinPluginDefinition = {
  id: 'builtin-play-count',
  name: i18n.global.t('plugin.playCountName'),
  version: '1.2.2',
  author: 'Mercurial Player',
  description: i18n.global.t('plugin.playCountDescription'),
  permissions: [PluginPermission.PLAYER_READ, PluginPermission.STORAGE],

  main: (api: PluginAPI) => {
    // 当前播放会话状态:仅内存,不落盘
    let lastTrack: Track | null = null
    /** 本轮"未提交分段"的起点,暂停时置空 */
    let playStartTime: number | null = null
    /** 尚未提交的分段时长(秒) */
    let accumulatedPlayTime = 0
    /** 当前曲目会话的累计收听时长(含已提交分段) */
    let sessionListened = 0
    /** 当前曲目时长(秒),来自播放器状态或曲目元信息 */
    let sessionDuration = 0
    /** 上一次轮询到的播放位置,用于识别回退 */
    let lastPosition = 0
    let sessionCounted = false
    let sessionCompleted = false

    let pollingInterval: ReturnType<typeof setInterval> | null = null

    // off 按引用移除监听,故须长期持有这两个回调引用,否则停用时订阅泄漏
    let trackChangedCallback: (data: unknown) => void
    let stateChangedCallback: (data: unknown) => void

    const emptyAggregate = (): TrackAggregate => ({
      plays: 0,
      seconds: 0,
      completed: 0,
      judged: 0,
      title: '',
      artist: '',
      duration: 0,
      firstPlayedAt: 0,
      lastPlayedAt: 0,
    })

    const emptyData = (): PlayCountData => ({
      version: SCHEMA_VERSION,
      tracks: {},
      history: [],
      daily: {},
      totalPlayTime: 0,
    })

    /**
     * 把旧版(1.0 / 1.1)数据转换成 v2 结构:纯函数,不落盘,插件未激活也能看到迁移结果
     *
     * 旧版只有播放次数与总时长:plays 沿用旧计数,seconds/judged 保持 0,元信息与每日次数由旧历史回填
     */
    const migrateLegacyData = (): PlayCountData => {
      const legacyCounts = api.storage.get<Record<string, number>>('playCounts', {}) ?? {}
      const legacyHistory = api.storage.get<LegacyHistoryEntry[]>('playHistory', []) ?? []
      const legacyTotalTime = api.storage.get<number>('totalPlayTime', 0) ?? 0

      const data = emptyData()
      data.totalPlayTime =
        Number.isFinite(legacyTotalTime) && legacyTotalTime > 0 ? legacyTotalTime : 0

      for (const [path, count] of Object.entries(legacyCounts)) {
        if (!path) continue
        const aggregate = emptyAggregate()
        aggregate.plays = Number.isFinite(count) ? Math.max(0, count) : 0
        aggregate.judged = 0
        data.tracks[path] = aggregate
      }

      for (const raw of legacyHistory) {
        const path = typeof raw?.path === 'string' ? raw.path : ''
        if (!path) continue

        const timestamp = Number(raw.timestamp) || 0
        const title = typeof raw.title === 'string' ? raw.title : ''
        const artist = typeof raw.artist === 'string' ? raw.artist : ''

        data.history.push({
          path,
          timestamp,
          listenedSeconds: 0,
          trackSeconds: 0,
          completed: false,
          counted: true,
        })

        const aggregate = data.tracks[path]
        if (aggregate) {
          if (!aggregate.title && title) aggregate.title = title
          if (!aggregate.artist && artist) aggregate.artist = artist
          if (timestamp > aggregate.lastPlayedAt) aggregate.lastPlayedAt = timestamp
          if (
            aggregate.firstPlayedAt === 0 ||
            (timestamp > 0 && timestamp < aggregate.firstPlayedAt)
          ) {
            aggregate.firstPlayedAt = timestamp
          }
        }

        if (timestamp > 0) {
          const key = dayKey(timestamp)
          const day = data.daily[key] ?? { plays: 0, seconds: 0, completed: 0 }
          // 旧历史只保留最近 100 条,按条数回填次数是唯一可用的近似
          day.plays += 1
          data.daily[key] = day
        }
      }

      data.history.sort((a, b) => a.timestamp - b.timestamp)
      data.history = data.history.slice(-HISTORY_LIMIT)
      return data
    }

    /** 读取数据;版本落后时返回 migrateLegacyData 的内存结果,不写盘 */
    const loadData = (): PlayCountData => {
      const version = api.storage.get<number>('version', 0) ?? 0
      if (version < SCHEMA_VERSION) {
        return migrateLegacyData()
      }
      const tracks = api.storage.get<Record<string, TrackAggregate>>('tracks', {}) ?? {}
      const history = api.storage.get<StoredHistoryEntry[]>('history', []) ?? []
      const daily = api.storage.get<Record<string, DailyStat>>('daily', {}) ?? {}
      const totalPlayTime = api.storage.get<number>('totalPlayTime', 0) ?? 0
      return {
        version: SCHEMA_VERSION,
        tracks,
        history: Array.isArray(history) ? history : [],
        daily,
        totalPlayTime: Number.isFinite(totalPlayTime) ? totalPlayTime : 0,
      }
    }

    const saveData = (data: PlayCountData): void => {
      api.storage.set('version', SCHEMA_VERSION)
      api.storage.set('tracks', data.tracks)
      api.storage.set('history', data.history)
      api.storage.set('daily', data.daily)
      api.storage.set('totalPlayTime', data.totalPlayTime)
    }

    /** 首次激活才把迁移结果落盘,并清掉废弃旧键 playCounts / playHistory */
    const persistMigrationIfNeeded = (): void => {
      const version = api.storage.get<number>('version', 0) ?? 0
      if (version >= SCHEMA_VERSION) return

      saveData(migrateLegacyData())
      api.storage.remove('playCounts')
      api.storage.remove('playHistory')
      api.log.info('播放统计数据已升级到 v2 结构')
    }

    const ensureAggregate = (data: PlayCountData, path: string): TrackAggregate => {
      const existing = data.tracks[path]
      if (existing) return existing
      const created = emptyAggregate()
      data.tracks[path] = created
      return created
    }

    const ensureDay = (data: PlayCountData, timestamp: number): DailyStat => {
      const key = dayKey(timestamp)
      const existing = data.daily[key]
      if (existing) return existing
      const created = { plays: 0, seconds: 0, completed: 0 }
      data.daily[key] = created
      return created
    }

    /** 容量裁剪:日期保留窗口 + 曲目聚合上限,淘汰优先级见两个常量 */
    const pruneData = (data: PlayCountData): void => {
      const dayKeys = Object.keys(data.daily).sort()
      if (dayKeys.length > DAILY_RETENTION_DAYS) {
        for (const key of dayKeys.slice(0, dayKeys.length - DAILY_RETENTION_DAYS)) {
          delete data.daily[key]
        }
      }

      if (Object.keys(data.tracks).length <= MAX_TRACK_AGGREGATES) return
      const droppable = Object.keys(data.tracks)
        .filter((path) => (data.tracks[path]?.plays ?? 0) === 0)
        .sort((a, b) => (data.tracks[a]?.lastPlayedAt ?? 0) - (data.tracks[b]?.lastPlayedAt ?? 0))
      for (const path of droppable) {
        if (Object.keys(data.tracks).length <= MAX_TRACK_AGGREGATES) break
        delete data.tracks[path]
      }
    }

    /** 尚未落盘的时长(秒) */
    const pendingSeconds = (): number =>
      accumulatedPlayTime + (playStartTime ? (Date.now() - playStartTime) / 1000 : 0)

    /** 实时可展示的进行中时长(秒) */
    const liveSeconds = (): number => Math.min(pendingSeconds(), MAX_SESSION_SECONDS)

    const startSegment = (): void => {
      playStartTime = Date.now()
    }

    const pauseSegment = (): void => {
      if (playStartTime) {
        accumulatedPlayTime += (Date.now() - playStartTime) / 1000
        playStartTime = null
      }
    }

    /** 只累计收听秒数,不判定计次与完播 */
    const commitTime = (track: Track | null, seconds: number): void => {
      if (seconds <= 0) return
      const data = loadData()
      data.totalPlayTime += seconds

      const path = track?.path
      if (path) {
        const aggregate = ensureAggregate(data, path)
        aggregate.seconds += seconds
        aggregate.lastPlayedAt = Math.max(aggregate.lastPlayedAt, Date.now())
      }
      ensureDay(data, Date.now()).seconds += seconds
      saveData(data)
    }

    /** 落盘判定结果(判定口径见 settleSegment):计次与完播次数,加一条明细 */
    const commitOutcome = (
      track: Track,
      listenedSeconds: number,
      trackSeconds: number,
      counted: boolean,
      completed: boolean,
    ): void => {
      const path = track.path
      if (!path) return

      const data = loadData()
      const aggregate = ensureAggregate(data, path)
      const title = (track.title || track.displayTitle || track.name || '') as string
      const artist = (track.artist || track.displayArtist || '') as string
      const now = Date.now()

      if (title) aggregate.title = title
      if (artist) aggregate.artist = artist
      if (trackSeconds > 0) aggregate.duration = trackSeconds
      if (counted) {
        aggregate.plays += 1
        aggregate.judged += 1
      }
      if (completed) aggregate.completed += 1
      if (aggregate.firstPlayedAt === 0) aggregate.firstPlayedAt = now
      aggregate.lastPlayedAt = now

      const day = ensureDay(data, now)
      if (counted) day.plays += 1
      if (completed) day.completed += 1

      // 新记录追加末尾,维持 PlayHistoryEntry 的升序约定
      data.history.push({
        path,
        timestamp: now,
        listenedSeconds,
        trackSeconds,
        completed,
        counted,
      })
      if (data.history.length > HISTORY_LIMIT) {
        data.history = data.history.slice(-HISTORY_LIMIT)
      }

      pruneData(data)
      saveData(data)
      api.log.debug(
        `播放记录: ${title || path} - 收听 ${Math.round(listenedSeconds)} 秒` +
          `${counted ? ' · 计次' : ' · 未达计次门槛'}${completed ? ' · 完播' : ''}`,
      )
    }

    /**
     * 结算当前分段:未提交时长写入聚合并计入会话累计
     *
     * 计次/完播按会话累计判定,只在会话结束时经 commitOutcome 落盘,故分段提交不会把一次收听拆成多次播放
     */
    const settleSegment = (): void => {
      pauseSegment()
      const segment = Math.min(accumulatedPlayTime, MAX_SESSION_SECONDS)
      accumulatedPlayTime = 0
      if (segment < MIN_SETTLE_SECONDS) return

      sessionListened += segment
      commitTime(lastTrack, segment)

      if (!sessionCounted && meetsCountThreshold(sessionListened, sessionDuration)) {
        sessionCounted = true
      }
      if (!sessionCompleted && meetsCompleteThreshold(sessionListened, sessionDuration)) {
        sessionCompleted = true
      }
    }

    /** 结束当前曲目会话:判定结果经 commitOutcome 落盘,并重置会话状态 */
    const endSession = (): void => {
      const track = lastTrack
      if (track?.path && sessionListened > 0) {
        commitOutcome(
          track,
          sessionListened,
          sessionDuration,
          sessionCounted,
          sessionCompleted && sessionCounted,
        )
      }
      sessionListened = 0
      sessionCounted = false
      sessionCompleted = false
      sessionDuration = 0
      accumulatedPlayTime = 0
    }

    const handlePlaybackState = (
      track: Track | null,
      isPlaying: boolean,
      position: number,
      duration: number,
    ): void => {
      const trackPath = track?.path ?? null
      const lastPath = lastTrack?.path ?? null
      const trackSeconds = duration > 0 ? duration : (track?.duration ?? 0)

      if (trackPath !== lastPath) {
        settleSegment()
        endSession()
        lastTrack = track
        sessionDuration = trackSeconds > 0 ? trackSeconds : 0
        lastPosition = position
        if (track && isPlaying) startSegment()
        return
      }

      if (trackSeconds > 0) sessionDuration = trackSeconds

      if (isPlaying && position + REWIND_THRESHOLD_SECONDS < lastPosition) {
        settleSegment()
        endSession()
        lastPosition = position
        startSegment()
        return
      }

      lastPosition = position

      if (isPlaying && track) {
        if (!playStartTime) startSegment()
        // 分段提交后立即续上计时,否则会漏掉结算瞬间这一个轮询间隔
        if (pendingSeconds() >= CHECKPOINT_SECONDS) {
          settleSegment()
          startSegment()
        }
        return
      }

      // 暂停只结算分段,不结束会话:继续播放要在同一轮里累计
      const finished = isTrackFinished(position, sessionDuration)
      settleSegment()
      // 位置到底即归档,否则本轮要等到下次切歌或退出应用才计数
      if (finished) endSession()
    }

    // 暂停/空闲时计时不推进,轮询无需唤醒
    let lastObservedPlaying = false

    const pollPlayerState = async (): Promise<void> => {
      try {
        const state = api.player.getState()
        lastObservedPlaying = state.isPlaying
        handlePlaybackState(state.currentTrack, state.isPlaying, state.currentTime, state.duration)
      } catch {
        // 播放器尚未就绪时忽略本轮轮询
      }
    }

    interface ScopeSummary {
      totalTracks: number
      totalPlays: number
      totalSeconds: number
      completedPlays: number
      judgedPlays: number
      distinctArtists: number
      topArtist: { name: string; plays: number } | null
      firstPlayedAt: number
      lastPlayedAt: number
      detailTruncated: boolean
    }

    const pickTopArtist = (
      artistPlays: Map<string, number>,
    ): { name: string; plays: number } | null => {
      let best: { name: string; plays: number } | null = null
      for (const [name, plays] of artistPlays) {
        if (!best || plays > best.plays) best = { name, plays }
      }
      return best
    }

    /**
     * 汇总指定范围的数据:days === null 取单曲聚合(含旧版迁移,数值最完整)
     *
     * days > 0 取每日聚合(不受明细窗口限制),但完播与判定次数仍取自明细,旧版每日聚合没有可判定次数
     */
    const summarizeScope = (data: PlayCountData, days: number | null): ScopeSummary => {
      const live = liveSeconds()

      if (days === null) {
        let totalTracks = 0
        let totalPlays = 0
        let completedPlays = 0
        let judgedPlays = 0
        let firstPlayedAt = 0
        let lastPlayedAt = 0
        const artistPlays = new Map<string, number>()

        for (const aggregate of Object.values(data.tracks)) {
          completedPlays += aggregate.completed
          judgedPlays += aggregate.judged
          // 播放过的歌曲只统计计次曲目,试听十几秒就切走的不算
          if (aggregate.plays > 0) {
            totalTracks += 1
            totalPlays += aggregate.plays
            if (aggregate.artist) {
              artistPlays.set(
                aggregate.artist,
                (artistPlays.get(aggregate.artist) ?? 0) + aggregate.plays,
              )
            }
          }
          if (
            aggregate.firstPlayedAt > 0 &&
            (firstPlayedAt === 0 || aggregate.firstPlayedAt < firstPlayedAt)
          ) {
            firstPlayedAt = aggregate.firstPlayedAt
          }
          if (aggregate.lastPlayedAt > lastPlayedAt) lastPlayedAt = aggregate.lastPlayedAt
        }

        // 总收听时长以全局计数器为准:单曲秒数在旧版数据里没有,求和会漏掉历史时长
        return {
          totalTracks,
          totalPlays,
          totalSeconds: data.totalPlayTime + live,
          completedPlays,
          judgedPlays,
          distinctArtists: artistPlays.size,
          topArtist: pickTopArtist(artistPlays),
          firstPlayedAt,
          lastPlayedAt: live > 0 ? Date.now() : lastPlayedAt,
          detailTruncated: false,
        }
      }

      const keys = recentDayKeys(days)
      let totalPlays = 0
      let totalSeconds = 0
      for (const key of keys) {
        const day = data.daily[key]
        if (!day) continue
        totalPlays += day.plays
        totalSeconds += day.seconds
      }

      const windowStart = keys.length > 0 ? dayKeyOffset(keys[0]!, 0).startAt : 0
      const paths = new Set<string>()
      const artistPlays = new Map<string, number>()
      let completedPlays = 0
      let judgedPlays = 0
      let firstPlayedAt = 0
      let lastPlayedAt = 0

      for (const entry of data.history) {
        if (entry.timestamp < windowStart) continue
        if (entry.counted) paths.add(entry.path)
        if (entry.counted) judgedPlays += 1
        if (entry.completed) completedPlays += 1
        if (firstPlayedAt === 0 || entry.timestamp < firstPlayedAt) firstPlayedAt = entry.timestamp
        if (entry.timestamp > lastPlayedAt) lastPlayedAt = entry.timestamp

        const artist = data.tracks[entry.path]?.artist
        if (artist)
          artistPlays.set(artist, (artistPlays.get(artist) ?? 0) + (entry.counted ? 1 : 0))
      }

      return {
        totalTracks: paths.size,
        totalPlays,
        totalSeconds: totalSeconds + live,
        completedPlays,
        judgedPlays,
        distinctArtists: artistPlays.size,
        topArtist: pickTopArtist(artistPlays),
        firstPlayedAt,
        lastPlayedAt: live > 0 ? Date.now() : lastPlayedAt,
        detailTruncated: data.history.length >= HISTORY_LIMIT,
      }
    }

    const computeStreaks = (data: PlayCountData): { current: number; longest: number } => {
      const activeKeys = Object.keys(data.daily)
        .filter((key) => (data.daily[key]?.plays ?? 0) > 0 || (data.daily[key]?.seconds ?? 0) > 0)
        .sort()
      if (activeKeys.length === 0) return { current: 0, longest: 0 }

      let longest = 1
      let run = 1
      for (let index = 1; index < activeKeys.length; index += 1) {
        const previous = activeKeys[index - 1]!
        const current = activeKeys[index]!
        const expected = dayKeyOffset(previous, 1).key
        run = current === expected ? run + 1 : 1
        if (run > longest) longest = run
      }

      // 连续天数含今天或截至昨天,见 PlayCountStats.currentStreak
      const today = dayKey(Date.now())
      const yesterdayKey = dayKeyOffset(today, -1).key
      const last = activeKeys[activeKeys.length - 1]!
      if (last !== today && last !== yesterdayKey) return { current: 0, longest }

      let current = 1
      for (let index = activeKeys.length - 1; index > 0; index -= 1) {
        const previous = activeKeys[index - 1]!
        const following = activeKeys[index]!
        if (dayKeyOffset(previous, 1).key !== following) break
        current += 1
      }
      return { current, longest }
    }

    const buildTopTrack = (
      path: string,
      data: PlayCountData,
      values: {
        plays: number
        seconds: number
        completed: number
        judged: number
        lastPlayedAt: number
      },
      duration: number,
    ): TopTrack => ({
      path,
      title: data.tracks[path]?.title ?? '',
      artist: data.tracks[path]?.artist ?? '',
      plays: values.plays,
      seconds: values.seconds,
      secondsFormatted: formatDuration(values.seconds),
      completed: values.completed,
      judged: values.judged,
      completionRate: toCompletionRate(values.completed, values.judged),
      duration: duration > 0 ? duration : (data.tracks[path]?.duration ?? 0),
      lastPlayedAt: values.lastPlayedAt,
    })

    return {
      async activate(): Promise<void> {
        api.log.info('播放统计插件已激活')
        persistMigrationIfNeeded()

        // 事件只负责"唤醒"轮询:位置与时长需要完整状态,统一从 getState 读取
        trackChangedCallback = () => {
          void pollPlayerState()
        }

        stateChangedCallback = () => {
          void pollPlayerState()
        }

        api.events.on('player:trackChanged', trackChangedCallback)
        api.events.on('player:stateChanged', stateChangedCallback)

        await pollPlayerState()
        pollingInterval = setInterval(() => {
          if (!lastObservedPlaying) return
          void pollPlayerState()
        }, POLL_INTERVAL_MS)
      },

      deactivate(): void {
        // 停用时把当前会话结算并归档,避免丢掉最后一段收听
        settleSegment()
        endSession()

        if (trackChangedCallback) {
          api.events.off('player:trackChanged', trackChangedCallback)
        }
        if (stateChangedCallback) {
          api.events.off('player:stateChanged', stateChangedCallback)
        }

        if (pollingInterval) {
          clearInterval(pollingInterval)
          pollingInterval = null
        }

        lastTrack = null
        playStartTime = null
        accumulatedPlayTime = 0
        lastPosition = 0
        api.log.info('播放统计插件已停用')
      },

      getStats(rangeDays: number | null = null): PlayCountStats {
        const data = loadData()
        const live = liveSeconds()
        const summary = summarizeScope(data, rangeDays)
        const streaks = computeStreaks(data)

        const todayKey = dayKey(Date.now())
        const today = data.daily[todayKey]
        const todaySeconds = (today?.seconds ?? 0) + live
        const todayPlays = today?.plays ?? 0

        const averageSeconds =
          summary.totalPlays > 0 ? summary.totalSeconds / summary.totalPlays : 0

        return {
          totalTracks: summary.totalTracks,
          totalPlays: summary.totalPlays,
          totalPlayTime: summary.totalSeconds,
          totalPlayTimeFormatted: formatDuration(summary.totalSeconds),
          completedPlays: summary.completedPlays,
          judgedPlays: summary.judgedPlays,
          completionRate: toCompletionRate(summary.completedPlays, summary.judgedPlays),
          averageSeconds,
          averageSecondsFormatted: summary.totalPlays > 0 ? formatDuration(averageSeconds) : '--',
          activeDays:
            rangeDays === null
              ? Object.keys(data.daily).length
              : recentDayKeys(rangeDays).filter((key) => (data.daily[key]?.seconds ?? 0) > 0)
                  .length,
          currentStreak: streaks.current,
          longestStreak: streaks.longest,
          todayPlays,
          todaySeconds,
          distinctArtists: summary.distinctArtists,
          topArtist: summary.topArtist,
          firstPlayedAt: summary.firstPlayedAt,
          lastPlayedAt: summary.lastPlayedAt,
          rangeDays,
          detailTruncated: summary.detailTruncated,
        }
      },

      getTotalPlayTime(): number {
        return loadData().totalPlayTime + liveSeconds()
      },

      getPlayCount(trackPath: string): number {
        return loadData().tracks[trackPath]?.plays ?? 0
      },

      getAllPlayCounts(): Record<string, number> {
        const counts: Record<string, number> = {}
        for (const [path, aggregate] of Object.entries(loadData().tracks)) {
          counts[path] = aggregate.plays
        }
        return counts
      },

      getMostPlayed(query: TopTrackQuery = {}): TopTrack[] {
        const { limit = 10, sortBy = 'plays', days = null } = query
        const data = loadData()
        let items: TopTrack[]

        if (days === null) {
          items = Object.entries(data.tracks)
            .filter(([, aggregate]) => aggregate.plays > 0 || aggregate.seconds > 0)
            .map(([path, aggregate]) =>
              buildTopTrack(
                path,
                data,
                {
                  plays: aggregate.plays,
                  seconds: aggregate.seconds,
                  completed: aggregate.completed,
                  judged: aggregate.judged,
                  lastPlayedAt: aggregate.lastPlayedAt,
                },
                aggregate.duration,
              ),
            )
        } else {
          const keys = recentDayKeys(days)
          const windowStart = keys.length > 0 ? dayKeyOffset(keys[0]!, 0).startAt : 0
          const buckets = new Map<
            string,
            {
              plays: number
              seconds: number
              completed: number
              judged: number
              lastPlayedAt: number
              trackSeconds: number
            }
          >()

          for (const entry of data.history) {
            if (entry.timestamp < windowStart) continue
            const bucket = buckets.get(entry.path) ?? {
              plays: 0,
              seconds: 0,
              completed: 0,
              judged: 0,
              lastPlayedAt: 0,
              trackSeconds: 0,
            }
            if (entry.counted) {
              bucket.plays += 1
              bucket.judged += 1
            }
            if (entry.completed) bucket.completed += 1
            bucket.seconds += entry.listenedSeconds
            bucket.lastPlayedAt = Math.max(bucket.lastPlayedAt, entry.timestamp)
            // history 升序(见 PlayHistoryEntry),顺序覆盖后留下最近一次记录的曲目时长
            if (entry.trackSeconds > 0) bucket.trackSeconds = entry.trackSeconds
            buckets.set(entry.path, bucket)
          }

          items = [...buckets.entries()].map(([path, bucket]) =>
            buildTopTrack(path, data, bucket, bucket.trackSeconds),
          )
        }

        const compare = (a: TopTrack, b: TopTrack): number => {
          if (sortBy === 'seconds') return b.seconds - a.seconds || b.plays - a.plays
          if (sortBy === 'recent') return b.lastPlayedAt - a.lastPlayedAt
          return b.plays - a.plays || b.seconds - a.seconds
        }

        return items.sort(compare).slice(0, limit)
      },

      getPlayHistory(limit = 50): PlayHistoryEntry[] {
        const data = loadData()
        return data.history
          .slice(-limit)
          .reverse()
          .map((entry) => ({
            ...entry,
            title: data.tracks[entry.path]?.title ?? '',
            artist: data.tracks[entry.path]?.artist ?? '',
          }))
      },

      /** 最近 N 天的趋势数据(升序,自动补齐没有播放的日期) */
      getDailySeries(days = TREND_DEFAULT_DAYS): DailyPoint[] {
        const data = loadData()
        return recentDayKeys(days).map((key) => {
          const day = data.daily[key]
          return {
            date: key,
            plays: day?.plays ?? 0,
            seconds: day?.seconds ?? 0,
            completed: day?.completed ?? 0,
          }
        })
      },

      clearAllData(): void {
        playStartTime = null
        accumulatedPlayTime = 0
        sessionListened = 0
        sessionCounted = false
        sessionCompleted = false
        lastPosition = 0
        // lastTrack / sessionDuration 保留:当前正在播放的曲目继续从零累计
        saveData(emptyData())
        api.storage.remove('playCounts')
        api.storage.remove('playHistory')
        api.log.info('播放统计数据已清除')
      },
    }
  },
}
