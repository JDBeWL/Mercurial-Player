<template>
  <div class="settings-panel">
    <SettingsNav 
      v-model="activeTab" 
      :tabs="visibleTabs" 
      @close="configStore.closeConfigPanel" 
    />

    <div class="settings-content">
      <FolderSettings v-if="activeTab === 'folders'" />
      <GeneralSettings v-if="activeTab === 'general'" />
      <LyricsSettings v-if="activeTab === 'lyrics'" />
      <TitleExtractionSettings v-if="activeTab === 'titleExtraction'" />
      <PlaylistSettings v-if="activeTab === 'playlist'" />
      <AudioDeviceSettings v-if="activeTab === 'audioDevice'" />
      <EqualizerSettings v-if="activeTab === 'equalizer'" />
      <PlayStatsSettings v-if="activeTab === 'playStats'" />
      <PluginSettings v-if="activeTab === 'plugins'" />
      <AboutSettings v-if="activeTab === 'about'" />
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'
import { useConfigStore } from '../stores/config'
import { pluginManager } from '../plugins'
import {
  SettingsNav,
  FolderSettings,
  GeneralSettings,
  TitleExtractionSettings,
  LyricsSettings,
  PlaylistSettings,
  AboutSettings
} from './settings'
import AudioDeviceSettings from './AudioDeviceSettings.vue'
import EqualizerSettings from './EqualizerSettings.vue'
import PlayStatsSettings from './settings/PlayStatsSettings.vue'
import PluginSettings from './settings/PluginSettings.vue'

const configStore = useConfigStore()
const activeTab = ref('folders')

const baseTabs = [
  { id: 'folders', icon: 'folder', label: 'config.musicFolders' },
  { id: 'general', icon: 'settings', label: 'config.generalSettings' },
  { id: 'lyrics', icon: 'lyrics', label: 'config.lyricsSettings' },
  { id: 'titleExtraction', icon: 'title', label: 'config.titleExtraction' },
  { id: 'playlist', icon: 'queue_music', label: 'config.playlistSettings' },
  { id: 'audioDevice', icon: 'speaker', label: 'config.audioDeviceSettings' },
  { id: 'equalizer', icon: 'equalizer', label: 'config.equalizer' },
]

// 动态计算可见的 tabs
const visibleTabs = computed(() => {
  const tabs = [...baseTabs]
  
  // 插件页面始终显示
  tabs.push({ id: 'plugins', icon: 'extension', label: 'config.plugins' })
  
  // 如果播放统计插件已激活，显示播放统计页面（放在插件下面）
  const playCountPlugin = pluginManager.plugins.get('builtin-play-count')
  if (playCountPlugin?.state === 'active') {
    tabs.push({ id: 'playStats', icon: 'bar_chart', label: 'config.playStats' })
  }
  
  // 关于页面始终显示在最后
  tabs.push({ id: 'about', icon: 'info', label: 'config.about' })
  
  return tabs
})
</script>

<style scoped>
.settings-panel {
  flex: 1;
  display: flex;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-container-low);
}

.settings-content {
  flex: 1;
  overflow-y: auto;
  padding: 24px 32px;
}

@media (max-width: 768px) {
  .settings-panel {
    flex-direction: column;
  }
  
  .settings-content {
    padding: 16px;
  }
}
</style>
