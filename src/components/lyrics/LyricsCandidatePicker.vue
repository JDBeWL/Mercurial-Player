<template>
  <MD3Dialog
    :open="visible"
    :title="$t('lyrics.pickCandidate')"
    max-width="920px"
    max-height="90dvh"
    @close="close"
  >
    <div class="picker-toolbar">
      <div class="picker-track">
        <span v-if="track" class="picker-track-text"
          >{{ track.title }}<template v-if="track.artist"> - {{ track.artist }}</template></span
        >
        <span v-else class="picker-track-text picker-track-empty">{{
          $t('lyrics.noTrackPlaying')
        }}</span>
      </div>
      <button
        type="button"
        class="picker-auto-btn"
        :disabled="autoFetching || loading || !track"
        @click="autoFetchBest"
      >
        {{ autoFetching ? $t('lyrics.fetching') : $t('lyrics.autoBest') }}
      </button>
    </div>

    <div class="picker-main">
      <div class="picker-body">
        <div v-if="loading" class="picker-status">{{ $t('lyrics.loading') }}</div>
        <div v-else-if="groups.length === 0" class="picker-status">
          {{ $t('lyrics.noCandidates') }}
        </div>
        <div v-else class="picker-groups">
          <section v-for="group in groups" :key="group.provider" class="candidate-group">
            <div class="candidate-group-header">
              <span class="candidate-group-name">{{ t(group.providerNameKey) }}</span>
              <span class="candidate-group-count">{{ group.items.length }}</span>
            </div>
            <div
              v-for="(cand, idx) in group.items"
              :key="`${cand.provider}-${cand.id}-${idx}`"
              class="candidate-row"
              role="button"
              tabindex="0"
              :aria-pressed="preview === cand"
              @click="preview = cand"
              @keydown.enter.prevent="preview = cand"
              @keydown.space.prevent="preview = cand"
            >
              <span class="candidate-check" :class="{ active: preview === cand }">
                <span class="material-symbols-rounded">{{
                  preview === cand ? 'radio_button_checked' : 'radio_button_unchecked'
                }}</span>
              </span>
              <div class="candidate-meta">
                <div class="candidate-title">{{ cand.title }}</div>
                <div class="candidate-sub">
                  <span v-if="cand.artist">{{ cand.artist }}</span>
                  <span v-if="cand.album"> · {{ cand.album }}</span>
                  <span v-if="cand.duration_ms"> · {{ formatMs(cand.duration_ms) }}</span>
                </div>
              </div>
            </div>
          </section>
        </div>
      </div>

      <div v-if="preview" class="preview-pane">
        <div class="preview-header">
          <div class="preview-title">
            {{ $t('lyrics.preview') }}
            <span v-if="isWordSynced" class="preview-badge">{{ $t('lyrics.wordSynced') }}</span>
          </div>
          <MD3Select v-model="kindModel" :options="kindOptions" @change="handleKindChange" />
        </div>
        <div class="preview-lines">
          <div v-for="(row, idx) in previewRows" :key="idx" class="lyric-row">
            <span class="row-time">[{{ row.time }}]</span>
            <span v-if="row.words" class="row-body"
              ><span v-for="(word, wi) in row.words" :key="wi" class="word-unit">{{
                word
              }}</span></span
            >
            <span v-else class="row-body">{{ row.body }}</span>
            <span v-if="row.extra" class="row-extra">{{ row.extra }}</span>
          </div>
          <div v-if="previewRows.length === 0" class="row-empty">
            {{ $t('lyrics.noLyricContent') }}
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <button type="button" @click="close">{{ $t('common.cancel') }}</button>
      <button type="button" :disabled="!preview || applying" @click="apply">
        {{ applying ? $t('lyrics.saving') : $t('lyrics.applyAndSave') }}
      </button>
    </template>
  </MD3Dialog>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfigStore } from '@/stores/config'
import type { LyricCandidate, LyricKind } from '@/services/lyrics'
import type { Track } from '@/types'
import { buildPreviewLyric, filterAssByKind } from '@/services/lyrics'
import { LYRIC_PROVIDERS } from '@/services/lyrics'
import { LyricsParser } from '@/utils/lyricsParser'
import MD3Select from '../MD3Select.vue'
import MD3Dialog from '../MD3Dialog.vue'

const props = defineProps<{
  visible: boolean
  loading: boolean
  candidates: LyricCandidate[]
  /** 弹窗打开时锁定的曲目：标题显示它，保存也写它，不跟当前播放走 */
  track: Track | null
}>()

const emit = defineEmits<{
  close: []
  apply: [candidate: LyricCandidate, kind: LyricKind]
  autoFetch: []
}>()

const { t } = useI18n()
const configStore = useConfigStore()

const groups = computed(() => {
  const byProvider = new Map<string, LyricCandidate[]>()
  for (const cand of props.candidates) {
    const list = byProvider.get(cand.provider) ?? []
    list.push(cand)
    byProvider.set(cand.provider, list)
  }
  return [...byProvider.entries()].map(([provider, items]) => {
    const descriptor = LYRIC_PROVIDERS.find((p) => p.id === provider)
    return { provider, providerNameKey: descriptor?.nameKey ?? provider, items }
  })
})

const preview = ref<LyricCandidate | null>(null)

// 候选歌词存在翻译/罗马音时，允许切换最终文本类型；逐字 bundle 可能只把它们放进 ASS 的 ts/roma 行
const previewHasTranslation = computed(() => {
  const bundle = preview.value?.bundle
  if (!bundle) return false
  if (bundle.tlyric?.trim() || bundle.romalrc?.trim()) return true
  return /,ts,|,roma,/.test(bundle.karaoke ?? '')
})
const kindModel = ref<LyricKind>('auto')
const kindOptions = computed(() => [
  { value: 'auto', label: t('lyrics.kindAuto') },
  { value: 'original', label: t('lyrics.kindOriginal') },
  ...(previewHasTranslation.value
    ? [{ value: 'translation', label: t('lyrics.kindTranslation') }]
    : []),
  ...(previewHasTranslation.value ? [{ value: 'roman', label: t('lyrics.kindRoman') }] : []),
])

/** 预览行：时间戳 + 正文（逐字候选带切好的字）+ 可选的译文/罗马音 */
interface PreviewRow {
  time: string
  /** 切好的字；只有一段或没有时为 null，按整行文本渲染（署名行、非逐字候选都是这种） */
  words: string[] | null
  body: string
  extra: string
}

const previewRows = ref<PreviewRow[]>([])
const isWordSynced = computed(() => !!preview.value?.bundle.karaoke?.trim())

/** 秒 -> `mm:ss.xx`，与 LRC 时间戳同形 */
function formatLineTime(secs: number): string {
  const total = Math.max(0, secs)
  const m = Math.floor(total / 60)
  const s = total - m * 60
  return `${String(m).padStart(2, '0')}:${s.toFixed(2).padStart(5, '0')}`
}

/** 非逐字预览文本（`mergeLyricTexts` 的产物）转行：同一时间戳的第二行就是译文/罗马音 */
function rowsFromLrc(text: string): PreviewRow[] {
  const rows: PreviewRow[] = []
  for (const raw of text.split('\n')) {
    const m = /^\[(\d{2}:\d{2}\.\d{2,3})\]([\s\S]*)$/.exec(raw.trim())
    if (!m) continue
    const body = m[2]!.trim()
    if (!body) continue
    const last = rows[rows.length - 1]
    if (last && last.time === m[1] && !last.extra) {
      last.extra = body
      continue
    }
    rows.push({ time: m[1]!, words: null, body, extra: '' })
  }
  return rows
}

// 预览与渲染走同一条通路：逐字候选先按 kind 裁掉不要的样式再解析 ASS，
// 所以切「原文/译文/罗马音」在预览里是真有反应的，也不会看到成堆的 Dialogue 行
let previewParseSeq = 0
watch(
  [preview, kindModel, () => configStore.lyrics?.preferTranslation],
  async ([cand, kind]) => {
    const seq = ++previewParseSeq
    previewRows.value = []
    if (!cand) return
    const prefer = configStore.lyrics?.preferTranslation ?? true
    const ass = cand.bundle.karaoke?.trim()
    if (ass) {
      const lines = await LyricsParser.parseAsync(filterAssByKind(ass, kind, prefer), 'ass')
      if (seq !== previewParseSeq) return
      previewRows.value = lines.map((line) => {
        const words = (line.words ?? []).map((word) => word.text).filter((text) => text.length > 0)
        const boxed = words.length > 1
        return {
          time: formatLineTime(line.time),
          words: boxed ? words : null,
          body: boxed ? words.join('') : (line.texts[0] ?? ''),
          extra: line.texts[1] ?? '',
        }
      })
      return
    }
    if (seq !== previewParseSeq) return
    previewRows.value = rowsFromLrc(buildPreviewLyric(cand.bundle, kind, prefer))
  },
  { immediate: true },
)

const handleKindChange = (): void => {
  // 空实现：v-model 已经改了 kindModel，上面的 watch 会重算预览
}

watch(
  () => props.candidates,
  () => {
    if (props.candidates.length > 0 && !preview.value) {
      preview.value = props.candidates[0]!
    }
  },
  { immediate: true },
)

watch(
  () => props.visible,
  (visible) => {
    if (!visible) preview.value = null
  },
)

const applying = ref(false)
const autoFetching = ref(false)

const close = (): void => {
  emit('close')
}

const autoFetchBest = async (): Promise<void> => {
  autoFetching.value = true
  try {
    emit('autoFetch')
  } finally {
    autoFetching.value = false
  }
}

const apply = async (): Promise<void> => {
  if (!preview.value) return
  applying.value = true
  try {
    emit('apply', preview.value, kindModel.value)
  } finally {
    applying.value = false
  }
}

function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000)
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  return `${m}:${String(s).padStart(2, '0')}`
}
</script>

<style scoped>
/* 遮罩、圆角、标题、页脚按钮都由 MD3Dialog 提供，这里只排内容区。
   层级：次级块与计数胶囊用 surface-variant，主色只用于选中态；不写深浅色兜底值。 */
.picker-toolbar {
  display: flex;
  flex: none;
  align-items: center;
  gap: 16px;
  padding-bottom: 12px;
}

.picker-track {
  flex: 1;
  min-width: 0;
  font-size: 14px;
  line-height: 20px;
}

.picker-track-text {
  display: block;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.picker-track-empty {
  color: var(--md-sys-color-on-surface-variant);
}

/* M3 tonal 按钮：secondary-container 实色，40dp 高、全圆角、label-large */
.picker-auto-btn {
  display: flex;
  flex: none;
  align-items: center;
  gap: 6px;
  height: 40px;
  padding: 0 16px;
  border: none;
  border-radius: 999px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  letter-spacing: 0.1px;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

@media (hover: hover) {
  .picker-auto-btn:hover:not(:disabled) {
    background-color: var(--md-sys-color-hover-overlay);
  }
}

.picker-auto-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

/* 列表与预览：窄屏上下叠、宽屏左右分栏，滚动只发生在各自区域内 */
.picker-main {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  flex-direction: column;
  gap: 12px;
}

/* 滚动只发生在候选列表上：预览区不参与滚动，挑的时候始终看得见正文 */
.picker-body {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.picker-status {
  padding: 32px 16px;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  text-align: center;
}

.picker-groups {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-bottom: 12px;
}

/* 分区标题用 title-small，M3 不做全大写 */
.candidate-group-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px 4px;
  font-size: 14px;
  font-weight: 500;
  line-height: 20px;
  color: var(--md-sys-color-on-surface-variant);
}

/* 来源名不换行：CJK 在窄列里会一字一行竖排 */
.candidate-group-name {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.candidate-group-count {
  min-width: 20px;
  padding: 0 6px;
  border-radius: 999px;
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 12px;
  font-weight: 500;
  line-height: 20px;
  text-align: center;
}

/* 候选行：项目没有全局 border-box，不写 box-sizing 的话 min-height 是内容高，
   56dp 会胖成 68px（上一版 72dp 实测 96px 就是这么来的） */
.candidate-row {
  display: flex;
  align-items: center;
  gap: 12px;
  box-sizing: border-box;
  min-height: 56px;
  padding: 6px 16px;
  border-radius: var(--md-sys-shape-corner-medium, 12px);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.candidate-row:focus-visible {
  outline: 2px solid var(--md-sys-color-primary);
  outline-offset: -2px;
}

@media (hover: hover) {
  .candidate-row:hover {
    background-color: var(--md-sys-color-hover-overlay);
  }
}

.candidate-check {
  display: flex;
  flex: none;
  color: var(--md-sys-color-on-surface-variant);
}

.candidate-check .material-symbols-rounded {
  font-size: 24px;
}

.candidate-check.active {
  color: var(--md-sys-color-primary);
}

.candidate-meta {
  min-width: 0;
}

.candidate-title {
  font-size: 16px;
  line-height: 24px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.candidate-sub {
  font-size: 14px;
  line-height: 20px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.preview-pane {
  flex: none;
}

.preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 8px;
}

.preview-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
}

.preview-lines {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 28dvh;
  overflow-y: auto;
  overscroll-behavior: contain;
  margin: 0;
  padding: 12px 16px;
  border-radius: var(--md-sys-shape-corner-medium, 12px);
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  line-height: 1.6;
}

.preview-badge {
  margin-left: 8px;
  padding: 1px 8px;
  border: 0;
  border-radius: 999px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  font-size: 12px;
  font-weight: 500;
  vertical-align: 2px;
  white-space: nowrap;
}

/* 时间戳定宽一列，正文和它下面的译文都对齐到同一条竖线（原来用 margin-left 猜宽度，
   时间戳一长就错位） */
.lyric-row {
  display: grid;
  grid-template-columns: 62px 1fr;
  column-gap: 8px;
  align-items: baseline;
}

.row-time {
  color: var(--md-sys-color-on-surface-variant);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.row-body {
  display: flex;
  min-width: 0;
  flex-wrap: wrap;
  gap: 2px;
  white-space: pre-wrap;
}

/* 逐字候选的字加描边，一眼看出这条候选是真切到字还是只有整行 */
.word-unit {
  padding: 0 2px;
  border-radius: var(--md-sys-shape-corner-extra-small, 4px);
  /* 不用 color-mix()：Android System WebView 110 不支持，见 MD3Dialog 的遮罩兜底注释 */
  outline: 1px solid var(--md-sys-color-outline-variant);
  outline-offset: -1px;
}

.row-extra {
  grid-column: 2;
  min-width: 0;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 12px;
  white-space: pre-wrap;
}

.row-empty {
  color: var(--md-sys-color-on-surface-variant);
}

/* 宽到一定程度才分栏：判据是弹窗自身宽度（视口宽 ≠ 弹窗宽，按视口判会把列表挤成一列字）。
   分栏后列表只占固定一小段，剩下的都给预览——歌词正文才是这里要看的东西 */
@container (min-width: 720px) {
  .picker-main {
    flex-direction: row;
  }

  .picker-body {
    flex: 0 1 340px;
    min-width: 220px;
  }

  .preview-pane {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
  }

  .preview-lines {
    flex: 1;
    max-height: none;
  }
}
</style>
