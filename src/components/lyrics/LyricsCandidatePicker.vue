<template>
  <!-- v-if 放在 Teleport 上:隐藏时不向 body 留下占位注释节点,
       挂载/卸载路径更干净,也避免"Teleport + 内部 v-if"的注释锚点复用 -->
  <Teleport v-if="visible" to="body">
    <div class="picker-overlay" @click.self="close">
      <div class="picker-dialog" role="dialog" aria-modal="true">
        <div class="picker-header">
          <h4 class="picker-title">{{ $t('lyrics.pickCandidate') }}</h4>
          <button class="picker-close" title="close" @click="close">
            <span class="material-symbols-rounded">close</span>
          </button>
        </div>

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
            class="picker-auto-btn"
            :disabled="autoFetching || loading || !track"
            @click="autoFetchBest"
          >
            {{ autoFetching ? $t('lyrics.fetching') : $t('lyrics.autoBest') }}
          </button>
        </div>

        <div class="picker-body">
          <div v-if="loading" class="picker-loading">{{ $t('lyrics.loading') }}</div>
          <div v-else-if="groups.length === 0" class="picker-empty">
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
                :class="{ 'is-selectable': true }"
                @click="preview = cand"
              >
                <span class="candidate-check" :class="{ active: preview === cand }">
                  <span class="material-symbols-rounded">radio_button_unchecked</span>
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
            <div class="preview-title">{{ $t('lyrics.preview') }}</div>
            <MD3Select v-model="kindModel" :options="kindOptions" @change="handleKindChange" />
          </div>
          <pre class="preview-lines">{{ previewLrc }}</pre>
        </div>

        <div class="picker-footer">
          <button class="btn-cancel" @click="close">{{ $t('common.cancel') }}</button>
          <button class="btn-apply" :disabled="!preview || applying" @click="apply">
            {{ applying ? $t('lyrics.saving') : $t('lyrics.applyAndSave') }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { usePlayerStore } from '@/stores/player'
import { useConfigStore } from '@/stores/config'
import type { LyricCandidate, LyricKind } from '@/utils/lyricProviders'
import { buildPreviewLyric } from '@/utils/lyricProviders'
import { LYRIC_PROVIDERS } from '@/utils/lyricProviders'
import MD3Select from '../MD3Select.vue'

const props = defineProps<{
  visible: boolean
  loading: boolean
  candidates: LyricCandidate[]
}>()

const emit = defineEmits<{
  close: []
  apply: [candidate: LyricCandidate, kind: LyricKind]
  autoFetch: []
}>()

const { t } = useI18n()
const playerStore = usePlayerStore()
const configStore = useConfigStore()

const track = computed(() => playerStore.currentTrack)

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

// 候选歌词存在翻译/罗马音时，允许切换最终文本类型
const previewHasTranslation = computed(
  () => !!preview.value?.bundle.tlyric || !!preview.value?.bundle.romalrc,
)
const kindModel = ref<LyricKind>('auto')
const kindOptions = computed(() => [
  { value: 'auto', label: t('lyrics.kindAuto') },
  { value: 'original', label: t('lyrics.kindOriginal') },
  ...(previewHasTranslation.value
    ? [{ value: 'translation', label: t('lyrics.kindTranslation') }]
    : []),
  ...(previewHasTranslation.value ? [{ value: 'roman', label: t('lyrics.kindRoman') }] : []),
])

const previewLrc = computed(() => {
  if (!preview.value) return ''
  // 预览走 LRC 视图：逐字 bundle 的 ASS 只给播放器渲染用，直接展示会是一堆 Dialogue 行
  return (
    buildPreviewLyric(
      preview.value.bundle,
      kindModel.value,
      configStore.lyrics?.preferTranslation ?? true,
    ) || t('lyrics.noLyricContent')
  )
})

const handleKindChange = (): void => {
  // 只需要触发 previewLrc 重新计算
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
/* =========================================
   主题适配说明
   -----------------------------------------
   本应用的主题系统(material-color-utilities 的 Scheme.light/dark)只输出 29 个
   基础角色色:primary / secondary / tertiary / error / background / surface /
   surface-variant / outline / outline-variant / on-* / inverse-* 等。
   MD3 的容器层级色 surface-container(-low/-high/-highest) 在本应用的主题里**不存在**
   (主题只输出 29 个基础角色色),所以这里一律使用真实存在的 token,也不写深浅色兜底值:
   之前 surface-container-high 配 #1e1e24 兜底,会让浅色模式出现
   「深色底 + 深色 on-surface 文字」而看不清。
   层级:panel=surface(配遮罩+阴影) / 内嵌块=surface-variant /
        次级块=surface-variant / 悬停=hover-overlay。
   注意:不要在这里改回 surface-container*,那会让弹窗底色随主题缺失而失效。
   ========================================= */
.picker-overlay {
  position: fixed;
  inset: 0;
  z-index: 3000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.45);
}

.picker-dialog {
  width: 560px;
  max-width: 92vw;
  max-height: 82vh;
  display: flex;
  flex-direction: column;
  border-radius: var(--md-sys-shape-corner-large, 16px);
  background: var(--md-sys-color-surface);
  color: var(--md-sys-color-on-surface);
  box-shadow: var(--shadow-strong, 0 12px 40px rgba(0, 0, 0, 0.4));
  overflow: hidden;
}

.picker-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 14px 18px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.picker-title {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
}

.picker-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: 20px;
  background: transparent;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.picker-close:hover {
  background-color: var(--md-sys-color-hover-overlay);
}

.picker-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 18px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.picker-track {
  flex: 1;
  min-width: 0;
  font-size: 14px;
}

.picker-track-text {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.picker-track-empty {
  color: var(--md-sys-color-on-surface-variant);
}

.picker-auto-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 40px;
  padding: 0 16px;
  border: none;
  border-radius: 20px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.picker-auto-btn:hover:not(:disabled) {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
}

.picker-auto-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.picker-body {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

.picker-loading,
.picker-empty {
  padding: 28px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.picker-groups {
  padding: 0 12px;
}

.candidate-group {
  margin-bottom: 8px;
}

.candidate-group-header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.4px;
  color: var(--md-sys-color-on-surface-variant);
}

.candidate-group-count {
  padding: 0 6px;
  border-radius: 999px;
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 11px;
}

.candidate-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--md-sys-shape-corner-medium, 12px);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.candidate-row:hover {
  background-color: var(--md-sys-color-hover-overlay);
}

.candidate-check {
  display: flex;
  color: var(--md-sys-color-on-surface-variant);
  opacity: 0.5;
}

.candidate-check.active {
  color: var(--md-sys-color-primary);
  opacity: 1;
}

.candidate-meta {
  min-width: 0;
}

.candidate-title {
  font-size: 14px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.candidate-sub {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.preview-pane {
  border-top: 1px solid var(--md-sys-color-outline-variant);
  padding: 12px 18px;
}

.preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.preview-title {
  font-size: 13px;
  color: var(--md-sys-color-on-surface-variant);
}

.preview-lines {
  max-height: 160px;
  overflow-y: auto;
  margin: 0;
  padding: 8px 10px;
  border-radius: var(--md-sys-shape-corner-small, 8px);
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface);
  font-family: inherit;
  font-size: 13px;
  line-height: 1.6;
  white-space: pre-wrap;
}

.picker-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 18px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.btn-cancel {
  height: 40px;
  padding: 0 16px;
  border: none;
  border-radius: 20px;
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.btn-cancel:hover {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-surface-variant)
  );
}

.btn-apply {
  height: 40px;
  padding: 0 16px;
  border: none;
  border-radius: 20px;
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.btn-apply:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
