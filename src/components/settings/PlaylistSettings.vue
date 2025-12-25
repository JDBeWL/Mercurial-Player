<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.playlistSettings') }}</h3>
    </div>
    
    <div class="settings-section">
      <div class="setting-item" @click="toggleSetting('generateAllSongsPlaylist')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.generateAllSongsPlaylist') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.playlist.generateAllSongsPlaylist }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item" @click="toggleSetting('folderBasedPlaylists')">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.folderBasedPlaylists') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.playlist.folderBasedPlaylists }">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.sortOrder') }}</span>
        </div>
        <select v-model="configStore.playlist.sortOrder" @change="saveConfig" class="md3-select">
          <option value="asc">A-Z (升序)</option>
          <option value="desc">Z-A (降序)</option>
        </select>
      </div>
      
      <div class="setting-item input">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.playlistNameFormat') }}</span>
        </div>
        <input 
          type="text" 
          v-model="configStore.playlist.playlistNameFormat" 
          @change="saveConfig"
          placeholder="{folderName}"
          class="md3-input"
        >
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
  configStore.playlist[key] = !configStore.playlist[key]
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

.setting-item.select,
.setting-item.input {
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

.md3-input {
  min-width: 200px;
  padding: 12px 16px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 8px;
  background-color: transparent;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  transition: border-color 0.2s ease, outline 0.2s ease;
  outline: 1px solid transparent;
  outline-offset: -1px;
}

.md3-input:hover {
  border-color: var(--md-sys-color-on-surface);
}

.md3-input:focus {
  outline: 1px solid var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.md3-input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}
</style>
