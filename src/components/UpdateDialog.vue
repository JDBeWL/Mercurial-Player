<template>
  <MD3Dialog
    :open="updateAvailable"
    :title="t('config.update.available')"
    :z-index="10000"
    @close="onDismiss"
  >
    <template #supporting>
      {{ t('config.update.newVersionAvailable', { version: newVersion }) }}
    </template>

    <div class="update-body">
      <div v-if="error" class="error-message">
        <span class="material-symbols-rounded">error</span>
        <span>{{ error }}</span>
      </div>

      <div v-if="isDownloading" class="download-section">
        <div class="progress-info">
          <span>{{ t('config.update.downloading') }}</span>
          <span class="progress-percent">{{ downloadProgress }}%</span>
        </div>
        <div class="progress-bar">
          <div class="progress-bar-fill" :style="{ width: downloadProgress + '%' }"></div>
        </div>
        <div class="progress-detail">
          <span>
            {{ formatBytes(downloadedBytes)
            }}<template v-if="totalBytes > 0"> / {{ formatBytes(totalBytes) }}</template>
          </span>
          <span v-if="downloadSpeed > 0">{{ formatBytes(downloadSpeed) }}/s</span>
        </div>
      </div>

      <div v-else class="release-notes">
        <h3>{{ t('config.update.releaseNotes') }}</h3>
        <div class="notes-content">
          <!-- eslint-disable-next-line vue/no-v-html -- release notes 来自 GitHub API，经 renderMarkdown 渲染 -->
          <div v-if="releaseNotes" class="markdown-body" v-html="renderedNotes"></div>
          <p v-else class="text-secondary">{{ t('config.update.noReleaseNotes') }}</p>
        </div>
      </div>
    </div>

    <template #footer>
      <button type="button" :disabled="isDownloading" @click="onDismiss">
        {{ t('common.later') }}
      </button>
      <button type="button" :disabled="isDownloading || hasError" @click="onUpdate">
        <span v-if="!isDownloading" class="material-symbols-rounded">download</span>
        <span v-else class="material-symbols-rounded spin">autorenew</span>
        <span v-if="isDownloading">{{ t('config.update.installing') }}</span>
        <span v-else-if="downloadFinished">{{ t('config.update.installNow') }}</span>
        <span v-else>{{ t('config.update.downloadNow') }}</span>
      </button>
    </template>
  </MD3Dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAutoUpdate } from '@/composables/useAutoUpdate'
import { renderMarkdown } from '@/utils/markdownRenderer'
import MD3Dialog from './MD3Dialog.vue'
import logger from '@/utils/logger'
import { formatBytes } from '@/utils/format'

interface Emits {
  (e: 'update'): void
  (e: 'dismiss'): void
}

const emit = defineEmits<Emits>()

const { t } = useI18n()

const {
  updateAvailable,
  newVersion,
  downloadProgress,
  isDownloading,
  error,
  releaseNotes,
  downloadFinished,
  downloadedBytes,
  totalBytes,
  downloadSpeed,
  hasError,
  downloadAndInstall,
  runInstaller,
  resetUpdateState,
} = useAutoUpdate()

/** Markdown -> HTML；渲染来源与安全豁免见模板里 v-html 处的注释 */
const renderedNotes = computed(() => {
  if (!releaseNotes.value) return ''
  return renderMarkdown(releaseNotes.value)
})

const onUpdate = async () => {
  try {
    if (!downloadFinished.value) {
      await downloadAndInstall()
    } else {
      await runInstaller()
    }
    emit('update')
  } catch (err) {
    logger.error('Update failed:', err)
  }
}

const onDismiss = () => {
  resetUpdateState()
  emit('dismiss')
}
</script>

<style scoped>
/* 遮罩、圆角、标题、supporting text、页脚按钮都由 MD3Dialog 提供；这里只留内容区 */
.update-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.error-message {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
  border-radius: var(--md-sys-shape-corner-small);
  margin-bottom: 16px;
  font-size: 14px;
}

.error-message .material-symbols-rounded {
  font-size: 20px;
  flex-shrink: 0;
}

.download-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.progress-info {
  display: flex;
  justify-content: space-between;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
}

.progress-percent {
  font-weight: 500;
  color: var(--md-sys-color-primary);
}

.progress-detail {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--md-sys-color-on-surface-variant);
}

.release-notes {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.release-notes h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

/* v-html 注入的节点不带 scoped 属性，必须用 :deep() 才能命中 */
.markdown-body {
  font-size: 14px;
  line-height: 1.7;
  color: var(--md-sys-color-on-surface);
  word-wrap: break-word;
}

.markdown-body :deep(h1),
.markdown-body :deep(h2),
.markdown-body :deep(h3),
.markdown-body :deep(h4) {
  margin: 12px 0 8px;
  font-weight: 600;
  line-height: 1.4;
  color: var(--md-sys-color-on-surface);
}

.markdown-body :deep(h1) {
  font-size: 1.3em;
}
.markdown-body :deep(h2) {
  font-size: 1.15em;
}
.markdown-body :deep(h3) {
  font-size: 1.05em;
}
.markdown-body :deep(h4) {
  font-size: 1em;
}

.markdown-body :deep(p) {
  margin: 6px 0;
}

.markdown-body :deep(ul),
.markdown-body :deep(ol) {
  margin: 6px 0;
  padding-left: 20px;
}

.markdown-body :deep(li) {
  margin: 3px 0;
}

.markdown-body :deep(li)::marker {
  color: var(--md-sys-color-primary);
}

.markdown-body :deep(code) {
  font-family: 'Cascadia Code', 'Fira Code', 'Consolas', monospace;
  font-size: 0.9em;
  padding: 2px 6px;
  border-radius: 4px;
  background-color: var(
    --md-sys-color-surface-container-highest,
    var(--md-sys-color-surface-variant)
  );
  color: var(--md-sys-color-primary);
}

.markdown-body :deep(pre) {
  margin: 8px 0;
  padding: 12px;
  border-radius: 8px;
  background-color: var(
    --md-sys-color-surface-container-highest,
    var(--md-sys-color-surface-variant)
  );
  overflow-x: auto;
}

.markdown-body :deep(pre code) {
  padding: 0;
  background: none;
  color: var(--md-sys-color-on-surface);
  font-size: 13px;
}

.markdown-body :deep(blockquote) {
  margin: 8px 0;
  padding: 4px 12px;
  border-left: 3px solid var(--md-sys-color-primary);
  background-color: var(--md-sys-color-surface-container-low, var(--md-sys-color-surface));
  border-radius: 0 4px 4px 0;
  color: var(--md-sys-color-on-surface-variant);
}

.markdown-body :deep(blockquote p) {
  margin: 4px 0;
}

.markdown-body :deep(hr) {
  margin: 12px 0;
  border: none;
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.markdown-body :deep(a) {
  color: var(--md-sys-color-primary);
  text-decoration: none;
}

@media (hover: hover) {
  .markdown-body :deep(a:hover) {
    text-decoration: underline;
  }
}

.markdown-body :deep(strong) {
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

.markdown-body :deep(del) {
  color: var(--md-sys-color-on-surface-variant);
}

.markdown-body :deep(img) {
  max-width: 100%;
  border-radius: 8px;
  margin: 8px 0;
}

/* display:block + overflow-x:auto：窄屏让宽表格横向滚，而不是撑破对话框 */
.markdown-body :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 8px 0;
  font-size: 0.95em;
  display: block;
  overflow-x: auto;
}

.markdown-body :deep(thead) {
  background-color: var(--md-sys-color-surface-container-high, var(--md-sys-color-surface-variant));
}

.markdown-body :deep(th),
.markdown-body :deep(td) {
  padding: 6px 12px;
  border: 1px solid var(--md-sys-color-outline-variant);
  text-align: left;
}

.markdown-body :deep(th) {
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

.markdown-body :deep(td) {
  color: var(--md-sys-color-on-surface-variant);
}

.markdown-body :deep(tbody tr:nth-child(even)) {
  background-color: var(--md-sys-color-surface-container-low, var(--md-sys-color-surface));
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.update-body::-webkit-scrollbar {
  width: 6px;
}

.update-body::-webkit-scrollbar-track {
  background: transparent;
}

.update-body::-webkit-scrollbar-thumb {
  background: var(--md-sys-color-outline-variant);
  border-radius: 3px;
}

@media (hover: hover) {
  .update-body::-webkit-scrollbar-thumb:hover {
    background: var(--md-sys-color-outline);
  }
}
</style>
