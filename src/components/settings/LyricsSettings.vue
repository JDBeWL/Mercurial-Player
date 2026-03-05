<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.lyricsSettings') }}</h3>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.onlineLyrics') || '在线歌词' }}</h4>
      
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableOnlineFetch') }}</span>
          <span class="setting-description">{{ $t('config.enableOnlineFetchDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.enableOnlineFetch }" @click="toggleSetting('enableOnlineFetch')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveOnlineLyrics') }}</span>
          <span class="setting-description">{{ $t('config.autoSaveOnlineLyricsDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.autoSaveOnlineLyrics }" @click="toggleSetting('autoSaveOnlineLyrics')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.preferTranslation') }}</span>
          <span class="setting-description">{{ $t('config.preferTranslationDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.preferTranslation }" @click="toggleSetting('preferTranslation')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.display') || '显示' }}</h4>
      
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
        </div>
        <MD3Select
          v-model="lyricsConfig.lyricsFontFamily"
          :options="fontOptions"
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

    <!-- 可视化设置 -->
    <div class="settings-section">
      <div class="section-header">
        <h4 class="section-title">{{ $t('config.visualizerSettings') }}</h4>
        <button class="icon-button" @click="detectScreenRefreshRate" :title="$t('config.detectRefreshRate')">
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

<script setup>
import { ref, computed, onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import logger from '../../utils/logger'
import MD3Select from '../MD3Select.vue'

const configStore = useConfigStore()
const { t } = useI18n()
const systemFonts = ref([])

const fpsOptions = computed(() => [
  { value: 30, label: '30 FPS' },
  { value: 60, label: '60 FPS' },
  { value: 120, label: '120 FPS' },
  { value: 144, label: '144 FPS' },
  { value: 165, label: '165 FPS' },
  { value: 240, label: '240 FPS' }
])

const alignmentOptions = computed(() => [
  { value: 'left', label: t('config.alignLeft') },
  { value: 'center', label: t('config.alignCenter') },
  { value: 'right', label: t('config.alignRight') }
])

const fontOptions = computed(() => {
  // 始终包含 Roboto 作为默认选项
  const fonts = ['Roboto', 'sans-serif', 'serif', 'monospace', ...systemFonts.value]
  // 去重
  const uniqueFonts = [...new Set(fonts)]
  return uniqueFonts.map(font => ({ value: font, label: font }))
})

const styleOptions = computed(() => [
  { value: 'modern', label: t('config.lyricsStyleModern') },
  { value: 'classic', label: t('config.lyricsStyleClassic') }
])

const visualizerConfig = computed({
  get: () => {
    if (!configStore.visualizer) {
      configStore.visualizer = {
        targetFps: 60,
        enableVerticalSync: false,
        detectedRefreshRate: 60
      }
    }
    return configStore.visualizer
  },
  set: (value) => {
    configStore.visualizer = value
  }
})

// 当前刷新率从config读取
const currentRefreshRate = computed(() => {
  return visualizerConfig.value.detectedRefreshRate || 60
})

// 检测屏幕刷新率
const detectScreenRefreshRate = async () => {
  try {
    // 首先尝试从系统API获取刷新率
    const systemRefreshRate = await invoke('get_screen_refresh_rate')
    
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
    
    const measure = () => {
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
const applyRefreshRate = async (fps) => {
  try {
    await invoke('set_target_fps', { fps })
    logger.info(`Applied refresh rate: ${fps} FPS`)
  } catch (error) {
    logger.error('Failed to apply refresh rate:', error)
  }
}

// 处理 FPS 变化
const handleFpsChange = async () => {
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
const toggleVerticalSync = async () => {
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
const applyFpsBasedOnVsync = async () => {
  let fpsToApply = visualizerConfig.value.targetFps
  
  if (visualizerConfig.value.enableVerticalSync && visualizerConfig.value.detectedRefreshRate) {
    // 垂直同步开启：使用 min(目标帧率, 屏幕刷新率)
    fpsToApply = Math.min(visualizerConfig.value.targetFps, visualizerConfig.value.detectedRefreshRate)
    logger.info(`VSync enabled: using min(${visualizerConfig.value.targetFps}, ${visualizerConfig.value.detectedRefreshRate}) = ${fpsToApply} FPS`)
  } else {
    // 垂直同步关闭：使用目标帧率
    logger.info(`VSync disabled: using target FPS ${fpsToApply}`)
  }
  
  await applyRefreshRate(fpsToApply)
}

const lyricsConfig = computed({
  get: () => {
    // 确保 lyrics 配置存在且包含所有必需字段
    if (!configStore.lyrics) {
      configStore.lyrics = {
        enableOnlineFetch: false,
        autoSaveOnlineLyrics: true,
        preferTranslation: true,
        onlineSource: 'netease',
        lyricsAlignment: 'center',
        lyricsFontFamily: 'Roboto',
        lyricsStyle: 'modern'
      }
    } else {
      // 确保所有字段都存在
      if (!configStore.lyrics.lyricsAlignment) {
        configStore.lyrics.lyricsAlignment = 'center'
      }
      if (!configStore.lyrics.lyricsFontFamily) {
        configStore.lyrics.lyricsFontFamily = 'Roboto'
      }
      if (!configStore.lyrics.lyricsStyle) {
        configStore.lyrics.lyricsStyle = 'modern'
      }
    }
    return configStore.lyrics
  },
  set: (value) => {
    configStore.lyrics = value
  }
})

const loadSystemFonts = async () => {
  try {
    const fonts = await invoke('get_system_fonts')
    // 过滤掉已经在默认列表中的字体
    const defaultFonts = ['Roboto', 'sans-serif', 'serif', 'monospace']
    systemFonts.value = fonts.filter(font => !defaultFonts.includes(font))
    logger.info(`Loaded ${systemFonts.value.length} system fonts`)
  } catch (error) {
    logger.error('Failed to load system fonts:', error)
    // 失败时使用空数组，仍然可以使用默认字体
    systemFonts.value = []
  }
}

const saveConfig = async () => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (key) => {
  // 确保 lyrics 配置存在
  if (!configStore.lyrics) {
    configStore.lyrics = {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease',
      lyricsAlignment: 'center',
      lyricsFontFamily: 'Roboto',
      lyricsStyle: 'modern'
    }
  }
  configStore.lyrics[key] = !configStore.lyrics[key]
  await saveConfig()
}

onMounted(() => {
  loadSystemFonts()
  
  // 如果没有检测过刷新率，自动检测一次
  if (!visualizerConfig.value.detectedRefreshRate || visualizerConfig.value.detectedRefreshRate === 60) {
    detectScreenRefreshRate()
  }
  
  // 确保后端的FPS设置与配置同步
  if (visualizerConfig.value.targetFps) {
    // 根据垂直同步状态应用正确的FPS
    applyFpsBasedOnVsync().catch(error => {
      logger.error('Failed to sync FPS on mount:', error)
    })
  }
  
  // 确保后端的垂直同步设置与配置同步
  if (visualizerConfig.value.enableVerticalSync !== undefined) {
    invoke('set_vertical_sync', { enabled: visualizerConfig.value.enableVerticalSync }).catch(error => {
      logger.error('Failed to sync vertical sync on mount:', error)
    })
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
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-secondary-container));
}

.filled-tonal-button .material-symbols-rounded {
  font-size: 20px;
}

</style>
