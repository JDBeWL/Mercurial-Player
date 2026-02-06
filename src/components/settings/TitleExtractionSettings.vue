<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.titleExtraction') }}</h3>
    </div>
    
    <div class="settings-section">
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.preferMetadata') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.titleExtraction.preferMetadata }" @click="toggleSetting('preferMetadata')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.hideFileExtension') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.titleExtraction.hideFileExtension }" @click="toggleSetting('hideFileExtension')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.parseArtistTitle') }}</span>
        </div>
        <div class="switch" :class="{ active: configStore.titleExtraction.parseArtistTitle }" @click="toggleSetting('parseArtistTitle')">
          <div class="switch-track"></div>
          <div class="switch-handle"></div>
        </div>
      </div>
      
      <div class="setting-item select">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.separator') }}</span>
        </div>
        <MD3Select
          v-model="configStore.titleExtraction.separator"
          :options="separatorOptions"
          @change="saveConfig"
        />
      </div>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import logger from '../../utils/logger'
import MD3Select from '../MD3Select.vue'

const configStore = useConfigStore()

const separatorOptions = computed(() => 
  configStore.validSeparators.map(sep => ({ value: sep, label: sep }))
)

const saveConfig = async () => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (key) => {
  configStore.titleExtraction[key] = !configStore.titleExtraction[key]
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
