<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.developerOptions') }}</h3>
    </div>

    <!-- 调试选项 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.debugOptions') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.glassEffect') }}</span>
          <div class="setting-desc">{{ $t('config.glassEffectDesc') }}</div>
        </div>
        <SettingSwitch
          :model-value="themeStore.enableGlassEffect"
          @update:model-value="toggleGlassEffect"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.gradients') }}</span>
          <div class="setting-desc">{{ $t('config.gradientsDesc') }}</div>
        </div>
        <SettingSwitch
          :model-value="themeStore.enableGradients"
          @update:model-value="toggleGradients"
        />
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.logLevel') }}</span>
          <div class="setting-desc">{{ $t('config.logLevelDesc') }}</div>
        </div>
        <MD3Select v-model="logLevel" :options="logLevelOptions" @change="changeLogLevel" />
      </div>
    </div>

    <!-- 系统信息 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.systemInfo') }}</h4>

      <div v-if="systemInfo" class="info-card">
        <div v-for="(value, key) in systemInfo" :key="key" class="info-row">
          <span class="info-label">{{ systemInfoLabel(key as string) }}</span>
          <span class="info-value">{{ value }}</span>
        </div>
      </div>
      <div v-else class="info-card">
        <span class="info-empty">{{ $t('config.systemInfoUnavailable') }}</span>
      </div>
    </div>

    <!-- 维护操作 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.maintenance') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.flushMetadataCache') }}</span>
          <div class="setting-desc">{{ $t('config.flushMetadataCacheDesc') }}</div>
        </div>
        <button class="action-btn" :disabled="isFlushingCache" @click="flushMetadataCache">
          {{ isFlushingCache ? $t('config.working') : $t('config.runAction') }}
        </button>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.clearLastSession') }}</span>
          <div class="setting-desc">{{ $t('config.clearLastSessionDesc') }}</div>
        </div>
        <button class="action-btn" :disabled="isClearingSession" @click="clearLastSession">
          {{ isClearingSession ? $t('config.working') : $t('config.runAction') }}
        </button>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.resetAllConfig') }}</span>
          <div class="setting-desc">{{ $t('config.resetAllConfigDesc') }}</div>
        </div>
        <button class="action-btn danger" :disabled="isResettingConfig" @click="resetAllConfig">
          {{ isResettingConfig ? $t('config.working') : $t('config.runAction') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ask } from '@tauri-apps/plugin-dialog'
import { useThemeStore } from '../../stores/theme'
import { useConfigStore } from '../../stores/config'
import logger, { LogLevel } from '../../utils/logger'
import MD3Select from '../MD3Select.vue'
import SettingSwitch from './SettingSwitch.vue'
import {
  clearLastSession as clearLastSessionCommand,
  flushMetadataCache as flushMetadataCacheCommand,
  getSystemInfo,
} from '../../services/appService'

const { t } = useI18n()
const themeStore = useThemeStore()
const configStore = useConfigStore()

// 切换后立即重新生成并应用主题 CSS 变量
const toggleGlassEffect = (): void => {
  themeStore.setGlassEffect(!themeStore.enableGlassEffect)
  themeStore.applyTheme()
}

const toggleGradients = (): void => {
  themeStore.setGradients(!themeStore.enableGradients)
  themeStore.applyTheme()
}

// 日志等级:logger 单例持有当前等级,设置后立即生效并持久化。
// 注意必须用本地 ref 保存选中值:logger.getMinLevel() 非响应式,
// 若用可写 computed 代理它,Vue 不会在 setter 后失效缓存,下拉框显示不会更新
const logLevelOptions = [
  { value: LogLevel.DEBUG, label: 'DEBUG' },
  { value: LogLevel.INFO, label: 'INFO' },
  { value: LogLevel.WARN, label: 'WARN' },
  { value: LogLevel.ERROR, label: 'ERROR' },
  { value: LogLevel.NONE, label: 'NONE' },
]

const logLevel = ref<LogLevel>(logger.getMinLevel())

const changeLogLevel = (value: string | number): void => {
  logLevel.value = value as LogLevel
  logger.setMinLevel(value as LogLevel)
}

const systemInfo = ref<Record<string, string> | null>(null)
const isFlushingCache = ref(false)
const isClearingSession = ref(false)
const isResettingConfig = ref(false)

const SYSTEM_INFO_LABELS: Record<string, string> = {
  os: 'config.sysOs',
  arch: 'config.sysArch',
  family: 'config.sysFamily',
  music_dir: 'config.sysMusicDir',
}

const systemInfoLabel = (key: string): string => {
  const labelKey = SYSTEM_INFO_LABELS[key]
  return labelKey ? t(labelKey) : key
}

const loadSystemInfo = async (): Promise<void> => {
  try {
    systemInfo.value = await getSystemInfo()
  } catch (error) {
    logger.error('Failed to load system info:', error)
    systemInfo.value = null
  }
}

const flushMetadataCache = async (): Promise<void> => {
  if (isFlushingCache.value) return
  isFlushingCache.value = true
  try {
    await flushMetadataCacheCommand()
    logger.info('Metadata cache flushed to disk')
  } catch (error) {
    logger.error('Failed to flush metadata cache:', error)
  } finally {
    isFlushingCache.value = false
  }
}

const clearLastSession = async (): Promise<void> => {
  if (isClearingSession.value) return
  isClearingSession.value = true
  try {
    await clearLastSessionCommand()
    logger.info('Last session cleared')
  } catch (error) {
    logger.error('Failed to clear last session:', error)
  } finally {
    isClearingSession.value = false
  }
}

const resetAllConfig = async (): Promise<void> => {
  if (isResettingConfig.value) return
  const confirmed = await ask(t('config.resetAllConfigConfirm'), {
    title: t('config.resetAllConfig'),
    kind: 'warning',
  })
  if (!confirmed) return

  isResettingConfig.value = true
  try {
    configStore.resetToDefaults()
    logger.info('Configuration reset to defaults')
  } catch (error) {
    logger.error('Failed to reset config:', error)
  } finally {
    isResettingConfig.value = false
  }
}

onMounted(() => {
  void loadSystemInfo()
})
</script>

<style scoped>
.tab-content {
  max-width: 720px;
}

.content-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

.settings-section {
  margin-bottom: 32px;
}

.section-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-primary);
  margin: 0 0 16px 16px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.setting-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  margin-bottom: 2px;
  border-radius: 12px;
  transition: background-color 0.2s ease;
}

.setting-item:hover {
  background-color: var(--md-sys-color-surface-container);
}

.setting-info {
  flex: 1;
  min-width: 0;
}

.setting-label {
  font-size: 16px;
  color: var(--md-sys-color-on-surface);
}

.setting-desc {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin-top: 2px;
}

.info-card {
  padding: 8px 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
}

.info-row {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 10px 0;
}

.info-row + .info-row {
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.info-label {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  flex-shrink: 0;
}

.info-value {
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
  word-break: break-all;
  text-align: right;
}

.info-empty {
  display: block;
  padding: 12px 0;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
}

.action-btn {
  padding: 8px 16px;
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  border: none;
  border-radius: 20px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
  flex-shrink: 0;
}

.action-btn:hover:not(:disabled) {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.action-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.action-btn.danger {
  background-color: var(--md-sys-color-error);
  color: var(--md-sys-color-on-error);
}

.action-btn.danger:hover:not(:disabled) {
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}
</style>
