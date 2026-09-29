<template>
  <div
    class="settings-panel"
    :data-mobile="isMobilePortrait ? 'true' : undefined"
    :data-view="mobileView"
  >
    <SettingsNav
      v-model="activeTab"
      :tabs="visibleTabs"
      @close="configStore.closeConfigPanel"
      @select="onTabSelect"
    />

    <!-- 手机竖屏：详情页头（返回 + 当前页名）。放在滚动容器外面，这样它天然不随内容滚动，既不需要
         sticky，也不需要给自己铺一层不透明底色（本项目 surface-container-* 全族缺失，铺了也是透明，
         滚动时内容会从下面透出来）。桌面/横屏不渲染这块（v-if），并排的导航栏本身就是返回路径。 -->
    <div v-if="isMobilePortrait" class="mobile-detail-header">
      <button class="mobile-back-btn" :title="$t('common.back')" @click="backToList">
        <span class="material-symbols-rounded">arrow_back</span>
      </button>
      <h2 class="mobile-detail-title">{{ $t(currentTabLabel) }}</h2>
    </div>

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
import { ref, computed, defineAsyncComponent, watch } from 'vue'
import { useConfigStore } from '../stores/config'
import { usePlatform } from '../composables/usePlatform'
import { useOrientation } from '../composables/useOrientation'
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

const { isAndroid } = usePlatform()
const { isPortrait } = useOrientation()

/**
 * 手机竖屏：面板拆成「列表 → 详情」两级。
 * 480px 宽放不下"左栏 280px + 右内容"的并排布局，而原来的窄屏降级（一排横向
 * tab）又因为 .nav-item 没重置 width:100% 而彻底失效——10 个 tab 各撑满整行，
 * 用户只能看到第一个（详见 SettingsNav 的样式注释）。
 * 桌面端与横屏不走这里，behavior 不变。
 */
const isMobilePortrait = computed<boolean>(() => isAndroid.value && isPortrait.value)
const mobileView = ref<'list' | 'detail'>('list')

// 用 select 事件而不是 watch(activeTab)：点"当前已选中"的那一项时值没变化，
// watch 不会触发，用户就卡在列表页进不去详情
const onTabSelect = (): void => {
  mobileView.value = 'detail'
}

const backToList = (): void => {
  mobileView.value = 'list'
}

// 每次重新打开设置都从列表开始，而不是停在上次看的详情页
watch(
  () => configStore.ui.showConfigPanel,
  (open) => {
    if (open) mobileView.value = 'list'
  },
)

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

/** 竖屏详情页的标题 —— 直接复用导航项的 i18n key，避免两处文案漂移 */
const currentTabLabel = computed<string>(
  () => visibleTabs.value.find((tab) => tab.id === activeTab.value)?.label ?? '',
)
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

/* 竖屏详情页头：只在手机竖屏渲染（模板里 v-if），其余情况不占位 */
.mobile-detail-header {
  display: none;
}

/* ===== 手机竖屏：列表 ↔ 详情 两级 =====
   上面那条 768px 规则把面板改成纵向堆叠（导航横跨整行），在 480px 宽的竖屏下
   仍然不可用：横跨整行的是一排横向 tab，而 .nav-item 没重置 width:100%，
   10 个 tab 各撑满整行 → 排成 10 屏宽，用户只看得到一个空胶囊（实测截图）。
   这里换成手机上标准的「整屏列表 → 详情」，导航的列表形态见 SettingsNav。

   用 [data-mobile='true'] 守卫：@media (orientation: portrait) 在桌面把窗口
   拉成窄高时同样会命中，不加守卫会把桌面端的并排布局一起改掉。 */
@media (orientation: portrait) {
  .settings-panel[data-mobile='true'] {
    flex-direction: column;
    /* 列表页由导航自己撑满、详情页由内容区撑满，左右留白交给各自的容器 */
    padding: 0;
  }

  /* 列表页：只留整屏的入口列表；详情页：只留头部 + 内容。
     .settings-nav 是子组件的根元素，用 :deep 保证选择器一定命中
     （不依赖"子组件根元素继承父 scope id"这一行为） */
  .settings-panel[data-mobile='true'][data-view='list'] .settings-content,
  .settings-panel[data-mobile='true'][data-view='list'] .mobile-detail-header,
  .settings-panel[data-mobile='true'][data-view='detail'] :deep(.settings-nav) {
    display: none;
  }

  .settings-panel[data-mobile='true'] .settings-content {
    padding: 16px 16px calc(16px + env(safe-area-inset-bottom, 0px));
  }

  /* 详情页头靠 flex 天然固定在顶部（它在滚动容器之外），不需要 sticky */
  .mobile-detail-header {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
    padding: 4px 16px 8px;
  }

  .mobile-detail-title {
    margin: 0;
    font-size: 20px;
    font-weight: 500;
    color: var(--md-sys-color-on-surface);
  }

  /* 负左边距让图标视觉上与下方内容左对齐，同时保住 44px 的触摸目标 */
  .mobile-back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    margin-left: -10px;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--md-sys-color-on-surface-variant);
    cursor: pointer;
  }
}
</style>
