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

<script setup lang="ts">
import { ref, computed, defineAsyncComponent } from 'vue'
import { useConfigStore } from '../stores/config'
import { pluginManager } from '../plugins'
import { SettingsNav } from './settings'
import type { SettingsTab } from '@/types'

// 设置子组件懒加载:
// 只有用户切换到对应 tab 时才加载该组件的代码 chunk,
// 避免所有 11 个设置子组件的代码都打包进主 bundle。
// SettingsNav 是导航栏,始终需要,保持静态导入。
const FolderSettings = defineAsyncComponent(() => import('./settings/FolderSettings.vue'))
const GeneralSettings = defineAsyncComponent(() => import('./settings/GeneralSettings.vue'))
const LyricsSettings = defineAsyncComponent(() => import('./settings/LyricsSettings.vue'))
const TitleExtractionSettings = defineAsyncComponent(() => import('./settings/TitleExtractionSettings.vue'))
const PlaylistSettings = defineAsyncComponent(() => import('./settings/PlaylistSettings.vue'))
const AudioDeviceSettings = defineAsyncComponent(() => import('./settings/AudioDeviceSettings.vue'))
const EqualizerSettings = defineAsyncComponent(() => import('./settings/EqualizerSettings.vue'))
const PlayStatsSettings = defineAsyncComponent(() => import('./settings/PlayStatsSettings.vue'))
const PluginSettings = defineAsyncComponent(() => import('./settings/PluginSettings.vue'))
const AboutSettings = defineAsyncComponent(() => import('./settings/AboutSettings.vue'))

const configStore = useConfigStore()
const activeTab = ref<string>('folders')

const baseTabs: SettingsTab[] = [
  { id: 'folders', icon: 'folder', label: 'config.musicFolders' },
  { id: 'general', icon: 'settings', label: 'config.generalSettings' },
  { id: 'lyrics', icon: 'lyrics', label: 'config.lyricsSettings' },
  { id: 'titleExtraction', icon: 'title', label: 'config.titleExtraction' },
  { id: 'playlist', icon: 'queue_music', label: 'config.playlistSettings' },
  { id: 'audioDevice', icon: 'speaker', label: 'config.audioDeviceSettings' },
  { id: 'equalizer', icon: 'graphic_eq', label: 'config.equalizer' },
]

// 动态计算可见的 tabs
const visibleTabs = computed<SettingsTab[]>(() => {
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
  padding-left: 6vw;
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
