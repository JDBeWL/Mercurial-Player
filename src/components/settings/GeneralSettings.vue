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
        <SettingSwitch
          :model-value="configStore.general.startupLoadLastConfig"
          @update:model-value="toggleSetting('startupLoadLastConfig')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveConfig') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.general.autoSaveConfig"
          @update:model-value="toggleSetting('autoSaveConfig')"
        />
      </div>
    </div>

    <!-- 目录扫描设置 -->
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.directoryScan') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableSubdirectoryScan') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.directoryScan.enableSubdirectoryScan"
          @update:model-value="toggleDirectoryScan('enableSubdirectoryScan')"
        />
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
        <SettingSwitch
          :model-value="configStore.directoryScan.ignoreHiddenFolders"
          @update:model-value="toggleDirectoryScan('ignoreHiddenFolders')"
        />
      </div>
    </div>

    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.display') }}</h4>

      <!-- 界面字号：只有 Android 真正生效（原生侧接管 WebView textZoom，原理见 useAppFontScale）。
           桌面端仍显示该项，但后端是 no-op。 -->
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.interfaceFontSize') }}</span>
          <div class="setting-desc">{{ $t('config.interfaceFontSizeDesc') }}</div>
        </div>
        <div class="cache-size-control">
          <input
            v-model.number="uiFontScale"
            type="range"
            :min="UI_FONT_SCALE_MIN"
            :max="UI_FONT_SCALE_MAX"
            step="0.05"
            class="cache-slider"
            :style="uiFontScaleSliderStyle"
            @change="handleUIFontScaleChange"
          />
          <span class="cache-size-value ui-font-size-value">{{ uiFontScaleText }}</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showAudioInfo') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.general.showAudioInfo"
          @update:model-value="toggleSetting('showAudioInfo')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showQueueInfo') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.general.showQueueInfo ?? false"
          @update:model-value="toggleSetting('showQueueInfo')"
        />
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
          <span class="setting-label">{{ $t('config.immersiveAutoTheme') }}</span>
          <div class="setting-desc">{{ $t('config.immersiveAutoThemeDesc') }}</div>
        </div>
        <SettingSwitch
          :model-value="configStore.general.immersiveAutoTheme ?? true"
          @update:model-value="toggleSetting('immersiveAutoTheme')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableAutoUpdate') }}</span>
          <div class="setting-desc">{{ $t('config.enableAutoUpdateDesc') }}</div>
        </div>
        <SettingSwitch
          :model-value="configStore.general.enableAutoUpdate ?? false"
          @update:model-value="toggleSetting('enableAutoUpdate')"
        />
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

      <!-- setting-item-wide：这一行的右侧控件（缓存路径 + 两个按钮）在窄屏上是撑不满的，
           见文件末尾 orientation 媒体查询，竖屏下改成上下排。 -->
      <div class="setting-item setting-item-wide">
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
import { formatKbMb } from '../../utils/format'
import { saveConfigSafely } from '../../utils/errorMessages'
import { useSliderFill } from '../../composables/useSliderFill'
import MD3Select from '../MD3Select.vue'
import SettingSwitch from './SettingSwitch.vue'
import { useI18n } from 'vue-i18n'
import {
  cleanCoverCache,
  clearMetadataCache as clearMetadataCacheCommand,
  getMetadataCacheStats,
  getTempDir,
  setCoverCachePath,
} from '../../services/mediaService'
import {
  clearFontCaches as clearFontCachesCommand,
  getFontCacheStats,
} from '../../services/appService'
import { applyAppFontScale } from '../../composables/useAppFontScale'
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

const cacheFillPercent = useSliderFill(1024, 8192, coverCacheSizeMb)

const cacheSliderStyle = computed(() => {
  const percentage = cacheFillPercent.value
  return {
    background: `linear-gradient(to right, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) ${percentage}%, var(--md-sys-color-surface-variant) ${percentage}%, var(--md-sys-color-surface-variant) 100%)`,
  }
})

/* ===== 界面字号 =====
   上下限必须与原生侧 FontScaleBridge.MIN_SCALE / MAX_SCALE 一致，
   那边还会再 clamp 一次（超范围的值不会让界面崩掉，只会被收进这个区间）。 */
const UI_FONT_SCALE_MIN = 0.8
const UI_FONT_SCALE_MAX = 1.6

const uiFontScale = computed({
  get: () => configStore.ui?.fontScale ?? 1,
  set: (value: number) => {
    configStore.setUIFontScale(value)
  },
})

const uiFontScaleFillPercent = useSliderFill(UI_FONT_SCALE_MIN, UI_FONT_SCALE_MAX, uiFontScale)

const uiFontScaleSliderStyle = computed(() => {
  const percentage = uiFontScaleFillPercent.value
  return {
    background: `linear-gradient(to right, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) ${percentage}%, var(--md-sys-color-surface-variant) ${percentage}%, var(--md-sys-color-surface-variant) 100%)`,
  }
})

/** 100% = 设计稿原始大小 */
const uiFontScaleText = computed(() => `${Math.round(uiFontScale.value * 100)}%`)

/** 把当前倍率下发给原生侧 */
const handleUIFontScaleChange = (): void => {
  void applyAppFontScale(uiFontScale.value)
}

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
    tempDirPath.value = await getTempDir()
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
  return t('config.metadataCacheStats', { count, size: formatKbMb(size) })
})

const loadMetadataCacheStats = async () => {
  try {
    const [count, size] = await getMetadataCacheStats()
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
  return t('config.fontCacheStats', { size: formatKbMb(size) })
})

const loadFontCacheStats = async () => {
  try {
    fontCacheStats.value = await getFontCacheStats()
  } catch (error) {
    logger.error('Failed to load font cache stats:', error)
  }
}

const clearFontCaches = async () => {
  if (isClearingFontCache.value) return

  isClearingFontCache.value = true
  try {
    fontCacheStats.value = await clearFontCachesCommand()
    logger.info('Font caches cleared')
  } catch (error) {
    logger.error('Failed to clear font caches:', error)
  } finally {
    isClearingFontCache.value = false
  }
}

const saveConfig = () => saveConfigSafely(configStore)

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
    const count = await cleanCoverCache(configStore.general.coverCacheSizeMb)
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
      await setCoverCachePath(selected)
      await saveConfig()
    }
  } catch (error) {
    logger.error('Failed to select cache path:', error)
  }
}

const resetCachePath = async () => {
  coverCachePath.value = undefined
  // 通知后端恢复默认路径
  await setCoverCachePath(null)
  await saveConfig()
}

const clearMetadataCache = async () => {
  if (isClearingMetadataCache.value) return

  isClearingMetadataCache.value = true
  try {
    await clearMetadataCacheCommand()
    logger.info('Metadata cache cleared')
    await loadMetadataCacheStats()
  } catch (error) {
    logger.error('Failed to clear metadata cache:', error)
  } finally {
    isClearingMetadataCache.value = false
  }
}

onMounted(() => {
  void loadMetadataCacheStats()
  void loadFontCacheStats()
  void loadTempDirPath()
})
</script>

<style scoped>
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

@media (hover: hover) {
  .setting-item:hover {
    background-color: var(--md-sys-color-surface-container);
  }
}

.setting-info {
  flex: 1;
  min-width: 0;
}

.setting-label {
  font-size: 16px;
  color: var(--md-sys-color-on-surface);
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

@media (hover: hover) {
  .number-btn:hover:not(:disabled) {
    background-color: var(--md-sys-color-surface-container);
  }
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

@media (hover: hover) {
  .cache-slider::-webkit-slider-thumb:hover {
    transform: scale(1.1);
  }
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
  text-align: right;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

/* 百分比文案（"100%"）在拖动时会从 3 位变 4 位，留个下限宽度免得整行左右抖 */
.ui-font-size-value {
  min-width: 44px;
}

/* 缓存路径控制 */
.cache-path-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.cache-path-value {
  flex: 1;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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

@media (hover: hover) {
  .cache-path-btn:hover {
    background-color: var(--md-sys-color-primary-container);
  }
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

@media (hover: hover) {
  .clear-cache-btn:hover:not(:disabled) {
    background-color: var(--md-sys-color-primary-container);
    color: var(--md-sys-color-on-primary-container);
  }
}

.clear-cache-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

/* 窄屏（手机竖屏）：这一行右侧是"一长串路径 + 两个图标按钮"，横排时它的 max-content 宽度只
   能靠挤压左侧文字腾地方，.setting-info 会被压到 0px 而控件顶出屏幕。改成上下排：文字占满整
   行，路径单独一行并允许换行，按钮落到下一行右对齐。 */
@media (orientation: portrait) {
  .setting-item-wide {
    flex-direction: column;
    align-items: stretch;
    gap: 10px;
  }

  .setting-item-wide .cache-path-control {
    flex-wrap: wrap;
    /* 路径独占一行（flex-basis 100%），按钮因此被挤到第二行，靠右收尾 */
    justify-content: flex-end;
    gap: 6px;
  }

  .setting-item-wide .cache-path-value {
    flex: 1 1 100%;
    white-space: normal;
    /* 路径没有可断行的空格，只有 break-all 才能换行；
       用 anywhere 会在过窄时把单行压成极限窄，break-all 更稳 */
    word-break: break-all;
    line-height: 1.35;
    text-overflow: clip;
  }
}
</style>
