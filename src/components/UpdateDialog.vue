<template>
  <div v-if="updateAvailable" class="update-dialog-overlay">
    <div class="update-dialog">
      <!-- Header -->
      <div class="dialog-header">
        <div class="flex-1">
          <h2 class="text-title">{{ t('config.update.available') }}</h2>
          <p class="text-secondary">{{ t('config.update.newVersionAvailable', { version: newVersion }) }}</p>
        </div>
        <button class="icon-button close-btn" @click="onDismiss">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>

      <!-- Content -->
      <div class="dialog-content">
        <!-- Error Message -->
        <div v-if="error" class="error-message">
          <span class="material-symbols-rounded">error</span>
          <span>{{ error }}</span>
        </div>

        <!-- Download Progress -->
        <div v-if="isDownloading" class="download-section">
          <div class="progress-info">
            <span>{{ t('config.update.downloading') }}</span>
            <span class="progress-percent">{{ downloadProgress }}%</span>
          </div>
          <div class="progress-bar">
            <div class="progress-bar-fill" :style="{ width: downloadProgress + '%' }"></div>
          </div>
        </div>

        <!-- Release Notes Preview -->
        <div v-else class="release-notes">
          <h3>{{ t('config.update.releaseNotes') }}</h3>
          <div class="notes-content">
            <p v-if="releaseNotes" class="notes-text">{{ releaseNotes }}</p>
            <p v-else class="text-secondary">{{ t('config.update.noReleaseNotes') }}</p>
          </div>
        </div>
      </div>

      <!-- Footer -->
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
import { computed, ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'

interface Props {
  visible?: boolean
}

interface Emits {
  (e: 'update'): void
  (e: 'dismiss'): void
}

defineProps<Props>()
const emit = defineEmits<Emits>()

const { t } = useI18n()

// 导入 composable
import { useAutoUpdate } from '@/composables/useAutoUpdate'

const {
  updateAvailable,
  newVersion,
  downloadProgress,
  isDownloading,
  error,
  releaseNotes,
  installerPath,
  downloadFinished,
  updateLog,
  hasError,
  downloadAndInstall,
  runInstaller,
  resetUpdateState,
} = useAutoUpdate()

const onUpdate = async () => {
  try {
    if (!downloadFinished.value) {
      await downloadAndInstall()
    } else {
      await runInstaller()
    }
    emit('update')
  } catch (err) {
    console.error('Update failed:', err)
  }
}

const onDismiss = () => {
  resetUpdateState()
  emit('dismiss')
}
</script>

<style scoped>
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
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

  .update-dialog {
  background-color: var(--md-sys-color-surface-container, #ffffff);
  border-radius: var(--md-sys-shape-corner-large);
  box-shadow: var(--shadow-strong);
  max-width: 500px;
  width: 90%;
  display: flex;
  flex-direction: column;
  animation: slideUp 0.3s cubic-bezier(0.34, 1.56, 0.64, 1);
  border: 1px solid var(--md-sys-color-outline, rgba(0,0,0,0.08));
}

  .update-dialog-overlay {
    background-color: rgba(0, 0, 0, 0.7);
  }

@keyframes slideUp {
  from {
    opacity: 0;
    transform: translateY(30px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

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

.dialog-content {
  flex: 1;
  padding: 24px;
  max-height: 300px;
  overflow-y: auto;
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

.release-notes {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.release-notes h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
}

.notes-content {
  max-height: 200px;
  overflow-y: auto;
}

.notes-text {
  margin: 0;
  font-size: 14px;
  line-height: 1.6;
  color: var(--md-sys-color-on-surface);
  white-space: pre-wrap;
}

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
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

/* 禁用状态 */
.text-button:disabled,
.filled-button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Scrollbar 美化 */
::-webkit-scrollbar {
  width: 6px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: var(--md-sys-color-outline-variant);
  border-radius: 3px;
}

::-webkit-scrollbar-thumb:hover {
  background: var(--md-sys-color-outline);
}
</style>
