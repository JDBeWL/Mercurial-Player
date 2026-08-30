<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.playlistSettings') }}</h3>
    </div>

    <div class="settings-section">
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.generateAllSongsPlaylist') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.playlist.generateAllSongsPlaylist"
          @update:model-value="toggleSetting('generateAllSongsPlaylist')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.folderBasedPlaylists') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.playlist.folderBasedPlaylists"
          @update:model-value="toggleSetting('folderBasedPlaylists')"
        />
      </div>

      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.sortOrder') }}</span>
        </div>
        <MD3Select
          v-model="configStore.playlist.sortOrder"
          :options="sortOrderOptions"
          @change="saveConfig"
        />
      </div>

      <div class="setting-item input">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.playlistNameFormat') }}</span>
        </div>
        <input
          v-model="configStore.playlist.playlistNameFormat"
          type="text"
          placeholder="{folderName}"
          class="md3-input"
          @change="saveConfig"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfigStore } from '../../stores/config'
import SettingSwitch from './SettingSwitch.vue'
import logger from '../../utils/logger'
import MD3Select from '../MD3Select.vue'

const { t } = useI18n()
const configStore = useConfigStore()

const sortOrderOptions = computed(() => [
  { value: 'asc', label: t('config.sortAsc') },
  { value: 'desc', label: t('config.sortDesc') },
])

const saveConfig = async (): Promise<void> => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (
  key: 'generateAllSongsPlaylist' | 'folderBasedPlaylists',
): Promise<void> => {
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

.md3-input {
  min-width: 200px;
  padding: 12px 16px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-surface-container-low);
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  font-weight: 400;
  font-family: 'Roboto', 'Roboto Fallback', sans-serif;
  min-height: 48px;
  box-sizing: border-box;
  transition: all 0.2s cubic-bezier(0.2, 0, 0, 1);
  outline: none;
  box-shadow: none;
}

.md3-input:hover {
  border-color: var(--md-sys-color-on-surface);
  background-color: var(--md-sys-color-surface-container);
}

.md3-input:focus {
  border-color: var(--md-sys-color-primary);
  background-color: var(--md-sys-color-surface-container);
  box-shadow: 0 0 0 1px var(--md-sys-color-primary);
}

.md3-input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}
</style>
