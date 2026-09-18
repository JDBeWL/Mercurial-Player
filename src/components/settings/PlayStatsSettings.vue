<template>
  <div class="tab-content">
    <!-- 头部：标题 + 统计范围 + 操作(结构与插件页保持一致) -->
    <div class="content-header">
      <h3>{{ $t('config.playStats') }}</h3>
      <div class="header-actions">
        <div class="segmented" role="group" :aria-label="$t('config.playStatsRange')">
          <button
            v-for="option in rangeOptions"
            :key="option.key"
            class="segment"
            :class="{ active: range === option.value }"
            :aria-pressed="range === option.value"
            @click="range = option.value"
          >
            {{ option.label }}
          </button>
        </div>
        <!-- 与插件页一致:带文字的填充按钮,而不是只有图标 -->
        <button class="filled-tonal-button" @click="refresh">
          <span class="material-symbols-rounded">refresh</span>
          {{ $t('config.refresh') }}
        </button>
        <button v-if="hasData" class="text-button danger" @click="clearConfirmed = true">
          <span class="material-symbols-rounded">delete_sweep</span>
          {{ $t('config.clearPlayStatsData') }}
        </button>
      </div>
    </div>

    <!-- 清除数据的二次确认：替换掉原先「点一下即清空」的不可逆操作 -->
    <div v-if="clearConfirmed" class="confirm-bar" role="alertdialog">
      <span class="material-symbols-rounded confirm-icon">warning</span>
      <div class="confirm-text">
        <strong>{{ $t('config.clearPlayStatsTitle') }}</strong>
        <span>{{ $t('config.clearPlayStatsDesc') }}</span>
      </div>
      <div class="confirm-actions">
        <button class="text-button" @click="clearConfirmed = false">
          {{ $t('config.cancelAction') }}
        </button>
        <button class="text-button danger filled" @click="confirmClear">
          {{ $t('config.clearPlayStatsConfirm') }}
        </button>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-if="!hasData" class="empty-state">
      <span class="material-symbols-rounded">bar_chart</span>
      <p>{{ $t('config.noPlayStats') }}</p>
      <p class="hint">{{ $t('config.noPlayStatsHint') }}</p>
    </div>

    <template v-else>
      <!-- 统计概览 -->
      <section class="stats-overview" role="list">
        <article
          v-for="card in overviewCards"
          :key="card.key"
          class="stat-card"
          role="listitem"
          :title="card.hint"
        >
          <span class="material-symbols-rounded card-icon">{{ card.icon }}</span>
          <div class="stat-content">
            <span class="stat-value" :class="{ muted: card.muted }">{{ card.value }}</span>
            <span class="stat-label">{{ card.label }}</span>
            <span v-if="card.sub" class="stat-sub">{{ card.sub }}</span>
          </div>
        </article>
      </section>

      <!-- 趋势 -->
      <section class="section">
        <div class="section-header">
          <h4>
            <span class="material-symbols-rounded">show_chart</span>
            {{ $t('config.trendTitle') }}
            <span class="range-tag">{{
              $t('config.playStatsRangeDays', { days: trendDays })
            }}</span>
          </h4>
          <div class="segmented" role="group">
            <button
              v-for="option in metricOptions"
              :key="option.value"
              class="segment"
              :class="{ active: trendMetric === option.value }"
              :aria-pressed="trendMetric === option.value"
              @click="trendMetric = option.value"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <div class="trend-panel">
          <div class="trend-chart">
            <div
              v-for="(point, index) in trendPoints"
              :key="point.date"
              class="trend-col"
              :title="trendTooltip(point)"
            >
              <div class="trend-bar-wrap">
                <div
                  class="trend-bar"
                  :class="{ empty: point.value === 0 }"
                  :style="{ height: barHeight(point.value) }"
                />
              </div>
              <span class="trend-label">{{
                showTrendLabel(index) ? formatDateLabel(point.date) : ''
              }}</span>
            </div>
          </div>
          <div class="trend-summary">
            <span>{{ $t('config.trendDailyAverage', { value: trendAverageLabel }) }}</span>
            <span class="trend-peak">
              {{
                $t('config.todayStat', {
                  plays: stats.todayPlays,
                  duration: formatDuration(stats.todaySeconds),
                })
              }}
            </span>
          </div>
        </div>
      </section>

      <!-- 最常播放 -->
      <section class="section">
        <div class="section-header">
          <h4>
            <span class="material-symbols-rounded">trending_up</span>
            {{ $t('config.mostPlayed') }}
          </h4>
          <div class="section-tools">
            <div class="segmented" role="group">
              <button
                v-for="option in sortOptions"
                :key="option.value"
                class="segment"
                :class="{ active: sortBy === option.value }"
                :aria-pressed="sortBy === option.value"
                @click="sortBy = option.value"
              >
                {{ option.label }}
              </button>
            </div>
            <label class="search-box">
              <span class="material-symbols-rounded">search</span>
              <input
                v-model="topSearch"
                type="search"
                :placeholder="$t('config.searchTracks')"
                :aria-label="$t('config.searchTracks')"
              />
              <button
                v-if="topSearch"
                class="search-clear"
                type="button"
                :title="$t('config.clearSearch')"
                :aria-label="$t('config.clearSearch')"
                @click.prevent="topSearch = ''"
              >
                <span class="material-symbols-rounded">close</span>
              </button>
            </label>
          </div>
        </div>

        <p v-if="detailHintVisible" class="detail-hint">
          <span class="material-symbols-rounded">info</span>
          {{ $t('config.detailWindowHint', { count: historyLimit }) }}
        </p>

        <div v-if="visibleTop.length === 0" class="section-empty">
          {{ $t('config.noSearchResult') }}
        </div>

        <div v-else class="track-list" role="list">
          <div
            v-for="(item, index) in visibleTop"
            :key="item.path"
            class="track-item"
            role="listitem"
            :class="{ 'is-current': isCurrent(item.path), playable: isPlayable(item.path) }"
            :tabindex="isPlayable(item.path) ? 0 : -1"
            :aria-label="`${displayTitle(item)} ${item.artist}`"
            @click="playFromStats(item.path)"
            @keydown.enter.prevent="playFromStats(item.path)"
            @keydown.space.prevent="playFromStats(item.path)"
          >
            <span class="rank" :class="{ 'top-3': index < 3 }">{{ index + 1 }}</span>
            <div v-if="coverUrlFor(item.path)" class="track-cover">
              <img
                :src="coverUrlFor(item.path)"
                :alt="displayTitle(item)"
                loading="lazy"
                decoding="async"
              />
            </div>
            <!-- 无封面时与播放器大封面/MiniPlayer/播放列表行保持一致:album 占位图标 -->
            <div v-else class="track-cover">
              <span class="material-symbols-rounded">album</span>
            </div>
            <div class="track-info">
              <div class="track-line">
                <span class="track-title">{{ displayTitle(item) }}</span>
                <span
                  v-if="isCurrent(item.path)"
                  class="now-playing"
                  :title="$t('config.playingNow')"
                >
                  <span class="bar" /><span class="bar" /><span class="bar" />
                </span>
              </div>
              <div class="track-meta">
                <span v-if="item.artist" class="track-artist">{{ item.artist }}</span>
                <span v-if="item.artist && item.judged > 0" class="sep">·</span>
                <span v-if="item.judged > 0" class="meta-strong">
                  {{ $t('config.badgeCompleted') }} {{ formatPercent(item.completionRate) }}
                </span>
                <span v-if="item.judged > 0 && item.lastPlayedAt > 0" class="sep">·</span>
                <span v-if="item.lastPlayedAt > 0">
                  {{ $t('config.lastPlayedAt', { time: formatRelative(item.lastPlayedAt) }) }}
                </span>
              </div>
              <div
                v-if="item.completionRate !== null"
                class="completion-bar"
                :title="$t('config.completionRateHint')"
              >
                <div class="completion-fill" :style="{ width: `${item.completionRate * 100}%` }" />
              </div>
            </div>
            <div class="track-metrics">
              <span class="play-count">{{
                $t('config.playCountTimes', { count: item.plays })
              }}</span>
              <span class="play-time">{{ item.secondsFormatted }}</span>
            </div>
            <span class="row-icon material-symbols-rounded">
              {{ isCurrent(item.path) && playerIsPlaying ? 'graphic_eq' : 'play_arrow' }}
            </span>
          </div>
        </div>

        <button
          v-if="filteredTop.length > LIST_PAGE_SIZE"
          class="text-button more-button"
          @click="topExpanded = !topExpanded"
        >
          {{
            topExpanded
              ? $t('config.showLess')
              : $t('config.showMore', { count: filteredTop.length })
          }}
        </button>
      </section>

      <!-- 最近播放 -->
      <section class="section">
        <div class="section-header">
          <h4>
            <span class="material-symbols-rounded">history</span>
            {{ $t('config.recentPlayed') }}
          </h4>
          <label class="search-box">
            <span class="material-symbols-rounded">search</span>
            <input
              v-model="historySearch"
              type="search"
              :placeholder="$t('config.searchTracks')"
              :aria-label="$t('config.searchTracks')"
            />
            <button
              v-if="historySearch"
              class="search-clear"
              type="button"
              :title="$t('config.clearSearch')"
              :aria-label="$t('config.clearSearch')"
              @click.prevent="historySearch = ''"
            >
              <span class="material-symbols-rounded">close</span>
            </button>
          </label>
        </div>

        <div v-if="visibleHistory.length === 0" class="section-empty">
          {{ $t('config.noSearchResult') }}
        </div>

        <div v-else class="track-list" role="list">
          <div
            v-for="item in visibleHistory"
            :key="`${item.path}-${item.timestamp}`"
            class="track-item history-item"
            role="listitem"
            :class="{ 'is-current': isCurrent(item.path), playable: isPlayable(item.path) }"
            :tabindex="isPlayable(item.path) ? 0 : -1"
            :aria-label="`${displayTitle(item)} ${item.artist}`"
            @click="playFromStats(item.path)"
            @keydown.enter.prevent="playFromStats(item.path)"
            @keydown.space.prevent="playFromStats(item.path)"
          >
            <div v-if="coverUrlFor(item.path)" class="track-cover">
              <img
                :src="coverUrlFor(item.path)"
                :alt="displayTitle(item)"
                loading="lazy"
                decoding="async"
              />
            </div>
            <!-- 无封面时与播放器大封面/MiniPlayer/播放列表行保持一致:album 占位图标 -->
            <div v-else class="track-cover">
              <span class="material-symbols-rounded">album</span>
            </div>
            <div class="track-info">
              <div class="track-line">
                <span class="track-title">{{ displayTitle(item) }}</span>
                <span
                  v-if="isCurrent(item.path)"
                  class="now-playing"
                  :title="$t('config.playingNow')"
                >
                  <span class="bar" /><span class="bar" /><span class="bar" />
                </span>
              </div>
              <div class="track-meta">
                <span v-if="item.artist" class="track-artist">{{ item.artist }}</span>
                <span v-if="item.artist && item.listenedSeconds > 0" class="sep">·</span>
                <span v-if="item.listenedSeconds > 0">
                  {{
                    $t('config.listenedThisTime', {
                      duration: formatDuration(item.listenedSeconds),
                    })
                  }}
                </span>
                <span v-if="item.trackSeconds > 0" class="meta-weak">
                  / {{ formatDuration(item.trackSeconds) }}
                </span>
              </div>
            </div>
            <span class="play-time">{{ formatRelative(item.timestamp) }}</span>
          </div>
        </div>

        <button
          v-if="filteredHistory.length > HISTORY_PAGE_SIZE"
          class="text-button more-button"
          @click="historyExpanded = !historyExpanded"
        >
          {{
            historyExpanded
              ? $t('config.showLess')
              : $t('config.showMore', { count: filteredHistory.length })
          }}
        </button>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { convertFileSrc } from '@tauri-apps/api/core'
import { pluginManager } from '../../plugins'
import { useErrorNotification } from '../../composables/useErrorNotification'
import { usePlayerStore } from '../../stores/player'
import { getCurrentLocale } from '../../i18n'
import { getTrackCoverPath } from '@/services/mediaService'
import logger from '../../utils/logger'
import {
  HISTORY_LIMIT,
  TREND_DEFAULT_DAYS,
  formatDuration,
  type DailyPoint,
  type PlayCountStats,
  type PlayHistoryEntry,
  type TopTrack,
  type TopTrackSortBy,
} from '@/plugins/builtins/playCount'
import type { Track } from '@/types'

const PLAY_COUNT_PLUGIN_ID = 'builtin-play-count'
const DAY_MS = 24 * 60 * 60 * 1000
const LIST_PAGE_SIZE = 10
const HISTORY_PAGE_SIZE = 12
const HISTORY_QUERY_LIMIT = 200
/** 趋势图最多绘制的柱子数量,避免 90 天范围下柱子过密 */
const TREND_MAX_BARS = 30

const { t } = useI18n()
const { showError, showSuccess } = useErrorNotification()
const playerStore = usePlayerStore()

// ============ 插件实例访问 ============

/** 播放统计插件对外暴露的查询方法（与 playCount.ts 的实现一一对应） */
interface PlayCountPluginInstance {
  getStats(rangeDays?: number | null): PlayCountStats
  getMostPlayed(query?: {
    limit?: number
    sortBy?: TopTrackSortBy
    days?: number | null
  }): TopTrack[]
  getPlayHistory(limit?: number): PlayHistoryEntry[]
  getDailySeries(days?: number): DailyPoint[]
  clearAllData(): void
}

/** pluginManager.instances 是私有字段，此处做一次集中的类型断言 */
interface PluginManagerWithInstances {
  instances: Map<string, { instance: PlayCountPluginInstance }>
}

const getPluginInstance = (): PlayCountPluginInstance | null => {
  const entry = (pluginManager as unknown as PluginManagerWithInstances).instances.get(
    PLAY_COUNT_PLUGIN_ID,
  )
  return entry?.instance ?? null
}

/** 数据源自插件存储（reactive 代理），computed 直接订阅即可；显式读激活状态以保证
 *  激活后重新求值。播放结算会自动重算，不再需要原先 5 秒一次的全量轮询。 */
const instance = computed<PlayCountPluginInstance | null>(() =>
  pluginManager.plugins.get(PLAY_COUNT_PLUGIN_ID)?.state === 'active' ? getPluginInstance() : null,
)

// ============ 视图状态 ============

const range = ref<number | null>(null)
const sortBy = ref<TopTrackSortBy>('plays')
const trendMetric = ref<'plays' | 'seconds'>('plays')
const topSearch = ref('')
const historySearch = ref('')
const topExpanded = ref(false)
const historyExpanded = ref(false)
const clearConfirmed = ref(false)
/** 手动刷新用的重算标记 */
const refreshTick = ref(0)
/** 每秒推进一次，用于让"进行中这一次播放"的实时时长持续累加 */
const playbackTick = ref(0)

let tickTimer: ReturnType<typeof setInterval> | null = null

const EMPTY_STATS: PlayCountStats = {
  totalTracks: 0,
  totalPlays: 0,
  totalPlayTime: 0,
  totalPlayTimeFormatted: '--',
  completedPlays: 0,
  judgedPlays: 0,
  completionRate: null,
  averageSeconds: 0,
  averageSecondsFormatted: '--',
  activeDays: 0,
  currentStreak: 0,
  longestStreak: 0,
  todayPlays: 0,
  todaySeconds: 0,
  distinctArtists: 0,
  topArtist: null,
  firstPlayedAt: 0,
  lastPlayedAt: 0,
  rangeDays: null,
  detailTruncated: false,
}

// ============ 数据 ============

/** 数据源 = 插件实例 + 变更信号（手动刷新计数、播放心跳）。心跳每秒推进一次，让进行中
 *  这次播放的时长持续增长；插件存储本身是 reactive 代理，结算后自动重算，无需轮询。 */
const dataSource = computed(() => ({
  plugin: instance.value,
  revision: refreshTick.value + playbackTick.value,
}))

const stats = computed<PlayCountStats>(() => {
  const { plugin } = dataSource.value
  if (!plugin) return EMPTY_STATS
  try {
    return plugin.getStats(range.value)
  } catch (error) {
    logger.error('读取播放统计失败:', error)
    return EMPTY_STATS
  }
})

const hasData = computed(() => stats.value.totalPlays > 0 || stats.value.totalPlayTime > 0)

const topTracks = computed<TopTrack[]>(() => {
  const { plugin } = dataSource.value
  if (!plugin) return []
  try {
    return plugin.getMostPlayed({ limit: 100, sortBy: sortBy.value, days: range.value })
  } catch (error) {
    logger.error('读取播放榜单失败:', error)
    return []
  }
})

const historyEntries = computed<PlayHistoryEntry[]>(() => {
  const { plugin } = dataSource.value
  if (!plugin) return []
  try {
    return plugin.getPlayHistory(HISTORY_QUERY_LIMIT)
  } catch (error) {
    logger.error('读取播放历史失败:', error)
    return []
  }
})

const dailySeries = computed<DailyPoint[]>(() => {
  const { plugin } = dataSource.value
  if (!plugin) return []
  try {
    return plugin.getDailySeries(trendDays.value)
  } catch (error) {
    logger.error('读取趋势数据失败:', error)
    return []
  }
})

const trendDays = computed(() => Math.min(range.value ?? TREND_DEFAULT_DAYS, TREND_MAX_BARS))

/**
 * 当前播放列表索引 + 封面映射，可播判断/标题兜底/封面都从这里取。
 * playlist 元素已 markRaw、封面就地分批写入，都不会触发响应式更新，
 * 因此显式依赖 `playlistCoverVersion`（每批封面写完重建索引）。
 */
const playlistIndex = computed(() => {
  const tracks = new Map<string, Track>()
  const covers = new Map<string, string>()
  for (const track of playerStore.playlist) {
    tracks.set(track.path, track)
    if (track.coverPath) {
      covers.set(track.path, convertFileSrc(track.coverPath))
    }
  }
  return { tracks, covers, coverVersion: playerStore.playlistCoverVersion }
})

const playerIsPlaying = computed(() => playerStore.isPlaying)
const currentPath = computed(() => playerStore.currentTrack?.path ?? '')

// ============ 曲目封面 ============

/** 播放列表之外的历史曲目按需补一次封面（与音乐库搜索一致：整批替换 Map，只触发一次
 *  渲染）。取不到封面保留占位图，不重试。 */
const lazyCovers = shallowRef<Map<string, string>>(new Map())
const coverRequested = new Set<string>()

const coverUrlFor = (path: string): string | undefined =>
  playlistIndex.value.covers.get(path) ?? lazyCovers.value.get(path)

const loadMissingCovers = (paths: string[]): void => {
  const pending = paths.filter((path) => !coverUrlFor(path) && !coverRequested.has(path))
  if (pending.length === 0) return
  for (const path of pending) {
    coverRequested.add(path)
  }

  void (async () => {
    const found: Array<[string, string]> = []
    for (const path of pending) {
      try {
        const coverPath = await getTrackCoverPath(path)
        if (coverPath) found.push([path, convertFileSrc(coverPath)])
      } catch {
        // 曲目文件已不在或没有内嵌封面,保留占位图
      }
    }
    if (found.length === 0) return
    const next = new Map(lazyCovers.value)
    for (const [path, url] of found) next.set(path, url)
    lazyCovers.value = next
  })()
}

// ============ 概览卡片 ============

interface OverviewCard {
  key: string
  icon: string
  value: string
  label: string
  sub?: string
  hint?: string
  muted?: boolean
}

const overviewCards = computed<OverviewCard[]>(() => {
  const data = stats.value
  const completion = formatPercent(data.completionRate)
  // 完播率分母只统计"可判定"的播放：旧数据没有完播信息，不能当作 0% 展示
  const completionSub =
    data.judgedPlays > 0
      ? t('config.judgedPlays', { count: data.judgedPlays })
      : t('config.completionSampleHint')
  const streakSub =
    data.currentStreak > 0
      ? t('config.streakDays', { days: data.currentStreak })
      : t('config.streakNone')

  return [
    {
      key: 'plays',
      icon: 'play_circle',
      value: String(data.totalPlays),
      label: t('config.totalPlays'),
      sub: t('config.todayStat', {
        plays: data.todayPlays,
        duration: formatDuration(data.todaySeconds),
      }),
      hint: t('config.playStatsCountHint'),
    },
    {
      key: 'tracks',
      icon: 'library_music',
      value: String(data.totalTracks),
      label: t('config.tracksPlayed'),
      sub: data.topArtist
        ? `${t('config.topArtist')} · ${data.topArtist.name}`
        : `${t('config.tracksPlayed')} ${data.totalTracks}`,
    },
    {
      key: 'time',
      icon: 'schedule',
      value: data.totalPlayTimeFormatted,
      label: t('config.totalPlayTime'),
      sub: t('config.todayStat', {
        plays: data.todayPlays,
        duration: formatDuration(data.todaySeconds),
      }),
    },
    {
      key: 'completion',
      icon: 'task_alt',
      value: completion,
      label: t('config.completionRate'),
      sub: completionSub,
      hint: t('config.completionRateHint'),
      muted: data.completionRate === null,
    },
    {
      key: 'days',
      icon: 'calendar_month',
      value: String(data.activeDays),
      label: t('config.listeningDays'),
      sub: streakSub,
    },
    {
      key: 'average',
      icon: 'avg_pace',
      value: data.averageSecondsFormatted,
      label: t('config.averagePerPlay'),
      sub: t('config.averagePerPlayHint'),
      hint: t('config.averagePerPlayHint'),
    },
  ]
})

// ============ 筛选与列表 ============

const normalize = (value: string): string => value.trim().toLowerCase()

const matchKeyword = (keyword: string, ...fields: Array<string | undefined>): boolean =>
  fields.some((field) => (field ?? '').toLowerCase().includes(keyword))

const filteredTop = computed(() => {
  const keyword = normalize(topSearch.value)
  if (!keyword) return topTracks.value
  return topTracks.value.filter((item) =>
    matchKeyword(keyword, item.title, item.artist, displayTitle(item)),
  )
})

const visibleTop = computed(() =>
  topExpanded.value ? filteredTop.value : filteredTop.value.slice(0, LIST_PAGE_SIZE),
)

/** 范围筛选下"最近播放"同步收窄窗口，避免列表与概览口径不一致 */
const historyWindowStart = computed(() =>
  range.value === null ? 0 : startOfDay(Date.now()) - (range.value - 1) * DAY_MS,
)

const matchedHistory = computed(() => {
  const keyword = normalize(historySearch.value)
  const windowStart = historyWindowStart.value
  return historyEntries.value.filter((item) => {
    if (item.timestamp < windowStart) return false
    if (!keyword) return true
    return matchKeyword(keyword, item.title, item.artist, displayTitle(item))
  })
})

/** 最近播放去重：同一曲目只留一条（最近一次）。入参已是"最近优先"，顺序扫描时首次
 *  遇到某曲目即其最近一次播放，保留它即可让曲目按最近播放时间排在最前。 */
const dedupeByPath = (entries: PlayHistoryEntry[]): PlayHistoryEntry[] => {
  const seen = new Set<string>()
  const result: PlayHistoryEntry[] = []
  for (const entry of entries) {
    if (seen.has(entry.path)) continue
    seen.add(entry.path)
    result.push(entry)
  }
  return result
}

const filteredHistory = computed(() => dedupeByPath(matchedHistory.value))

const visibleHistory = computed(() =>
  historyExpanded.value ? filteredHistory.value : filteredHistory.value.slice(0, HISTORY_PAGE_SIZE),
)

// 只为当前可见的行补封面:展开"显示更多"时会自动把新出现的曲目补进来
watch(
  () => [
    ...visibleTop.value.map((item) => item.path),
    ...visibleHistory.value.map((item) => item.path),
  ],
  (paths) => loadMissingCovers(paths),
  { immediate: true },
)

const detailHintVisible = computed(() => range.value !== null && stats.value.detailTruncated)

const historyLimit = HISTORY_LIMIT

// ============ 选项卡配置 ============

const rangeOptions = computed(() => [
  { key: 'all', value: null as number | null, label: t('config.playStatsRangeAll') },
  { key: 'd7', value: 7 as number | null, label: t('config.playStatsRangeDays', { days: 7 }) },
  { key: 'd30', value: 30 as number | null, label: t('config.playStatsRangeDays', { days: 30 }) },
  { key: 'd90', value: 90 as number | null, label: t('config.playStatsRangeDays', { days: 90 }) },
])

const sortOptions = computed<Array<{ value: TopTrackSortBy; label: string }>>(() => [
  { value: 'plays', label: t('config.sortByPlays') },
  { value: 'seconds', label: t('config.sortBySeconds') },
  { value: 'recent', label: t('config.sortByRecent') },
])

const metricOptions = computed<Array<{ value: 'plays' | 'seconds'; label: string }>>(() => [
  { value: 'plays', label: t('config.trendMetricPlays') },
  { value: 'seconds', label: t('config.trendMetricSeconds') },
])

// ============ 趋势图 ============

const trendPoints = computed(() =>
  dailySeries.value.map((point) => ({
    ...point,
    value: trendMetric.value === 'plays' ? point.plays : point.seconds,
  })),
)

const trendMax = computed(() => Math.max(1, ...trendPoints.value.map((point) => point.value)))

const barHeight = (value: number): string => {
  if (value <= 0) return '2px'
  return `${Math.max(6, (value / trendMax.value) * 100)}%`
}

const showTrendLabel = (index: number): boolean => {
  const last = trendPoints.value.length - 1
  return index === 0 || index === last || index % 3 === 0
}

const trendAverageLabel = computed(() => {
  const points = trendPoints.value
  if (points.length === 0) return '--'
  const total = points.reduce((sum, point) => sum + point.value, 0)
  const average = total / points.length
  return trendMetric.value === 'seconds' ? formatDuration(average) : formatPercentValue(average)
})

const trendTooltip = (point: DailyPoint & { value: number }): string =>
  `${point.date} · ${t('config.trendMetricPlays')} ${point.plays} · ${t('config.trendMetricSeconds')} ${formatDuration(point.seconds)}`

// ============ 格式化与判定 ============

const startOfDay = (timestamp: number): number => {
  const date = new Date(timestamp)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

/** 从文件路径提取曲名，作为元信息缺失时的兜底 */
const extractFileName = (path: string): string => {
  if (!path) return t('config.unknown')
  const parts = path.replace(/\\/g, '/').split('/')
  const filename = parts[parts.length - 1] ?? ''
  return filename.replace(/\.[^/.]+$/, '') || t('config.unknown')
}

/** 元信息优先取插件留存的曲目信息，其次查当前播放列表，最后退回文件名 */
const displayTitle = (item: { path: string; title?: string }): string =>
  item.title ||
  playlistIndex.value.tracks.get(item.path)?.title ||
  playlistIndex.value.tracks.get(item.path)?.displayTitle ||
  extractFileName(item.path)

const formatPercentValue = (value: number): string => `${Math.round(value * 10) / 10}%`

const formatPercent = (rate: number | null): string => {
  if (rate === null) return '--'
  const percent = rate * 100
  // 99.95% 以上直接显示 100%，避免浮点误差造成"99.9%"
  return formatPercentValue(percent >= 99.95 ? 100 : percent)
}

const formatDateLabel = (dateKey: string): string => {
  const parts = dateKey.split('-')
  return `${parts[1] ?? ''}/${parts[2] ?? ''}`
}

const formatRelative = (timestamp: number): string => {
  if (!timestamp) return '--'
  const locale = getCurrentLocale() === 'zh' ? 'zh-CN' : 'en-US'
  const date = new Date(timestamp)
  const diff = Date.now() - timestamp

  if (diff < 60 * 1000) return t('config.justNow')
  if (diff < 60 * 60 * 1000) return t('config.minutesAgo', { count: Math.floor(diff / 60000) })
  if (diff < DAY_MS) return t('config.hoursAgo', { count: Math.floor(diff / 3600000) })
  if (timestamp >= startOfDay(Date.now()) - DAY_MS) {
    return `${t('config.yesterday')} ${date.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' })}`
  }
  const days = Math.floor((startOfDay(Date.now()) - startOfDay(timestamp)) / DAY_MS)
  if (days < 30) return t('config.daysAgo', { count: days })
  return date.toLocaleDateString(locale, { year: 'numeric', month: 'short', day: 'numeric' })
}

const isPlayable = (path: string): boolean => playlistIndex.value.tracks.has(path)
const isCurrent = (path: string): boolean => currentPath.value !== '' && currentPath.value === path

// ============ 操作 ============

const refresh = (): void => {
  refreshTick.value += 1
  showSuccess(t('config.playStatsRefreshed'))
}

const playFromStats = async (path: string): Promise<void> => {
  const track = playlistIndex.value.tracks.get(path)
  if (!track) {
    showError(t('config.trackNotInPlaylist'), 'warning', 3000)
    return
  }
  try {
    await playerStore.playTrack(track)
  } catch (error) {
    logger.error('从播放统计中播放曲目失败:', error)
    showError(t('config.trackNotInPlaylist'), 'error')
  }
}

const confirmClear = (): void => {
  const plugin = instance.value
  if (!plugin) return
  try {
    plugin.clearAllData()
    refreshTick.value += 1
    showSuccess(t('config.playStatsCleared'))
  } catch (error) {
    logger.error('清除播放统计失败:', error)
  } finally {
    clearConfirmed.value = false
  }
}

onMounted(() => {
  // 数据本身由响应式存储驱动，这里只为"进行中这一次播放"提供秒级心跳，
  // 且仅在真正播放时推进，暂停/隐藏窗口时不做任何计算
  tickTimer = setInterval(() => {
    if (!document.hidden && playerStore.isPlaying) {
      playbackTick.value += 1
    }
  }, 1000)
})

onUnmounted(() => {
  if (tickTimer) {
    clearInterval(tickTimer)
    tickTimer = null
  }
})
</script>

<style scoped>
.tab-content {
  max-width: 800px;
}

/* ---------- 头部 ---------- */
.content-header {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  margin-bottom: 20px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

.header-actions {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

/* ---------- 分段控件 ----------
   高度与圆角对齐其它设置页的按钮:控件整体 40px 高、20px 圆角(即胶囊形),
   内部选项取 16px(--md-sys-shape-corner-large)保持嵌套观感,文字 14px。
   注意:应用没有全局 box-sizing: border-box,若不显式声明,
   40px 会被撑成 40 + 3×2(padding) + 1×2(border) = 48px */
.segmented {
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  height: 40px;
  padding: 3px;
  border: 1px solid var(--md-sys-color-outline-variant);
  border-radius: 20px;
  background-color: var(--md-sys-color-surface-container);
}

.segmented .segment {
  height: 100%;
  padding: 0 14px;
  border: none;
  border-radius: var(--md-sys-shape-corner-large);
  background: none;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  font-weight: 500;
  font-family: inherit;
  cursor: pointer;
  white-space: nowrap;
  transition:
    background-color 0.2s ease,
    color 0.2s ease;
}

.segmented .segment:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.segmented .segment.active {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

/* 填充按钮:与插件页的刷新/打开目录按钮一致(20px 圆角、secondary-container 底) */
.filled-tonal-button {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 40px;
  padding: 0 16px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  font-family: inherit;
  white-space: nowrap;
  transition:
    background-color 0.2s ease,
    color 0.2s ease;
}

.filled-tonal-button .material-symbols-rounded {
  font-size: 20px;
}

.filled-tonal-button:hover {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
}

/* ---------- 清除确认条 ---------- */
.confirm-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding: 14px 16px;
  margin-bottom: 20px;
  border-radius: 16px;
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

.confirm-bar .confirm-icon {
  font-size: 22px;
}

.confirm-text {
  flex: 1;
  min-width: 200px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.confirm-text strong {
  font-size: 14px;
  font-weight: 600;
}

.confirm-text span {
  font-size: 12px;
  opacity: 0.85;
}

.confirm-actions {
  display: flex;
  gap: 8px;
}

/* ---------- 概览卡片 ---------- */
.stats-overview {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 12px;
  margin-bottom: 28px;
}

.stat-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 16px;
}

.stat-card > .card-icon {
  font-size: 28px;
  color: var(--md-sys-color-primary);
  flex-shrink: 0;
}

.stat-content {
  display: flex;
  flex-direction: column;
  min-width: 0;
  gap: 1px;
}

.stat-value {
  font-size: 22px;
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stat-value.muted {
  color: var(--md-sys-color-on-surface-variant);
}

.stat-label {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.stat-sub {
  font-size: 11px;
  color: var(--md-sys-color-on-surface-variant);
  opacity: 0.75;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---------- 区块 ---------- */
.section {
  margin-bottom: 28px;
}

.section-header {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.section-header h4 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.section-header h4 .material-symbols-rounded {
  font-size: 20px;
  color: var(--md-sys-color-primary);
}

.range-tag {
  padding: 2px 8px;
  border-radius: 8px;
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 11px;
  font-weight: 400;
}

.section-tools {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

/* 搜索框:对齐音乐库的样式(实心底 + 1px outline 描边 + 图标前缀) */
/* 搜索框:音乐库的描边样式 + 与本页其它控件一致的 40px 高 / 20px 圆角。
   同样需要 border-box,否则 1px 上下描边会把它撑到 42px */
.search-box {
  box-sizing: border-box;
  display: flex;
  align-items: center;
  height: 40px;
  padding: 0 10px 0 12px;
  background-color: var(--md-sys-color-surface);
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 20px;
  color: var(--md-sys-color-on-surface-variant);
  transition: border-color 0.2s ease;
}

.search-box:focus-within {
  border-color: var(--md-sys-color-primary);
}

.search-box .material-symbols-rounded {
  font-size: 20px;
  margin-right: 8px;
}

.search-box input {
  width: 150px;
  border: none;
  outline: none;
  background: none;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  font-family: inherit;
}

.search-box input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

/* 隐藏浏览器原生清除按钮,统一用下方的清除按钮 */
.search-box input::-webkit-search-cancel-button {
  appearance: none;
}

.search-clear {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  margin-left: 4px;
  border: none;
  border-radius: 50%;
  background: none;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
}

.search-clear:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.search-clear .material-symbols-rounded {
  font-size: 16px;
  margin-right: 0;
}

.detail-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 0 12px;
  font-size: 11px;
  color: var(--md-sys-color-on-surface-variant);
}

.detail-hint .material-symbols-rounded {
  font-size: 16px;
}

.section-empty {
  padding: 24px;
  border-radius: 12px;
  background-color: var(--md-sys-color-surface-container);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 13px;
  text-align: center;
}

/* ---------- 趋势图 ---------- */
.trend-panel {
  padding: 16px;
  border-radius: 16px;
  background-color: var(--md-sys-color-surface-container);
}

.trend-chart {
  display: flex;
  align-items: flex-end;
  gap: 4px;
  height: 140px;
}

.trend-col {
  flex: 1;
  min-width: 0;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 4px;
}

.trend-bar-wrap {
  flex: 1;
  display: flex;
  align-items: flex-end;
}

.trend-bar {
  width: 100%;
  max-width: 26px;
  margin: 0 auto;
  border-radius: 4px 4px 2px 2px;
  background-color: var(--md-sys-color-primary);
  transition: height 0.25s ease;
}

.trend-bar.empty {
  background-color: var(--md-sys-color-surface-container-highest);
}

.trend-col:hover .trend-bar {
  filter: brightness(1.12);
}

.trend-label {
  height: 12px;
  font-size: 10px;
  line-height: 12px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
  white-space: nowrap;
}

.trend-summary {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

/* ---------- 曲目列表 ---------- */
.track-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.track-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background-color: var(--md-sys-color-surface-container);
  border: 1px solid transparent;
  border-radius: 12px;
  transition:
    background-color 0.2s ease,
    border-color 0.2s ease;
  outline: none;
}

.track-item.playable {
  cursor: pointer;
}

.track-item.playable:hover,
.track-item.playable:focus-visible {
  background-color: var(--md-sys-color-surface-container-high);
  border-color: var(--md-sys-color-outline-variant);
}

.track-item.is-current {
  border-color: var(--md-sys-color-primary);
  background-color: var(--md-sys-color-primary-container);
}

.rank {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-on-surface-variant);
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  flex-shrink: 0;
}

.rank.top-3 {
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
}

/* 封面:尺寸与圆角沿用播放列表/音乐库的列表封面规范(48px 那套的同比例小号),
   无封面时用播放器一致的 album 占位图标,底色用 surface-variant */
.track-cover {
  width: 40px;
  height: 40px;
  border-radius: 4px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-variant);
}

.track-cover img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.track-cover .material-symbols-rounded {
  font-size: 20px;
  color: var(--md-sys-color-on-surface-variant);
}

.track-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.track-line {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.track-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.track-meta {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  white-space: nowrap;
}

.track-artist {
  overflow: hidden;
  text-overflow: ellipsis;
}

.track-meta .sep {
  opacity: 0.5;
}

.track-meta .meta-strong {
  color: var(--md-sys-color-primary);
  font-weight: 500;
}

.track-meta .meta-weak {
  opacity: 0.7;
}

.completion-bar {
  height: 3px;
  border-radius: 2px;
  background-color: var(--md-sys-color-surface-container-highest);
  overflow: hidden;
  margin-top: 2px;
}

.completion-fill {
  height: 100%;
  border-radius: 2px;
  background-color: var(--md-sys-color-primary);
  transition: width 0.3s ease;
}

.track-metrics {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  flex-shrink: 0;
}

.play-count,
.play-time {
  font-size: 11px;
  color: var(--md-sys-color-on-surface-variant);
}

.play-count {
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

.row-icon {
  font-size: 20px;
  color: var(--md-sys-color-on-surface-variant);
  opacity: 0;
  transition: opacity 0.2s ease;
  flex-shrink: 0;
}

.track-item.playable:hover .row-icon,
.track-item.playable:focus-visible .row-icon,
.track-item.is-current .row-icon {
  opacity: 1;
}

.track-item.is-current .row-icon {
  color: var(--md-sys-color-primary);
}

/* "正在播放"的跳动条 */
.now-playing {
  display: inline-flex;
  align-items: flex-end;
  gap: 2px;
  height: 12px;
  flex-shrink: 0;
}

.now-playing .bar {
  width: 3px;
  height: 100%;
  border-radius: 1px;
  background-color: var(--md-sys-color-primary);
  transform-origin: bottom;
  animation: equalize 0.9s ease-in-out infinite;
}

.now-playing .bar:nth-child(2) {
  animation-delay: -0.3s;
}

.now-playing .bar:nth-child(3) {
  animation-delay: -0.6s;
}

@keyframes equalize {
  0%,
  100% {
    transform: scaleY(0.35);
  }
  50% {
    transform: scaleY(1);
  }
}

@media (prefers-reduced-motion: reduce) {
  .now-playing .bar {
    animation: none;
  }
}

.more-button {
  margin-top: 10px;
}

/* ---------- 按钮 ----------
   与其它设置页的 .filled-tonal-button / .text-button 同尺度:
   40px 高、20px 圆角、14px 字号、500 字重 */
.text-button {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  height: 40px;
  padding: 0 16px;
  background: none;
  color: var(--md-sys-color-primary);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  font-family: inherit;
  white-space: nowrap;
  transition: background-color 0.2s ease;
}

.text-button .material-symbols-rounded {
  font-size: 20px;
  /* 全局样式给图标加了 margin-right,这里用 flex gap 统一控制间距 */
  margin-right: 0;
}

.text-button:hover {
  background-color: var(--md-sys-color-primary-container);
}

.text-button.danger {
  color: var(--md-sys-color-error);
}

.text-button.danger:hover,
.text-button.danger.filled {
  background-color: var(--md-sys-color-error-container);
}

.text-button.danger.filled {
  color: var(--md-sys-color-on-error-container);
}

/* ---------- 空状态 ---------- */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.empty-state .material-symbols-rounded {
  font-size: 64px;
  margin-bottom: 16px;
  opacity: 0.5;
}

.empty-state p {
  margin: 0;
  font-size: 16px;
}

.empty-state .hint {
  margin-top: 8px;
  font-size: 14px;
  opacity: 0.7;
}

@media (max-width: 600px) {
  .stats-overview {
    grid-template-columns: 1fr;
  }

  .search-box input {
    width: 100px;
  }
}
</style>
