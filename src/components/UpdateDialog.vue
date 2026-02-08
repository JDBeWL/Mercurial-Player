<template>
  <div v-if="updateAvailable" class="update-dialog-overlay">
    <div class="update-dialog">
      <div class="dialog-header">
        <div class="flex-1">
          <h2 class="text-title">{{ t('config.update.available') }}</h2>
          <p class="text-secondary">{{ t('config.update.newVersionAvailable', { version: newVersion }) }}</p>
        </div>
        <button class="icon-button close-btn" @click="onDismiss">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>

      <div class="dialog-content">
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
        </div>

        <div v-else class="release-notes">
          <h3>{{ t('config.update.releaseNotes') }}</h3>
          <div class="notes-content">
            <div v-if="releaseNotes" class="markdown-body" v-html="renderedNotes"></div>
            <p v-else class="text-secondary">{{ t('config.update.noReleaseNotes') }}</p>
          </div>
        </div>
      </div>

      <div class="dialog-footer">
        <button
          class="text-button"
          @click="onDismiss"
          :disabled="isDownloading"
        >
          {{ t('common.later') }}
        </button>
        <button
          class="filled-button"
          @click="onUpdate"
          :disabled="isDownloading || hasError"
        >
          <span v-if="!isDownloading" class="material-symbols-rounded">download</span>
          <span class="material-symbols-rounded spin" v-else>autorenew</span>
          <span v-if="isDownloading">{{ t('config.update.installing') }}</span>
          <span v-else-if="downloadFinished">{{ t('config.update.installNow') }}</span>
          <span v-else>{{ t('config.update.downloadNow') }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useAutoUpdate } from '@/composables/useAutoUpdate'
import { renderMarkdown } from '@/utils/markdownRenderer'
import logger from '@/utils/logger'

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
  hasError,
  downloadAndInstall,
  runInstaller,
  resetUpdateState,
} = useAutoUpdate()

/** 将release notes渲染为HTML */
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
/* ======== 遮罩层 ======== */
.update-dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
  backdrop-filter: blur(4px);
  animation: fadeIn 0.3s ease-out;
}

@keyframes fadeIn {
  from { opacity: 0; }
  to { opacity: 1; }
}

/* ======== 对话框主体 ======== */
.update-dialog {
  background-color: var(--md-sys-color-surface-container, var(--md-sys-color-surface));
  color: var(--md-sys-color-on-surface);
  border-radius: var(--md-sys-shape-corner-large);
  box-shadow: var(--shadow-strong);
  max-width: 500px;
  width: 90%;
  display: flex;
  flex-direction: column;
  animation: slideUp 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  border: 1px solid var(--md-sys-color-outline-variant, var(--md-sys-color-outline));
}

@keyframes slideUp {
  from { opacity: 0; transform: translateY(30px); }
  to { opacity: 1; transform: translateY(0); }
}

/* ======== Header ======== */
.dialog-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  padding: 24px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.dialog-header h2 {
  margin: 0 0 8px 0;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.dialog-header p {
  margin: 0;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
}

.close-btn {
  flex-shrink: 0;
  margin-left: 16px;
}

/* ======== Content ======== */
.dialog-content {
  flex: 1;
  padding: 24px;
  max-height: 300px;
  overflow-y: auto;
}

/* 错误消息 */
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

/* 下载进度 */
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

/* 更新日志区域 */
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

.notes-content {
  max-height: 200px;
  overflow-y: auto;
}

/* ======== Markdown 渲染样式 ======== */
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

.markdown-body :deep(h1) { font-size: 1.3em; }
.markdown-body :deep(h2) { font-size: 1.15em; }
.markdown-body :deep(h3) { font-size: 1.05em; }
.markdown-body :deep(h4) { font-size: 1em; }

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
  background-color: var(--md-sys-color-surface-container-highest, var(--md-sys-color-surface-variant));
  color: var(--md-sys-color-primary);
}

.markdown-body :deep(pre) {
  margin: 8px 0;
  padding: 12px;
  border-radius: 8px;
  background-color: var(--md-sys-color-surface-container-highest, var(--md-sys-color-surface-variant));
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

.markdown-body :deep(a:hover) {
  text-decoration: underline;
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

/* ======== Footer ======== */
.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding: 16px 24px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.text-button,
.filled-button {
  display: flex;
  align-items: center;
  gap: 8px;
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* 禁用状态 */
.text-button:disabled,
.filled-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Scrollbar */
.dialog-content::-webkit-scrollbar,
.notes-content::-webkit-scrollbar {
  width: 6px;
}

.dialog-content::-webkit-scrollbar-track,
.notes-content::-webkit-scrollbar-track {
  background: transparent;
}

.dialog-content::-webkit-scrollbar-thumb,
.notes-content::-webkit-scrollbar-thumb {
  background: var(--md-sys-color-outline-variant);
  border-radius: 3px;
}

.dialog-content::-webkit-scrollbar-thumb:hover,
.notes-content::-webkit-scrollbar-thumb:hover {
  background: var(--md-sys-color-outline);
}
</style>
