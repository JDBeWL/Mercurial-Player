/**
 * 播放统计插件
 *
 * 数据分三层:单曲聚合 tracks[path]、每日聚合 daily[日期]、播放明细 history。
 * 口径:单次收听 ≥ 30 秒计次(短曲听完也算);覆盖时长 90% 以上算完播,时长未知时
 * 退化为"累计 ≥ 5 分钟";完播率以"可判定次数"为分母;单曲循环/回退超 30 秒算新一轮。
 * 未落盘时长每 5 分钟分段提交;存储上限 1MB,超限时宿主裁剪"数组"键,
 * 故 history 升序存放(裁剪保留末尾即最新),写入时也对曲目数/天数/条数做裁剪。
 */

import i18n from '@/i18n'
import { PluginPermission, type PluginAPI, type BuiltinPluginDefinition } from '../pluginManager'
import type { Track } from '@/types'

// ============ 统计口径与容量常量 ============

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
/** 轮询间隔(毫秒):3 秒比原先的 5 秒更容易命中"播完自动切歌"的结算时机 */
const POLL_INTERVAL_MS = 3000
/** 未落盘时长达到该秒数时先做一次分段提交 */
const CHECKPOINT_SECONDS = 300
/** 播放位置相对上一次回退超过该秒数时,视为开始新一轮播放 */
const REWIND_THRESHOLD_SECONDS = 30
/** 单次会话低于该秒数视为误触,不写入统计 */
const MIN_SETTLE_SECONDS = 1
/** 趋势图默认跨度(天) */
export const TREND_DEFAULT_DAYS = 14

// ============ 数据结构 ============

/** 已归档的旧版播放历史条目(v1.0 / v1.1) */
interface LegacyHistoryEntry {
  path?: unknown
  title?: unknown
  artist?: unknown
  timestamp?: unknown
}

/** 单条播放明细(时间升序存放,读取时倒序返回) */
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
  /** 曲名(读取时由单曲聚合补全,写入时不落盘) */
  title: string
  /** 歌手(读取时由单曲聚合补全,写入时不落盘) */
  artist: string
}

/** 落盘用的明细条目:不含 title / artist,避免与 tracks 重复占用存储 */
type StoredHistoryEntry = Omit<PlayHistoryEntry, 'title' | 'artist'>

/** 单曲聚合统计 */
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

/** 单日聚合统计 */
interface DailyStat {
  plays: number
  seconds: number
  completed: number
}

/** 插件持久化数据结构 */
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

/** 榜单条目 */
export interface TopTrack {
  path: string
  title: string
  artist: string
  plays: number
  seconds: number
  secondsFormatted: string
  completed: number
  judged: number
  /** 完播率(0~1),无可判定样本时为 null */
  completionRate: number | null
  duration: number
  lastPlayedAt: number
}

/** 趋势图数据点 */
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

// ============ 时间与格式化工具 ============

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

/** 格式化收听时长(秒 → 本地化文案),页面侧复用同一口径 */
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

// ============ 统计判定 ============

/** 单次收听是否达到计次门槛 */
const meetsCountThreshold = (listened: number, trackSeconds: number): boolean =>
  listened >= MIN_COUNTED_SECONDS || (trackSeconds > 0 && listened >= trackSeconds * COMPLETE_RATIO)

/** 单次收听是否构成完播 */
const meetsCompleteThreshold = (listened: number, trackSeconds: number): boolean =>
  trackSeconds > 0
    ? Math.min(listened, trackSeconds) >= trackSeconds * COMPLETE_RATIO
    : listened >= COMPLETE_FALLBACK_SECONDS

/** 播放位置是否已到曲目末尾(播放停止时用于判定"这一轮是不是听完了") */
const isTrackFinished = (position: number, trackSeconds: number): boolean =>
  trackSeconds > 0 && position >= trackSeconds - 2

/** 计算完播率,无判定样本时返回 null(而不是误导性的 0) */
const toCompletionRate = (completed: number, judged: number): number | null =>
  judged > 0 ? Math.min(1, completed / judged) : null

export const playCountPlugin: BuiltinPluginDefinition = {
  id: 'builtin-play-count',
  name: i18n.global.t('plugin.playCountName'),
  version: '1.2.0',
  author: 'Mercurial Player',
  description: i18n.global.t('plugin.playCountDescription'),
  permissions: [PluginPermission.PLAYER_READ, PluginPermission.STORAGE],

  main: (api: PluginAPI) => {
    // ---- 当前播放会话状态(仅内存,不落盘) ----
    let lastTrack: Track | null = null
    /** 本轮"未提交分段"的起点,暂停时置空 */
    let playStartTime: number | null = null
    /** 已暂停但尚未提交的分段时长 */
    let accumulatedPlayTime = 0
    /** 当前曲目会话的累计收听时长(含已提交分段) */
    let sessionListened = 0
    /** 当前曲目时长(秒),来自播放器状态或曲目元信息 */
    let sessionDuration = 0
    /** 上一次轮询到的播放位置,用于识别回退(重播/单曲循环) */
    let lastPosition = 0
    /** 本会话是否已计一次播放 */
    let sessionCounted = false
    /** 本会话是否已记一次完播 */
    let sessionCompleted = false

    let pollingInterval: ReturnType<typeof setInterval> | null = null

    // 保存事件回调引用以便正确清理
    let trackChangedCallback: (data: unknown) => void
    let stateChangedCallback: (data: unknown) => void

    // ========== 数据读写 ==========

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
     * 把旧版(1.0 / 1.1)数据转换成 v2 结构。纯函数、不落盘,插件未激活也能看到迁移结果。
     *
     * 旧版只有播放次数与总时长:tracks[].plays 沿用旧计数,seconds/judged 保持 0;
     * 用旧历史回填曲目元信息、最近播放时间与每日播放次数(趋势图立即可用)。
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
          // 旧历史只保留了最近 100 条,按条数回填"播放次数"是此处唯一可用的近似
          day.plays += 1
          data.daily[key] = day
        }
      }

      data.history.sort((a, b) => a.timestamp - b.timestamp)
      data.history = data.history.slice(-HISTORY_LIMIT)
      return data
    }

    /** 读取数据;数据结构落后时返回内存中的迁移结果(仍不写盘) */
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

    /** 首次激活时把迁移结果落盘,并清掉不再使用的旧键 */
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

    /** 容量裁剪:日期保留窗口 + 曲目聚合上限(优先淘汰零计次的曲目) */
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

    // ========== 会话计时 ==========

    /** 尚未落盘的时长(秒) */
    const pendingSeconds = (): number =>
      accumulatedPlayTime + (playStartTime ? (Date.now() - playStartTime) / 1000 : 0)

    /** 实时可展示的进行中时长(秒) */
    const liveSeconds = (): number => Math.min(pendingSeconds(), MAX_SESSION_SECONDS)

    const startSegment = (): void => {
      playStartTime = Date.now()
    }

    /** 把运行中的分段并入"未提交时长" */
    const pauseSegment = (): void => {
      if (playStartTime) {
        accumulatedPlayTime += (Date.now() - playStartTime) / 1000
        playStartTime = null
      }
    }

    /** 提交时长:只累计收听秒数,不决定播放次数与完播 */
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

    /** 提交结果:写播放次数、完播次数与一条播放明细 */
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

      // 明细按时间升序存放:宿主在存储超限时裁剪数组末尾之外的部分,升序可保住最新记录
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
     * 结算当前分段:把未提交时长写进聚合、并入会话累计。
     * 是否计次/完播由会话累计决定,统一在会话结束时经 commitOutcome 落盘,
     * 因此分段提交不会把一次收听拆成多次播放。
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

    /** 结束当前曲目会话:落盘播放次数与完播,并重置会话状态 */
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

    // ========== 播放状态跟踪 ==========

    const handlePlaybackState = (
      track: Track | null,
      isPlaying: boolean,
      position: number,
      duration: number,
    ): void => {
      const trackPath = track?.path ?? null
      const lastPath = lastTrack?.path ?? null
      const trackSeconds = duration > 0 ? duration : (track?.duration ?? 0)

      // 切歌:结算并归档上一首,然后开新会话
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

      // 位置大幅回退 = 单曲循环或手动重播,按新一轮播放结算
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
        // 每满 5 分钟先落一次盘,避免异常退出丢掉整段时长;
        // 提交后立刻续上计时,否则会漏掉结算瞬间这一个轮询间隔
        if (pendingSeconds() >= CHECKPOINT_SECONDS) {
          settleSegment()
          startSegment()
        }
        return
      }

      // 暂停:先把已收听时长落盘,分段与后续继续播放会接着累计
      const finished = isTrackFinished(position, sessionDuration)
      settleSegment()
      // 播放位置已经到底而播放停止(列表播完 / 手动暂停在结尾),直接归档本次播放,
      // 否则这一次播放要等到下次切歌或退出应用才会计数
      if (finished) endSession()
    }

    const pollPlayerState = async (): Promise<void> => {
      try {
        const state = api.player.getState()
        handlePlaybackState(state.currentTrack, state.isPlaying, state.currentTime, state.duration)
      } catch {
        // 播放器尚未就绪时忽略本轮轮询
      }
    }

    // ========== 查询实现 ==========

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
     * 汇总指定范围的数据。
     * days === null 取单曲聚合(含旧版迁移部分,数值最完整);days > 0 取每日聚合
     * (不受明细窗口限制),但完播口径仍取自明细——旧版每日聚合没有"可判定次数"。
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
          // "播放过的歌曲"只统计真正计次的曲目:试听十几秒就切走的不算
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

    /** 连续收听天数(截至今天或昨天)与历史最长连续天数 */
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

      // 今天还没听时,连续天数截至昨天仍然有意义
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
        pollingInterval = setInterval(() => void pollPlayerState(), POLL_INTERVAL_MS)
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

      // ---------- 对外查询 API ----------

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
            // 明细库时间升序,顺序遍历后留下的即最近一次记录到的曲目时长
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
