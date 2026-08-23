<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.generalSettings') }}</h3>
    </div>

    <!-- 配置设置 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.configSettings') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.startupLoadLastConfig') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.general.startupLoadLastConfig }"
          @click="toggleSetting('startupLoadLastConfig')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveConfig') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.general.autoSaveConfig }"
          @click="toggleSetting('autoSaveConfig')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>

    <!-- 目录扫描设置 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.directoryScan') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableSubdirectoryScan') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.directoryScan.enableSubdirectoryScan }"
          @click="toggleDirectoryScan('enableSubdirectoryScan')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div v-if="configStore.directoryScan.enableSubdirectoryScan" class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.maxDepth') }}</span>
        </div>
        <div class="number-input">
          <button
            class="number-btn"
            :disabled="configStore.directoryScan.maxDepth <= 1"
            @click="decreaseMaxDepth"
          >
            <span class="material-symbols-rounded">remove</span>
          </button>
          <span class="number-value">{{ configStore.directoryScan.maxDepth }}</span>
          <button
            class="number-btn"
            :disabled="configStore.directoryScan.maxDepth >= 10"
            @click="increaseMaxDepth"
          >
            <span class="material-symbols-rounded">add</span>
          </button>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.ignoreHiddenFolders') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.directoryScan.ignoreHiddenFolders }"
          @click="toggleDirectoryScan('ignoreHiddenFolders')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.display') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showAudioInfo') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.general.showAudioInfo }"
          @click="toggleSetting('showAudioInfo')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showQueueInfo') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.general.showQueueInfo }"
          @click="toggleSetting('showQueueInfo')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.immersiveColorScheme') }}</span>
          <div class="setting-desc">{{ $t('config.immersiveColorSchemeDesc') }}</div>
        </div>
        <MD3Select
          v-model="immersiveColorScheme"
          :options="immersiveSchemeOptions"
          @change="handleImmersiveSchemeChange"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableAutoUpdate') }}</span>
          <div class="setting-desc">{{ $t('config.enableAutoUpdateDesc') }}</div>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.general.enableAutoUpdate }"
          @click="toggleSetting('enableAutoUpdate')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.language') }}</span>
        </div>
        <MD3Select
          v-model="configStore.general.language"
          :options="languageOptions"
          @change="handleLanguageChange"
        />
      </div>
    </div>

    <!-- 缓存设置 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.cacheSettings') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.coverCacheSize') }}</span>
          <div class="setting-desc">
            {{ $t('config.coverCacheSizeDesc') }}
          </div>
        </div>
        <div class="cache-size-control">
          <input
            v-model.number="coverCacheSizeMb"
            type="range"
            min="1024"
            max="8192"
            step="512"
            class="cache-slider"
            :style="cacheSliderStyle"
            @input="handleCacheSizeChange"
          />
          <span class="cache-size-value">{{ formatCacheSize(coverCacheSizeMb) }}</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.coverCachePath') }}</span>
          <div class="setting-desc">
            {{ $t('config.coverCachePathDesc') }}
          </div>
        </div>
        <div class="cache-path-control">
          <span class="cache-path-value">{{ coverCachePathDisplay }}</span>
          <button class="cache-path-btn" @click="selectCachePath">
            <span class="material-symbols-rounded">folder_open</span>
          </button>
          <button
            v-if="coverCachePath"
            class="cache-path-btn"
            :title="$t('config.restoreDefault')"
            @click="resetCachePath"
          >
            <span class="material-symbols-rounded">refresh</span>
          </button>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.clearCoverCache') }}</span>
          <div class="setting-desc">
            {{ $t('config.clearCoverCacheDesc') }}
          </div>
        </div>
        <button class="clear-cache-btn" :disabled="isClearingCache" @click="clearCoverCache">
          <span v-if="!isClearingCache">{{ $t('config.clearCache') }}</span>
          <span v-else>{{ $t('config.clearing') }}</span>
        </button>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.metadataCache') }}</span>
          <div class="setting-desc">{{ metadataCacheDesc }}</div>
        </div>
        <button
          class="clear-cache-btn"
          :disabled="isClearingMetadataCache"
          @click="clearMetadataCache"
        >
          <span v-if="!isClearingMetadataCache">{{ $t('config.clearCache') }}</span>
          <span v-else>{{ $t('config.clearing') }}</span>
        </button>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.fontCache') }}</span>
          <div class="setting-desc">{{ fontCacheDesc }}</div>
        </div>
        <button class="clear-cache-btn" :disabled="isClearingFontCache" @click="clearFontCaches">
          <span v-if="!isClearingFontCache">{{ $t('config.clearCache') }}</span>
          <span v-else>{{ $t('config.clearing') }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import { setLocale } from '../../i18n'
import logger from '../../utils/logger'
import MD3Select from '../MD3Select.vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import type { ImmersiveColorScheme } from '../../types'

const configStore = useConfigStore()
const { t } = useI18n()

const languageOptions = computed(() => [
  { value: 'zh', label: '中文' },
  { value: 'en', label: 'English' },
])

const immersiveSchemeOptions = computed(() => [
  { value: 'album', label: t('config.immersiveSchemeAlbum') },
  { value: 'fusion', label: t('config.immersiveSchemeFusion') },
])

const immersiveColorScheme = computed({
  get: () => (configStore.general.immersiveColorScheme ?? 'album') as ImmersiveColorScheme,
  set: (value: string | number) => {
    if (value === 'album' || value === 'fusion') {
      configStore.general.immersiveColorScheme = value
    }
  },
})

const handleImmersiveSchemeChange = async () => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save immersive color scheme:', error)
  }
}

const coverCacheSizeMb = computed({
  get: () => configStore.general.coverCacheSizeMb || 1024,
  set: (value: number) => {
    configStore.general.coverCacheSizeMb = value
  },
})

const cacheSliderStyle = computed(() => {
  const min = 1024
  const max = 8192
  const value = coverCacheSizeMb.value
  const percentage = ((value - min) / (max - min)) * 100
  return {
    background: `linear-gradient(to right, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) ${percentage}%, var(--md-sys-color-surface-variant) ${percentage}%, var(--md-sys-color-surface-variant) 100%)`,
  }
})

const coverCachePath = computed({
  get: () => configStore.general.coverCachePath,
  set: (value: string | undefined) => {
    configStore.general.coverCachePath = value
  },
})

const coverCachePathDisplay = computed(() => {
  if (!coverCachePath.value) {
    return tempDirPath.value
  }
  // 缩短路径显示
  const path = coverCachePath.value
  if (path.length > 30) {
    return '...' + path.slice(-30)
  }
  return path
})

const tempDirPath = ref('')

const loadTempDirPath = async () => {
  try {
    tempDirPath.value = await invoke<string>('get_temp_dir_command')
  } catch (error) {
    tempDirPath.value = t('config.systemTempDir')
    logger.error('Failed to get temp dir:', error)
  }
}

const isClearingCache = ref(false)
const isClearingMetadataCache = ref(false)
const metadataCacheStats = ref({ count: 0, size: 0 })

const metadataCacheDesc = computed(() => {
  const { count, size } = metadataCacheStats.value
  if (count === 0) {
    return t('config.metadataCacheEmpty')
  }
  const sizeStr =
    size > 1024 * 1024 ? `${(size / 1024 / 1024).toFixed(2)} MB` : `${(size / 1024).toFixed(2)} KB`
  return t('config.metadataCacheStats', { count, size: sizeStr })
})

const loadMetadataCacheStats = async () => {
  try {
    const [count, size] = await invoke<[number, number]>('get_metadata_cache_stats_command')
    metadataCacheStats.value = { count, size }
  } catch (error) {
    logger.error('Failed to load metadata cache stats:', error)
  }
}

const isClearingFontCache = ref(false)
const fontCacheStats = ref({ extractCacheBytes: 0 })

const fontCacheDesc = computed(() => {
  const size = fontCacheStats.value.extractCacheBytes
  if (size === 0) {
    return t('config.fontCacheEmpty')
  }
  const sizeStr =
    size > 1024 * 1024 ? `${(size / 1024 / 1024).toFixed(2)} MB` : `${(size / 1024).toFixed(2)} KB`
  return t('config.fontCacheStats', { size: sizeStr })
})

const loadFontCacheStats = async () => {
  try {
    fontCacheStats.value = await invoke<{ extractCacheBytes: number }>('get_font_cache_stats')
  } catch (error) {
    logger.error('Failed to load font cache stats:', error)
  }
}

const clearFontCaches = async () => {
  if (isClearingFontCache.value) return

  isClearingFontCache.value = true
  try {
    fontCacheStats.value = await invoke<{ extractCacheBytes: number }>('clear_font_caches')
    logger.info('Font caches cleared')
  } catch (error) {
    logger.error('Failed to clear font caches:', error)
  } finally {
    isClearingFontCache.value = false
  }
}

const saveConfig = async () => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (key: string) => {
  ;(configStore.general as Record<string, unknown>)[key] = !(
    configStore.general as Record<string, unknown>
  )[key]
  await saveConfig()
}

const toggleDirectoryScan = async (key: string) => {
  ;(configStore.directoryScan as Record<string, unknown>)[key] = !(
    configStore.directoryScan as Record<string, unknown>
  )[key]
  configStore.setDirectoryScanConfig(configStore.directoryScan)
}

const increaseMaxDepth = () => {
  if (configStore.directoryScan.maxDepth < 10) {
    configStore.directoryScan.maxDepth++
    configStore.setDirectoryScanConfig(configStore.directoryScan)
  }
}

const decreaseMaxDepth = () => {
  if (configStore.directoryScan.maxDepth > 1) {
    configStore.directoryScan.maxDepth--
    configStore.setDirectoryScanConfig(configStore.directoryScan)
  }
}

const handleLanguageChange = async () => {
  try {
    setLocale(configStore.general.language)
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to change language:', error)
  }
}

const handleCacheSizeChange = async () => {
  await saveConfig()
}

const formatCacheSize = (mb: number): string => {
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(1)} GB`
  }
  return `${mb} MB`
}

const clearCoverCache = async () => {
  if (isClearingCache.value) return

  isClearingCache.value = true
  try {
    const count = await invoke<number>('clean_cover_cache_command', {
      maxCacheSizeMb: configStore.general.coverCacheSizeMb,
    })
    logger.info(`Cleaned ${count} cover cache files`)
  } catch (error) {
    logger.error('Failed to clear cover cache:', error)
  } finally {
    isClearingCache.value = false
  }
}

const selectCachePath = async () => {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('config.selectCoverCacheDir'),
    })
    if (selected && typeof selected === 'string') {
      coverCachePath.value = selected
      // 通知后端更新缓存路径
      await invoke('set_cover_cache_path_command', { path: selected })
      await saveConfig()
    }
  } catch (error) {
    logger.error('Failed to select cache path:', error)
  }
}

const resetCachePath = async () => {
  coverCachePath.value = undefined
  // 通知后端恢复默认路径
  await invoke('set_cover_cache_path_command', { path: null })
  await saveConfig()
}

const clearMetadataCache = async () => {
  if (isClearingMetadataCache.value) return

  isClearingMetadataCache.value = true
  try {
    await invoke('clear_metadata_cache_command')
    logger.info('Metadata cache cleared')
    await loadMetadataCacheStats()
  } catch (error) {
    logger.error('Failed to clear metadata cache:', error)
  } finally {
    isClearingMetadataCache.value = false
  }
}

onMounted(() => {
  loadMetadataCacheStats()
  loadFontCacheStats()
  loadTempDirPath()
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

.switch {
  position: relative;
  width: 52px;
  height: 28px;
  flex-shrink: 0;
  cursor: pointer;
}

.switch-track {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--md-sys-color-surface-container);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 14px;
  transition: all 0.2s ease;
}

.switch.active .switch-track {
  background-color: var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.switch-handle {
  position: absolute;
  top: 50%;
  left: 6px;
  transform: translateY(-50%);
  width: 16px;
  height: 16px;
  background-color: var(--md-sys-color-outline);
  border-radius: 50%;
  transition: all 0.2s ease;
}

.switch.active .switch-handle {
  left: 28px;
  width: 18px;
  height: 18px;
  background-color: var(--md-sys-color-on-primary);
}

/* 设置描述文字 */
.setting-desc {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin-top: 2px;
}

/* 数字输入控件 */
.number-input {
  display: flex;
  align-items: center;
  gap: 4px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 20px;
  padding: 4px;
}

.number-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: 50%;
  background-color: transparent;
  color: var(--md-sys-color-on-surface);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.number-btn:hover:not(:disabled) {
  background-color: var(--md-sys-color-surface-container);
}

.number-btn:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.number-btn .material-symbols-rounded {
  font-size: 20px;
}

.number-value {
  min-width: 28px;
  text-align: center;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

/* 缓存大小控制 */
.cache-size-control {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 200px;
}

.cache-slider {
  flex: 1;
  height: 4px;
  border-radius: 2px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}

.cache-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 20px;
  height: 20px;
  background: var(--md-sys-color-primary);
  border-radius: 50%;
  cursor: pointer;
  transition: transform 0.2s ease;
}

.cache-slider::-webkit-slider-thumb:hover {
  transform: scale(1.1);
}

.cache-slider::-moz-range-thumb {
  width: 20px;
  height: 20px;
  background: var(--md-sys-color-primary);
  border-radius: 50%;
  cursor: pointer;
  border: none;
}

.cache-size-value {
  /* min-width: 60px; */
  text-align: right;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

/* 缓存路径控制 */
.cache-path-control {
  display: flex;
  align-items: center;
  gap: 8px;
  /* min-width: 200px; */
}

.cache-path-value {
  flex: 1;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  /* max-width: 200px; */
}

.cache-path-btn {
  padding: 8px;
  background-color: transparent;
  color: var(--md-sys-color-primary);
  border: none;
  border-radius: 50%;
  cursor: pointer;
  transition: all 0.2s ease;
  display: flex;
  align-items: center;
  justify-content: center;
}

.cache-path-btn:hover {
  background-color: var(--md-sys-color-primary-container);
}

.cache-path-btn .material-symbols-rounded {
  font-size: 20px;
}

/* 清理缓存按钮 */
.clear-cache-btn {
  padding: 8px 16px;
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  border: none;
  border-radius: 20px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
}

.clear-cache-btn:hover:not(:disabled) {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.clear-cache-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
</style>
