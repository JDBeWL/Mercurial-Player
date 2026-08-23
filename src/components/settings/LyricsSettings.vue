<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.lyricsSettings') }}</h3>
    </div>

    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.onlineLyrics') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableOnlineFetch') }}</span>
          <span class="setting-description">{{ $t('config.enableOnlineFetchDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.lyrics?.enableOnlineFetch }"
          @click="toggleSetting('enableOnlineFetch')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveOnlineLyrics') }}</span>
          <span class="setting-description">{{ $t('config.autoSaveOnlineLyricsDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.lyrics?.autoSaveOnlineLyrics }"
          @click="toggleSetting('autoSaveOnlineLyrics')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.preferTranslation') }}</span>
          <span class="setting-description">{{ $t('config.preferTranslationDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.lyrics?.preferTranslation }"
          @click="toggleSetting('preferTranslation')"
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
          <span class="setting-label">{{ $t('config.showNoLyricsHint') }}</span>
          <span class="setting-description">{{ $t('config.showNoLyricsHintDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.lyrics?.showNoLyricsHint !== false }"
          @click="toggleSetting('showNoLyricsHint')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showFetchLyricsButton') }}</span>
          <span class="setting-description">{{ $t('config.showFetchLyricsButtonDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: configStore.lyrics?.showFetchLyricsButton !== false }"
          @click="toggleSetting('showFetchLyricsButton')"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsAlignment') }}</span>
        </div>
        <MD3Select
          v-model="lyricsConfig.lyricsAlignment"
          :options="alignmentOptions"
          @change="saveConfig"
        />
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsFontFamily') }}</span>
          <span class="setting-description">{{ $t('config.lyricsFontFamilyDesc') }}</span>
        </div>
        <MD3Select
          v-model="lyricsConfig.lyricsFontFamily"
          :options="fontOptions"
          @change="saveConfig"
        />
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.translationFontFamily') }}</span>
          <span class="setting-description">{{ $t('config.translationFontFamilyDesc') }}</span>
        </div>
        <MD3Select
          v-model="translationFontModel"
          :options="translationFontOptions"
          @change="saveConfig"
        />
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsStyle') }}</span>
        </div>
        <MD3Select
          v-model="lyricsConfig.lyricsStyle"
          :options="styleOptions"
          @change="saveConfig"
        />
      </div>
    </div>

    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.desktopLyrics') }}</h4>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableDesktopLyrics') }}</span>
          <span class="setting-description">{{ $t('config.enableDesktopLyricsDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: desktopLyricsConfig.enabled }"
          @click="toggleDesktopLyrics"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>

      <template v-if="desktopLyricsConfig.enabled">
        <div class="setting-item">
          <div class="setting-info">
            <span class="setting-label">{{ $t('config.lockDesktopLyrics') }}</span>
            <span class="setting-description">{{ $t('config.lockDesktopLyricsDesc') }}</span>
          </div>
          <div
            class="switch"
            :class="{ active: desktopLyricsConfig.locked }"
            @click="toggleDesktopLyricsLock"
          >
            <div class="switch-track"></div>
            <div class="switch-handle"></div>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-info">
            <span class="setting-label">{{ $t('config.desktopLyricsFontSize') }}</span>
            <span class="setting-description">{{ $t('config.desktopLyricsFontSizeDesc') }}</span>
          </div>
          <div class="font-size-control">
            <input
              type="range"
              class="font-size-slider"
              :min="16"
              :max="48"
              :step="1"
              :value="desktopLyricsConfig.fontSize"
              :style="fontSizeSliderStyle"
              @input="handleFontSizeChange"
            />
            <span class="font-size-value">{{ desktopLyricsConfig.fontSize }}px</span>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-info">
            <span class="setting-label">{{ $t('config.desktopLyricsColorPreset') }}</span>
            <span class="setting-description">{{ $t('config.desktopLyricsColorPresetDesc') }}</span>
          </div>
          <div class="preset-buttons">
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'auto' }"
              :title="$t('config.colorPresetAuto')"
              @click="setColorPreset('auto')"
            >
              <span class="preset-preview auto-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetAuto') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'dark' }"
              :title="$t('config.colorPresetDark')"
              @click="setColorPreset('dark')"
            >
              <span class="preset-preview dark-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetDark') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'light' }"
              :title="$t('config.colorPresetLight')"
              @click="setColorPreset('light')"
            >
              <span class="preset-preview light-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetLight') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'blue' }"
              :title="$t('config.colorPresetBlue')"
              @click="setColorPreset('blue')"
            >
              <span class="preset-preview blue-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetBlue') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'pink' }"
              :title="$t('config.colorPresetPink')"
              @click="setColorPreset('pink')"
            >
              <span class="preset-preview pink-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetPink') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'orange' }"
              :title="$t('config.colorPresetOrange')"
              @click="setColorPreset('orange')"
            >
              <span class="preset-preview orange-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetOrange') }}</span>
            </button>
            <button
              class="preset-btn"
              :class="{ active: desktopLyricsConfig.colorPreset === 'green' }"
              :title="$t('config.colorPresetGreen')"
              @click="setColorPreset('green')"
            >
              <span class="preset-preview green-preview"></span>
              <span class="preset-label">{{ $t('config.colorPresetGreen') }}</span>
            </button>
          </div>
        </div>
      </template>
    </div>

    <!-- 可视化设置 -->
    <div class="settings-section">
      <div class="section-header">
        <h4 class="section-title">{{ $t('config.visualizerSettings') }}</h4>
        <button
          class="icon-button"
          :title="$t('config.detectRefreshRate')"
          @click="detectScreenRefreshRate"
        >
          <span class="material-symbols-rounded">refresh</span>
        </button>
      </div>

      <div class="setting-item info-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.screenRefreshRate') }}</span>
          <span class="setting-value">{{ currentRefreshRate }} Hz</span>
        </div>
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.targetFps') }}</span>
          <span class="setting-description">{{ $t('config.targetFpsDesc') }}</span>
        </div>
        <MD3Select
          v-model="visualizerConfig.targetFps"
          :options="fpsOptions"
          @change="handleFpsChange"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableVerticalSync') }}</span>
          <span class="setting-description">{{ $t('config.enableVerticalSyncDesc') }}</span>
        </div>
        <div
          class="switch"
          :class="{ active: visualizerConfig.enableVerticalSync }"
          @click="toggleVerticalSync"
        >
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import logger from '../../utils/logger'
import {
  bundledFontOptions,
  externalFontOptions,
  loadExternalFonts,
} from '../../utils/bundledFonts'
import MD3Select from '../MD3Select.vue'
import type { LyricsConfig, DesktopLyricsConfig, VisualizerConfig } from '@/types'

const configStore = useConfigStore()
const { t } = useI18n()
const systemFonts = ref<string[]>([])

const fpsOptions = computed(() => [
  { value: 30, label: '30 FPS' },
  { value: 60, label: '60 FPS' },
  { value: 120, label: '120 FPS' },
  { value: 144, label: '144 FPS' },
  { value: 165, label: '165 FPS' },
  { value: 240, label: '240 FPS' },
])

const alignmentOptions = computed(() => [
  { value: 'left', label: t('config.alignLeft') },
  { value: 'center', label: t('config.alignCenter') },
  { value: 'right', label: t('config.alignRight') },
])

const fontOptions = computed(() => {
  // 通用字体 / 内置打包字体 / 外部字体（软件同级 fonts/）/ 系统字体（与内置重名的只保留靠前分组）
  const bundledValues = new Set(bundledFontOptions.map((f) => f.value))
  const externalOptions = externalFontOptions.value.filter((f) => !bundledValues.has(f.value))
  const externalValues = new Set([...bundledValues, ...externalOptions.map((f) => f.value)])
  const systemOptions = [...new Set(systemFonts.value)]
    .filter((font) => !externalValues.has(font))
    .map((font) => ({ value: font, label: font }))
  return [
    {
      label: t('config.fontGroupCommon'),
      options: ['Roboto', 'sans-serif', 'serif', 'monospace'].map((font) => ({
        value: font,
        label: font,
      })),
    },
    { label: t('config.fontGroupBundled'), options: bundledFontOptions },
    ...(externalOptions.length > 0
      ? [{ label: t('config.fontGroupExternal'), options: externalOptions }]
      : []),
    { label: t('config.fontGroupSystem'), options: systemOptions },
  ]
})

// 译文字体选项：在原文选项基础上多一个“跟随原文”（空值 = 继承原文歌词字体）
const translationFontOptions = computed(() => [
  { value: '', label: t('config.translationFontFollow') },
  ...fontOptions.value,
])

// 译文字体的 v-model 适配：可选字段归一为空串（跟随原文），MD3Select 只接受 string | number
const translationFontModel = computed<string>({
  get: () => lyricsConfig.value.translationFontFamily ?? '',
  set: (value: string) => {
    lyricsConfig.value.translationFontFamily = value
  },
})

const styleOptions = computed(() => [
  { value: 'modern', label: t('config.lyricsStyleModern') },
  { value: 'classic', label: t('config.lyricsStyleClassic') },
])

// 初始化默认值（在 setup 阶段同步执行，避免 computed 副作用）
if (!configStore.visualizer) {
  configStore.visualizer = {
    targetFps: 60,
    enableVerticalSync: false,
    detectedRefreshRate: 60,
  }
}

const visualizerConfig = computed<VisualizerConfig>({
  get: () => configStore.visualizer,
  set: (value: VisualizerConfig) => {
    configStore.visualizer = value
  },
})

// 当前刷新率从config读取
const currentRefreshRate = computed(() => {
  return visualizerConfig.value.detectedRefreshRate || 60
})

// 检测屏幕刷新率
const detectScreenRefreshRate = async (): Promise<void> => {
  try {
    // 首先尝试从系统API获取刷新率
    const systemRefreshRate = await invoke<number>('get_screen_refresh_rate')

    if (systemRefreshRate && systemRefreshRate > 0) {
      // 使用系统检测到的刷新率
      visualizerConfig.value.detectedRefreshRate = systemRefreshRate
      configStore._markDirty()

      logger.info(`Detected screen refresh rate from system: ${systemRefreshRate} Hz`)

      // 如果垂直同步已开启，重新应用FPS（可能需要调整）
      if (visualizerConfig.value.enableVerticalSync) {
        await applyFpsBasedOnVsync()
      }

      // 保存检测结果
      await saveConfig()
      return
    }

    // 如果系统API失败，回退到requestAnimationFrame方法
    logger.info('System API failed, falling back to requestAnimationFrame detection')
    let frames = 0
    const start = performance.now()

    const measure = (): void => {
      frames++
      if (performance.now() - start < 1000) {
        requestAnimationFrame(measure)
      } else {
        // 保存检测到的刷新率
        visualizerConfig.value.detectedRefreshRate = frames
        configStore._markDirty()

        logger.info(`Detected screen refresh rate from RAF: ${frames} Hz`)

        // 如果垂直同步已开启，重新应用FPS（可能需要调整）
        if (visualizerConfig.value.enableVerticalSync) {
          applyFpsBasedOnVsync()
        }

        // 保存检测结果
        saveConfig()
      }
    }
    requestAnimationFrame(measure)
  } catch (error) {
    logger.error('Failed to detect refresh rate:', error)
  }
}

// 应用刷新率到后端
const applyRefreshRate = async (fps: number): Promise<void> => {
  try {
    await invoke('set_target_fps', { fps })
    logger.info(`Applied refresh rate: ${fps} FPS`)
  } catch (error) {
    logger.error('Failed to apply refresh rate:', error)
  }
}

// 处理 FPS 变化
const handleFpsChange = async (): Promise<void> => {
  try {
    // 应用正确的FPS设置（考虑垂直同步）
    await applyFpsBasedOnVsync()
    configStore._markDirty()
    await saveConfig()
  } catch (error) {
    logger.error('Failed to set FPS:', error)
  }
}

// 切换垂直同步
const toggleVerticalSync = async (): Promise<void> => {
  visualizerConfig.value.enableVerticalSync = !visualizerConfig.value.enableVerticalSync
  try {
    await invoke('set_vertical_sync', { enabled: visualizerConfig.value.enableVerticalSync })

    // 应用正确的FPS设置
    await applyFpsBasedOnVsync()

    configStore._markDirty()
    await saveConfig()
  } catch (error) {
    logger.error('Failed to set vertical sync:', error)
    visualizerConfig.value.enableVerticalSync = !visualizerConfig.value.enableVerticalSync
  }
}

// 根据垂直同步状态应用FPS
const applyFpsBasedOnVsync = async (): Promise<void> => {
  let fpsToApply = visualizerConfig.value.targetFps

  if (visualizerConfig.value.enableVerticalSync && visualizerConfig.value.detectedRefreshRate) {
    // 垂直同步开启：使用 min(目标帧率, 屏幕刷新率)
    fpsToApply = Math.min(
      visualizerConfig.value.targetFps,
      visualizerConfig.value.detectedRefreshRate,
    )
    logger.info(
      `VSync enabled: using min(${visualizerConfig.value.targetFps}, ${visualizerConfig.value.detectedRefreshRate}) = ${fpsToApply} FPS`,
    )
  } else {
    // 垂直同步关闭：使用目标帧率
    logger.info(`VSync disabled: using target FPS ${fpsToApply}`)
  }

  await applyRefreshRate(fpsToApply)
}

// 初始化默认值（在 setup 阶段同步执行，避免 computed 副作用）
if (!configStore.lyrics) {
  configStore.lyrics = {
    enableOnlineFetch: false,
    autoSaveOnlineLyrics: true,
    preferTranslation: true,
    onlineSource: 'netease',
    lyricsAlignment: 'center',
    lyricsFontFamily: 'Noto Sans SC',
    translationFontFamily: '',
    lyricsStyle: 'modern',
    showNoLyricsHint: true,
    showFetchLyricsButton: true,
  }
} else {
  // 确保所有字段都存在
  if (!configStore.lyrics.lyricsAlignment) {
    configStore.lyrics.lyricsAlignment = 'center'
  }
  if (!configStore.lyrics.lyricsFontFamily) {
    configStore.lyrics.lyricsFontFamily = 'Noto Sans SC'
  }
  if (configStore.lyrics.translationFontFamily === undefined) {
    configStore.lyrics.translationFontFamily = ''
  }
  if (!configStore.lyrics.lyricsStyle) {
    configStore.lyrics.lyricsStyle = 'modern'
  }
  if (configStore.lyrics.showNoLyricsHint === undefined) {
    configStore.lyrics.showNoLyricsHint = true
  }
  if (configStore.lyrics.showFetchLyricsButton === undefined) {
    configStore.lyrics.showFetchLyricsButton = true
  }
}

const lyricsConfig = computed<LyricsConfig>({
  get: () => configStore.lyrics,
  set: (value: LyricsConfig) => {
    configStore.lyrics = value
  },
})

const loadSystemFonts = async (): Promise<void> => {
  try {
    const fonts = await invoke<string[]>('get_system_fonts')
    // 过滤掉已经在默认列表中的字体
    const defaultFonts = ['Roboto', 'sans-serif', 'serif', 'monospace']
    systemFonts.value = fonts.filter((font) => !defaultFonts.includes(font))
    logger.info(`Loaded ${systemFonts.value.length} system fonts`)
  } catch (error) {
    logger.error('Failed to load system fonts:', error)
    // 失败时使用空数组，仍然可以使用默认字体
    systemFonts.value = []
  }
}

const saveConfig = async (): Promise<void> => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (
  key:
    | 'enableOnlineFetch'
    | 'autoSaveOnlineLyrics'
    | 'preferTranslation'
    | 'showNoLyricsHint'
    | 'showFetchLyricsButton',
): Promise<void> => {
  // 确保 lyrics 配置存在
  if (!configStore.lyrics) {
    configStore.lyrics = {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease',
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Noto Sans SC',
      translationFontFamily: '',
      lyricsStyle: 'modern',
      showNoLyricsHint: true,
      showFetchLyricsButton: true,
    }
  }
  configStore.lyrics[key] = !configStore.lyrics[key]
  await saveConfig()
}

const desktopLyricsConfig = computed<DesktopLyricsConfig>({
  get: () => {
    if (!configStore.lyrics?.desktopLyrics) {
      return { enabled: false, locked: true, fontSize: 28, colorPreset: 'auto' as const }
    }
    return configStore.lyrics.desktopLyrics
  },
  set: (value: DesktopLyricsConfig) => {
    configStore.setDesktopLyricsConfig(value)
  },
})

const fontSizeSliderStyle = computed(() => {
  const min = 16
  const max = 48
  const value = desktopLyricsConfig.value.fontSize
  const percentage = ((value - min) / (max - min)) * 100
  return {
    background: `linear-gradient(to right, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) ${percentage}%, var(--md-sys-color-surface-variant) ${percentage}%, var(--md-sys-color-surface-variant) 100%)`,
  }
})

const toggleDesktopLyrics = async (): Promise<void> => {
  configStore.setDesktopLyricsConfig({ enabled: !desktopLyricsConfig.value.enabled })
  await saveConfig()
}

const toggleDesktopLyricsLock = async (): Promise<void> => {
  configStore.setDesktopLyricsConfig({ locked: !desktopLyricsConfig.value.locked })
  await saveConfig()
}

const handleFontSizeChange = async (event: Event): Promise<void> => {
  const size = parseInt((event.target as HTMLInputElement).value, 10)
  configStore.setDesktopLyricsConfig({ fontSize: size })
  await saveConfig()
}

const setColorPreset = async (preset: DesktopLyricsConfig['colorPreset']): Promise<void> => {
  configStore.setDesktopLyricsConfig({ colorPreset: preset })
  await saveConfig()
}

onMounted(() => {
  loadSystemFonts()
  // 每次打开设置页重新扫描外部字体目录，运行中放入的字体无需重启即可选择
  void loadExternalFonts()

  // 如果没有检测过刷新率，自动检测一次
  if (
    !visualizerConfig.value.detectedRefreshRate ||
    visualizerConfig.value.detectedRefreshRate === 60
  ) {
    detectScreenRefreshRate()
  }

  // 确保后端的FPS设置与配置同步
  if (visualizerConfig.value.targetFps) {
    // 根据垂直同步状态应用正确的FPS
    applyFpsBasedOnVsync().catch((error) => {
      logger.error('Failed to sync FPS on mount:', error)
    })
  }

  // 确保后端的垂直同步设置与配置同步
  if (visualizerConfig.value.enableVerticalSync !== undefined) {
    invoke('set_vertical_sync', { enabled: visualizerConfig.value.enableVerticalSync }).catch(
      (error) => {
        logger.error('Failed to sync vertical sync on mount:', error)
      },
    )
  }
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

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  padding: 0 16px;
}

.section-header .section-title {
  margin: 0;
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  background-color: transparent;
  color: var(--md-sys-color-on-surface-variant);
  border: none;
  border-radius: var(--md-sys-shape-corner-large, 12px);
  cursor: pointer;
  transition: all 0.2s ease;
}

.icon-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.icon-button .material-symbols-rounded {
  font-size: 24px;
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

.setting-description {
  display: block;
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin-top: 4px;
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
  background-color: var(--md-sys-color-surface-container-highest);
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

.setting-item.info-item {
  flex-direction: column;
  align-items: flex-start;
  gap: 12px;
}

.setting-item.info-item .setting-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}

.setting-value {
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-primary);
}

.filled-tonal-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 24px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.filled-tonal-button:hover {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
}

.filled-tonal-button .material-symbols-rounded {
  font-size: 20px;
}

.font-size-control {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 200px;
}

.font-size-slider {
  flex: 1;
  height: 4px;
  border-radius: 2px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
  cursor: pointer;
}

.font-size-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 20px;
  height: 20px;
  background: var(--md-sys-color-primary);
  border-radius: 50%;
  cursor: pointer;
  transition: transform 0.2s ease;
}

.font-size-slider::-webkit-slider-thumb:hover {
  transform: scale(1.1);
}

.font-size-slider::-moz-range-thumb {
  width: 20px;
  height: 20px;
  background: var(--md-sys-color-primary);
  border-radius: 50%;
  cursor: pointer;
  border: none;
}

.font-size-value {
  min-width: 48px;
  text-align: right;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.preset-buttons {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  flex-shrink: 0;
  justify-content: flex-end;
  max-width: 400px;
}

.preset-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid var(--md-sys-color-outline-variant);
  border-radius: var(--md-sys-shape-corner-small);
  background: var(--md-sys-color-surface-container-highest);
  color: var(--md-sys-color-on-surface);
  cursor: pointer;
  font-size: 13px;
  transition: all 0.2s ease;
}

.preset-btn:hover {
  background: var(--md-sys-color-surface-container);
}

.preset-btn.active {
  border-color: var(--md-sys-color-primary);
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.preset-preview {
  width: 20px;
  height: 20px;
  border-radius: 4px;
  border: 1px solid var(--md-sys-color-outline-variant);
}

.dark-preview {
  background: #1a1a1a;
}

.auto-preview {
  /* 半深半浅 */
  background: linear-gradient(135deg, #1a1a1a 50%, #f5f5f5 50%);
}

.light-preview {
  background: #f5f5f5;
}

.blue-preview {
  background: #0288d1;
}

.pink-preview {
  background: #e91e63;
}

.orange-preview {
  background: #f57c00;
}

.green-preview {
  background: #388e3c;
}

.preset-label {
  font-size: 12px;
}
</style>
