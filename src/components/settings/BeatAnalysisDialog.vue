<template>
  <MD3Dialog
    :open="open"
    :title="dialogTitle"
    @close="onRequestClose"
  >
    <div class="beat-dialog">
      <!-- 阶段一：确认。批量任务要跑十几分钟，不能因为误触开关就悄悄占用后台。 -->
      <template v-if="phase === 'confirm'">
        <p class="beat-dialog-text">
          {{
            isEmpty
              ? $t('config.beatAnalysisEmpty')
              : $t('config.beatAnalysisConfirm', { count: paths.length })
          }}
        </p>
        <div v-if="!isEmpty" class="beat-dialog-facts">
          <div class="beat-fact">
            <span class="beat-fact-label">{{ $t('config.beatAnalysisTrackCount') }}</span>
            <span class="beat-fact-value">{{ paths.length }}</span>
          </div>
          <div class="beat-fact">
            <span class="beat-fact-label">{{ $t('config.beatAnalysisEstimate') }}</span>
            <span class="beat-fact-value">{{ formatDuration(estimatedSeconds) }}</span>
          </div>
        </div>
        <label v-if="!isEmpty" class="beat-option">
          <input v-model="forceRecompute" type="checkbox" />
          <span>
            <span class="beat-option-label">{{ $t('config.beatAnalysisForce') }}</span>
            <span class="beat-option-desc">{{ $t('config.beatAnalysisForceDesc') }}</span>
          </span>
        </label>
        <p v-if="!isEmpty" class="beat-dialog-hint">{{ $t('config.beatAnalysisHint') }}</p>
      </template>

      <!-- 阶段二：进行中 -->
      <template v-else-if="phase === 'running'">
        <div class="beat-progress-head">
          <span>{{ $t('config.beatAnalysisRunning') }}</span>
          <span class="beat-progress-percent">{{ percent }}%</span>
        </div>
        <div class="beat-progress-track">
          <div class="beat-progress-fill" :style="{ width: `${percent}%` }"></div>
        </div>
        <div class="beat-progress-detail">
          <span class="beat-progress-count">{{ progress.done }} / {{ progress.total }}</span>
          <span class="beat-progress-elapsed">
            {{ $t('config.beatAnalysisElapsed', { time: elapsedLabel }) }}
          </span>
          <span v-if="remainingText" class="beat-progress-remaining">{{ remainingText }}</span>
        </div>
        <p class="beat-dialog-current" :title="progress.current">{{ progress.current || '—' }}</p>
        <div class="beat-stat-row">
          <span>{{ $t('config.beatAnalysisDone') }} {{ analyzed }}</span>
          <span v-if="failed > 0" class="beat-stat-failed">
            {{ $t('config.beatAnalysisFailed') }} {{ failed }}
          </span>
        </div>
      </template>

      <!-- 阶段三：结束 -->
      <template v-else>
        <p class="beat-dialog-text">
          {{ cancelled ? $t('config.beatAnalysisCancelled') : $t('config.beatAnalysisFinished') }}
        </p>
        <div class="beat-dialog-facts">
          <div class="beat-fact">
            <span class="beat-fact-label">{{ $t('config.beatAnalysisDone') }}</span>
            <span class="beat-fact-value">{{ analyzed }}</span>
          </div>
          <div class="beat-fact">
            <span class="beat-fact-label">{{ $t('config.beatAnalysisSkipped') }}</span>
            <span class="beat-fact-value">{{ skipped }}</span>
          </div>
          <div class="beat-fact">
            <span class="beat-fact-label">{{ $t('config.beatAnalysisFailed') }}</span>
            <span class="beat-fact-value">{{ failed }}</span>
          </div>
        </div>
        <p v-if="invokeError" class="beat-dialog-error">{{ invokeError }}</p>
        <ul v-else-if="summary?.issues?.length" class="beat-issue-list">
          <li v-for="issue in summary.issues" :key="issue.message">
            {{ $t('config.beatAnalysisIssueLine', { count: issue.count, message: issue.message }) }}
          </li>
        </ul>
        <p v-else-if="cancelled" class="beat-dialog-hint">
          {{ $t('config.beatAnalysisResumeHint') }}
        </p>
      </template>
    </div>

    <template #footer>
      <template v-if="phase === 'confirm'">
        <button type="button" @click="onRequestClose">{{ $t('common.cancel') }}</button>
        <button type="button" :disabled="isEmpty" @click="start">
          {{ $t('config.beatAnalysisStart') }}
        </button>
      </template>
      <template v-else-if="phase === 'running'">
        <button type="button" :disabled="cancelling" @click="onCancel">
          {{ cancelling ? $t('config.beatAnalysisCancelling') : $t('common.cancel') }}
        </button>
      </template>
      <template v-else>
        <button type="button" @click="onRequestClose">
          {{ $t('common.close') }}
        </button>
      </template>
    </template>
  </MD3Dialog>
</template>

<script setup lang="ts">
/**
 * 批量节拍分析弹窗：confirm → running → done 三段状态机。
 * 独立成组件：用户可能在分析中关掉设置页，进度仍在推进。
 */
import { computed, ref, watch, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { UnlistenFn } from '@tauri-apps/api/event'
import MD3Dialog from '../MD3Dialog.vue'
import {
  analyzeBeatsBatch,
  beatBackend,
  cancelBeatsBatch,
  estimateBatchSeconds,
  onBeatsProgress,
  type BeatsBatchSummary,
  type BeatsProgress,
} from '@/services/beatService'
import { usePlayerStore } from '@/stores/player'
import logger from '@/utils/logger'

const props = defineProps<{
  open: boolean
  /** 待分析曲目的完整路径 */
  paths: string[]
}>()

const { t } = useI18n()

const emit = defineEmits<{
  close: []
  /** 分析结束（无论成功、失败还是取消），供父级刷新统计 */
  settled: []
}>()

type Phase = 'confirm' | 'running' | 'done'

const phase = ref<Phase>('confirm')
const progress = ref<BeatsProgress>({
  done: 0,
  total: 0,
  current: '',
  path: '',
  bpm: null,
  status: 'started',
  track_ms: 0,
})
const summary = ref<BeatsBatchSummary | null>(null)
/** invoke 本身失败（命令异常），与"每一首的失败原因"分开显示 */
const invokeError = ref('')
const cancelled = ref(false)
const cancelling = ref(false)
/** 默认只补缺失的：已有结果不重算，省掉绝大部分时间 */
const forceRecompute = ref(false)
const analyzed = ref(0)
const skipped = ref(0)
const failed = ref(0)
/** 已用秒数：单首要 6–9 秒，没有计时器界面会像卡死 */
const elapsedSecs = ref(0)
/** 已耗时的样本：用实测均速外推剩余 */
const timedMs = ref(0)
const timedCount = ref(0)

let unlisten: UnlistenFn | null = null
let elapsedTimer: ReturnType<typeof setInterval> | null = null

// 必须在这里翻译：MD3Dialog 的 title 是直接插值的文本，传 key 会原样显示
const dialogTitle = computed(() => {
  if (phase.value === 'confirm') return t('config.beatAnalysisTitle')
  if (phase.value === 'running') return t('config.beatAnalysisTitleRunning')
  return t('config.beatAnalysisTitleDone')
})

const percent = computed(() => {
  if (progress.value.total <= 0) return 0
  return Math.min(100, Math.round((progress.value.done / progress.value.total) * 100))
})

const isEmpty = computed(() => props.paths.length === 0)

/** 推理后端。打开弹窗时探测一次：决定预估耗时用 1.2 秒/首还是 7.5 秒/首 */
const backend = ref<'directml' | 'rten'>('rten')

const estimatedSeconds = computed(() =>
  estimateBatchSeconds(props.paths.length, backend.value),
)

/** mm:ss —— 不随语言变化，省得为"2 分 10 秒"再造一批文案 */
function formatDuration(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds))
  const minutes = Math.floor(seconds / 60)
  const rest = seconds % 60
  return minutes > 0 ? `${minutes}:${String(rest).padStart(2, '0')}` : `${rest}s`
}

const elapsedLabel = computed(() => formatDuration(elapsedSecs.value))

/**
 * 剩余时间：优先用本机实测的单首均速。固定系数在慢机器上会严重低估。
 * 样本不足 1 首时不显示，避免给一个假数字。
 */
const remainingText = computed(() => {
  const { done, total } = progress.value
  if (total <= 0) return ''
  let averageMs: number
  if (timedCount.value >= 1) {
    averageMs = timedMs.value / timedCount.value
  } else if (done >= 1) {
    averageMs = (estimateBatchSeconds(total) * 1000) / total
  } else {
    return ''
  }
  const remaining = ((total - done) * averageMs) / 1000
  return t('config.beatAnalysisRemaining', { time: formatDuration(remaining) })
})

/** 每次打开都重置：上一次的汇总结果不该残留到下一次 */
watch(
  () => props.open,
  (open) => {
    if (open) {
      phase.value = 'confirm'
      progress.value = {
        done: 0,
        total: 0,
        current: '',
        path: '',
        bpm: null,
        status: 'started',
        track_ms: 0,
      }
      summary.value = null
      invokeError.value = ''
      cancelled.value = false
      cancelling.value = false
      forceRecompute.value = false
      analyzed.value = 0
      skipped.value = 0
      failed.value = 0
      elapsedSecs.value = 0
      timedMs.value = 0
      timedCount.value = 0
      // 探测后端只为把预估耗时算准：两条路差一个数量级
      void beatBackend()
        .then((kind) => {
          backend.value = kind
        })
        .catch((error) => {
          logger.error('探测节拍推理后端失败:', error)
        })
    }
  },
)

async function start(): Promise<void> {
  if (props.paths.length === 0) {
    phase.value = 'done'
    return
  }
  phase.value = 'running'
  progress.value = {
    done: 0,
    total: props.paths.length,
    current: '',
    path: '',
    bpm: null,
    status: 'started',
    track_ms: 0,
  }
  elapsedSecs.value = 0
  timedMs.value = 0
  timedCount.value = 0
  // 秒表：单首推理要好几秒，界面必须有东西在动，否则和卡死无法区分
  elapsedTimer = setInterval(() => {
    elapsedSecs.value++
  }, 1000)

  // 先订阅再发命令：反过来的话前几首的进度会漏掉
  try {
    unlisten = await onBeatsProgress((event) => {
      progress.value = event
      if (event.status === 'started') {
        // 只更新"正在分析哪首"；done 由事件里的已完成数决定，不在这里加
        return
      }
      if (event.status === 'analyzed') {
        analyzed.value++
        // 正在播的那首算完就立刻反映到左下角：否则用户要切歌再切回来才看得到
        syncCurrentTrackBpm(event.path, event.bpm)
      } else if (event.status === 'skipped' || event.status === 'missing') {
        // 缺失的曲目没有输入，算失败会误导
        skipped.value++
      } else if (event.status === 'failed') failed.value++
      else if (event.status === 'cancelled') cancelled.value = true
      if (event.track_ms > 0) {
        timedMs.value += event.track_ms
        timedCount.value++
      }
    })
  } catch (error) {
    logger.error('订阅节拍进度失败:', error)
  }

  try {
    const result = await analyzeBeatsBatch(props.paths, forceRecompute.value)
    summary.value = result
    cancelled.value = result.cancelled
  } catch (error) {
    logger.error('批量节拍分析失败:', error)
    invokeError.value = error instanceof Error ? error.message : String(error)
    summary.value = {
      analyzed: analyzed.value,
      skipped: skipped.value,
      failed: failed.value,
      cancelled: false,
      issues: [],
    }
  } finally {
    cleanup()
    phase.value = 'done'
    emit('settled')
  }
}

/** 把新算出的 BPM 写到当前播放曲目上（后端已同步元数据缓存，这里只补内存快照） */
function syncCurrentTrackBpm(path: string, bpm: number | null): void {
  if (!path || bpm === null) return
  const store = usePlayerStore()
  if (store.currentTrack?.path !== path) return
  if (store.currentTrack.bpm !== bpm) {
    store.currentTrack = { ...store.currentTrack, bpm }
  }
  if (store.audioInfo.bpm !== bpm) {
    store.audioInfo = { ...store.audioInfo, bpm }
  }
}

async function onCancel(): Promise<void> {
  cancelling.value = true
  try {
    await cancelBeatsBatch()
  } catch (error) {
    logger.error('取消节拍分析失败:', error)
  }
}

/** 关闭：进行中先请求取消，避免后台还在跑却没了入口 */
function onRequestClose(): void {
  if (phase.value === 'running') {
    void onCancel()
    return
  }
  cleanup()
  emit('close')
}

function cleanup(): void {
  if (unlisten) {
    unlisten()
    unlisten = null
  }
  if (elapsedTimer !== null) {
    clearInterval(elapsedTimer)
    elapsedTimer = null
  }
}

onUnmounted(cleanup)
</script>

<style scoped>
/* 尺寸与配色取自设置页既有的 MD3 变量与 .stat-card 度量 */
.beat-dialog {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.beat-dialog-text {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
  color: var(--md-sys-color-on-surface);
}

.beat-dialog-hint {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-dialog-facts {
  display: flex;
  gap: 12px;
}

.beat-fact {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding: 16px;
  border-radius: 16px;
  background-color: var(--md-sys-color-surface-variant);
}

.beat-fact-label {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-fact-value {
  font-size: 22px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  color: var(--md-sys-color-on-surface);
}

.beat-option {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 12px 16px;
  border-radius: 12px;
  cursor: pointer;
  background-color: var(--md-sys-color-surface-variant);
}

.beat-option input {
  margin-top: 3px;
  accent-color: var(--md-sys-color-primary);
}

.beat-option-label {
  display: block;
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
}

.beat-option-desc {
  display: block;
  margin-top: 2px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-progress-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
}

.beat-progress-percent {
  font-size: 14px;
  font-weight: 500;
  font-variant-numeric: tabular-nums;
  color: var(--md-sys-color-primary);
}

.beat-progress-track {
  height: 4px;
  border-radius: 2px;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-variant);
}

.beat-progress-fill {
  height: 100%;
  border-radius: 2px;
  background-color: var(--md-sys-color-primary);
  transition: width 0.2s ease;
}

.beat-progress-detail {
  display: flex;
  gap: 12px;
  justify-content: space-between;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-progress-elapsed,
.beat-progress-remaining {
  white-space: nowrap;
}

.beat-dialog-current {
  margin: 0;
  overflow: hidden;
  font-size: 12px;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-stat-row {
  display: flex;
  gap: 16px;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--md-sys-color-on-surface-variant);
}

.beat-stat-failed {
  color: var(--md-sys-color-error);
}

.beat-dialog-error {
  margin: 0;
  padding: 12px 16px;
  border-radius: 12px;
  font-size: 12px;
  line-height: 1.6;
  word-break: break-all;
  color: var(--md-sys-color-error);
  background-color: var(--md-sys-color-error-container);
}

/* 失败原因按类归并后列表展示：只报一条样本会让人误以为全库都是同一个毛病 */
.beat-issue-list {
  margin: 0;
  padding: 12px 16px;
  border-radius: 12px;
  list-style: none;
  font-size: 12px;
  line-height: 1.7;
  color: var(--md-sys-color-on-surface-variant);
  background-color: var(--md-sys-color-surface-variant);
}

.beat-issue-list li + li {
  margin-top: 4px;
}
</style>
