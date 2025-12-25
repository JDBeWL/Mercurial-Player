<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.lyricsSettings') }}</h3>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.onlineLyrics') || '在线歌词' }}</h4>
      
      <div class="setting-item" @click="toggleSetting('enableOnlineFetch')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.enableOnlineFetch') }}</span>
          <span class="setting-description">{{ $t('config.enableOnlineFetchDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.enableOnlineFetch }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item" @click="toggleSetting('autoSaveOnlineLyrics')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.autoSaveOnlineLyrics') }}</span>
          <span class="setting-description">{{ $t('config.autoSaveOnlineLyricsDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.autoSaveOnlineLyrics }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item" @click="toggleSetting('preferTranslation')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.preferTranslation') }}</span>
          <span class="setting-description">{{ $t('config.preferTranslationDesc') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.lyrics?.preferTranslation }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { useConfigStore } from '../../stores/config'
import logger from '../../utils/logger'

const configStore = useConfigStore()

const saveConfig = async () => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (key) => {
  if (!configStore.lyrics) {
    configStore.lyrics = {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease'
    }
  }
  configStore.lyrics[key] = !configStore.lyrics[key]
  await saveConfig()
}
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
