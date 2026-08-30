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
        <SettingSwitch
          :model-value="configStore.titleExtraction.preferMetadata"
          @update:model-value="toggleSetting('preferMetadata')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.hideFileExtension') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.titleExtraction.hideFileExtension"
          @update:model-value="toggleSetting('hideFileExtension')"
        />
      </div>

      <div class="setting-item">
        <div class="setting-info">
          <span class="setting-label">{{ $t('config.parseArtistTitle') }}</span>
        </div>
        <SettingSwitch
          :model-value="configStore.titleExtraction.parseArtistTitle"
          @update:model-value="toggleSetting('parseArtistTitle')"
        />
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

<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import { useTrackInfo } from '../../composables/useTrackInfo'
import logger from '../../utils/logger'
import MD3Select from '../MD3Select.vue'
import SettingSwitch from './SettingSwitch.vue'

const configStore = useConfigStore()
const { clearAllCache } = useTrackInfo()

const separatorOptions = computed(() =>
  configStore.validSeparators.map((sep) => ({ value: sep, label: sep })),
)

const saveConfig = async (): Promise<void> => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}

const toggleSetting = async (
  key: 'preferMetadata' | 'hideFileExtension' | 'parseArtistTitle',
): Promise<void> => {
  configStore.titleExtraction[key] = !configStore.titleExtraction[key]
  // 标题提取设置变化后清除 useTrackInfo 缓存,让当前播放歌曲重新处理
  clearAllCache()
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
</style>
