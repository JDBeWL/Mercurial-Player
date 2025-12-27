<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.generalSettings') }}</h3>
    </div>
    
    <div class="settings-section">
      <div class="setting-item" @click="toggleSetting('startupLoadLastConfig')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.startupLoadLastConfig') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.general.startupLoadLastConfig }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item" @click="toggleSetting('autoSaveConfig')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveConfig') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.general.autoSaveConfig }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item" @click="toggleSetting('showAudioInfo')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.showAudioInfo') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.general.showAudioInfo }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.display') || '显示' }}</h4>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.language') }}</span>
        </div>
        <select v-model="configStore.general.language" @change="handleLanguageChange" class="md3-select">
          <option value="zh">中文</option>
          <option value="en">English</option>
        </select>
      </div>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsAlignment') }}</span>
        </div>
        <select v-model="configStore.general.lyricsAlignment" @change="saveConfig" class="md3-select">
          <option value="left">{{ $t('config.alignLeft') }}</option>
          <option value="center">{{ $t('config.alignCenter') }}</option>
          <option value="right">{{ $t('config.alignRight') }}</option>
        </select>
      </div>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsFontFamily') }}</span>
        </div>
        <select v-model="configStore.general.lyricsFontFamily" @change="saveConfig" class="md3-select">
          <option v-for="font in systemFonts" :key="font" :value="font">{{ font }}</option>
        </select>
      </div>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.lyricsStyle') }}</span>
        </div>
        <select v-model="configStore.general.lyricsStyle" @change="saveConfig" class="md3-select">
          <option value="modern">{{ $t('config.lyricsStyleModern') }}</option>
          <option value="classic">{{ $t('config.lyricsStyleClassic') }}</option>
        </select>
      </div>
    </div>
    

  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { useConfigStore } from '../../stores/config'
import { invoke } from '@tauri-apps/api/core'
import { setLocale } from '../../i18n'
import logger from '../../utils/logger'

const configStore = useConfigStore()
const systemFonts = ref(['system-ui', 'sans-serif', 'serif', 'monospace'])

const loadSystemFonts = async () => {
  try {
    const fonts = await invoke('get_system_fonts')
    systemFonts.value = ['sans-serif', 'serif', 'monospace', ...fonts]
  } catch (error) {
    logger.error('Failed to load system fonts:', error)
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
  configStore.general[key] = !configStore.general[key]
  await saveConfig()
}

const handleLanguageChange = async () => {
  try {
    setLocale(configStore.general.language)
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to change language:', error)
  }
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
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.setting-item:hover {
  background-color: var(--md-sys-color-surface-container);
}

.setting-item.select {
  cursor: default;
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
  height: 32px;
  flex-shrink: 0;
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

.md3-select {
  min-width: 160px;
  padding: 12px 16px;
  padding-right: 40px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 8px;
  background-color: transparent;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'%3E%3Cpath fill='%23666' d='M7 10l5 5 5-5z'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  transition: border-color 0.2s ease, outline 0.2s ease;
  outline: 1px solid transparent;
  outline-offset: -1px;
}

.md3-select:hover {
  border-color: var(--md-sys-color-on-surface);
}

.md3-select:focus {
  outline: 1px solid var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}
</style>
