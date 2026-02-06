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

.setting-description {
  display: block;
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin-top: 4px;
}

.switch {
  position: relative;
  width: 52px;
  height: 32px;
  flex-shrink: 0;
  cursor: pointer;
}

.switch-track {
  position: absolute;
  width: 100%;
  height: 100%;
  background-color: var(--md-sys-color-surface-container-highest);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 16px;
  box-sizing: border-box;
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
  left: 22px;
  width: 24px;
  height: 24px;
  background-color: var(--md-sys-color-on-primary);
}

</style>
