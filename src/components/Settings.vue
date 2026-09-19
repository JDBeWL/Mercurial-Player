<template>
  <div class="settings-panel">
    <SettingsNav v-model="activeTab" :tabs="visibleTabs" @close="configStore.closeConfigPanel" />

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
      <DeveloperSettings v-if="activeTab === 'developer'" />
      <AboutSettings v-if="activeTab === 'about'" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, defineAsyncComponent } from 'vue'
import { useConfigStore } from '../stores/config'
import { pluginManager } from '../plugins'
import { useDeveloperMode } from '../composables/useDeveloperMode'
import { SettingsNav } from './settings'
import type { SettingsTab } from '@/types'

// 设置子组件懒加载:
// 只有用户切换到对应 tab 时才加载该组件的代码 chunk,
// 避免所有 11 个设置子组件的代码都打包进主 bundle。
// SettingsNav 是导航栏,始终需要,保持静态导入。
const FolderSettings = defineAsyncComponent(() => import('./settings/FolderSettings.vue'))
const GeneralSettings = defineAsyncComponent(() => import('./settings/GeneralSettings.vue'))
const LyricsSettings = defineAsyncComponent(() => import('./settings/LyricsSettings.vue'))
const TitleExtractionSettings = defineAsyncComponent(
  () => import('./settings/TitleExtractionSettings.vue'),
)
const PlaylistSettings = defineAsyncComponent(() => import('./settings/PlaylistSettings.vue'))
const AudioDeviceSettings = defineAsyncComponent(() => import('./settings/AudioDeviceSettings.vue'))
const EqualizerSettings = defineAsyncComponent(() => import('./settings/EqualizerSettings.vue'))
const PlayStatsSettings = defineAsyncComponent(() => import('./settings/PlayStatsSettings.vue'))
const PluginSettings = defineAsyncComponent(() => import('./settings/PluginSettings.vue'))
const AboutSettings = defineAsyncComponent(() => import('./settings/AboutSettings.vue'))
const DeveloperSettings = defineAsyncComponent(() => import('./settings/DeveloperSettings.vue'))

const configStore = useConfigStore()
const { developerMode } = useDeveloperMode()
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

  // 开发者页面(仅开发者模式开启时显示)
  if (developerMode.value) {
    tabs.push({ id: 'developer', icon: 'science', label: 'config.developerOptions' })
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
  /* 左侧 6vw 对齐播放器主界面的左边距。
     右侧刻意不在面板上留白：右侧留白改由 .settings-content 的 padding-right 承担，
     这样 .settings-content 本身能一直延伸到窗口右缘，它的滚动条才贴得住右边 */
  padding-left: 6vw;
}

.settings-content {
  flex: 1;
  overflow-y: auto;
  /* 左 32px 与导航栏拉开距离；右侧 = 32px + 5vw。
     滚动容器的 padding 渲染在滚动条内侧，滚动条落在窗口最右缘，
     5vw 加上 8px 滚动条后视觉上与左侧的 6vw 基本齐平 */
  padding: 24px calc(32px + 5vw) 24px 32px;
}

/* 各设置页的根容器不做宽度限制，铺满右侧可用宽度：
   设置项靠 justify-content: space-between 把控件推到右边缘，
   窗口变宽时刻度和控件之间的间距自然拉伸。 */
.settings-content :deep(.tab-content),
.settings-content :deep(.audio-device-settings),
.settings-content :deep(.equalizer-settings) {
  width: 100%;
}

@media (max-width: 768px) {
  .settings-panel {
    flex-direction: column;
    /* 纵向堆叠时导航栏横跨整行，右侧留白要回到面板上，
       否则导航栏会顶到窗口右缘；内容区的右内边距随之恢复常规值 */
    padding-right: 6vw;
  }

  .settings-content {
    padding: 16px;
  }
}
</style>
