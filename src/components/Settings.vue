<template>
  <div
    class="settings-panel"
    :data-mobile="isMobilePortrait ? 'true' : undefined"
    :data-view="mobileView"
  >
    <!-- 竖屏唯一的头部行, 列表级与详情级共用 (此时顶栏整行隐藏, 见 AppHeader); 纵向 flex 里必须排在导航和内容之前。
         返回键由 requestBack 按层级分派: 详情回列表, 列表退出 -->
    <div v-if="isMobilePortrait" class="mobile-app-bar">
      <button class="mobile-back-btn" :title="$t('common.back')" @click="requestBack">
        <span class="material-symbols-rounded">arrow_back</span>
      </button>
      <h2 class="mobile-app-bar-title">{{ $t(barTitleKey) }}</h2>
      <div class="mobile-app-bar-actions">
        <button
          class="icon-button"
          :title="themeStore.isDarkMode ? $t('nav.theme.light') : $t('nav.theme.dark')"
          @click="themeStore.toggleDarkMode"
        >
          <span class="material-symbols-rounded">{{
            themeStore.isDarkMode ? 'light_mode' : 'dark_mode'
          }}</span>
        </button>
        <ThemeSelector />
      </div>
    </div>

    <SettingsNav
      v-model="activeTab"
      :tabs="visibleTabs"
      @close="requestBack"
      @select="onTabSelect"
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
      <DeveloperSettings v-if="activeTab === 'developer'" />
      <AboutSettings v-if="activeTab === 'about'" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, defineAsyncComponent, onMounted, onUnmounted } from 'vue'
import { useConfigStore } from '../stores/config'
import { useThemeStore } from '../stores/theme'
import { usePlatform } from '../composables/usePlatform'
import { useOrientation } from '../composables/useOrientation'
import { pluginManager } from '../plugins'
import { useDeveloperMode } from '../composables/useDeveloperMode'
import { SettingsNav } from './settings'
import ThemeSelector from './ThemeSelector.vue'
import type { SettingsTab } from '@/types'

// 设置子组件懒加载: 切到对应 tab 才拉 chunk, 11 个子组件不进主 bundle; SettingsNav 始终静态导入
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
const themeStore = useThemeStore()
const { developerMode } = useDeveloperMode()

const { isAndroid } = usePlatform()
const { isPortrait } = useOrientation()

/** 手机竖屏把面板拆成列表 -> 详情两级: 480px 宽放不下左栏 280px + 右内容的并排布局,
 *  窄屏降级出的横向 tab 又因 .nav-item 没重置 width:100% 而失效 (见 SettingsNav)
 *  桌面端与横屏不走这里, behavior 不变 */
const isMobilePortrait = computed<boolean>(() => isAndroid.value && isPortrait.value)
// 竖屏列表页不预选任何一项: 一进设置就看到选中底色会误导; 桌面并排布局需要默认页, 仍从 folders 开始
const activeTab = ref<string>(isMobilePortrait.value ? '' : 'folders')
const mobileView = ref<'list' | 'detail'>('list')

/** 系统返回键做成应用内后退一级: MainActivity 开了 handleBackNavigation, 返回键先走
 *  webview.goBack(), 逐次 popstate 到关闭设置才退出应用; heldEntries 记账层级,
 *  页面自行关闭时在卸载里消费掉多余条目 */
let heldEntries = 0
const pushEntry = (): void => {
  history.pushState({ settingsPanel: true }, '')
  heldEntries += 1
}

const backOneLevel = (): void => {
  if (mobileView.value === 'detail') {
    mobileView.value = 'list'
    return
  }
  configStore.closeConfigPanel()
}

const onPopState = (): void => {
  if (heldEntries === 0) return
  heldEntries -= 1
  backOneLevel()
}

/** 页内返回键: 能交给历史就交给历史, 让 popstate 成为唯一改状态的地方 (层级分派见 backOneLevel) */
const requestBack = (): void => {
  if (heldEntries > 0) {
    history.back()
    return
  }
  configStore.closeConfigPanel()
}

onMounted(() => {
  window.addEventListener('popstate', onPopState)
  pushEntry()
})

onUnmounted(() => {
  window.removeEventListener('popstate', onPopState)
  if (heldEntries > 0) history.go(-heldEntries)
})

// 用 select 事件而不是 watch(activeTab): 点已选中项时值没变, watch 不触发, 会卡在列表页进不去详情
const onTabSelect = (): void => {
  mobileView.value = 'detail'
  if (isMobilePortrait.value) pushEntry()
}

const baseTabs: SettingsTab[] = [
  { id: 'folders', icon: 'folder', label: 'config.musicFolders' },
  { id: 'general', icon: 'settings', label: 'config.generalSettings' },
  { id: 'lyrics', icon: 'lyrics', label: 'config.lyricsSettings' },
  { id: 'titleExtraction', icon: 'title', label: 'config.titleExtraction' },
  { id: 'playlist', icon: 'queue_music', label: 'config.playlistSettings' },
  { id: 'audioDevice', icon: 'speaker', label: 'config.audioDeviceSettings' },
  { id: 'equalizer', icon: 'graphic_eq', label: 'config.equalizer' },
]

const visibleTabs = computed<SettingsTab[]>(() => {
  const tabs = [...baseTabs]

  tabs.push({ id: 'plugins', icon: 'extension', label: 'config.plugins' })

  // 播放统计要等 builtin-play-count 激活才出现, 插在 plugins 之后
  const playCountPlugin = pluginManager.plugins.get('builtin-play-count')
  if (playCountPlugin?.state === 'active') {
    tabs.push({ id: 'playStats', icon: 'bar_chart', label: 'config.playStats' })
  }

  // 开发者页仅在开发者模式开启时出现
  if (developerMode.value) {
    tabs.push({ id: 'developer', icon: 'science', label: 'config.developerOptions' })
  }

  tabs.push({ id: 'about', icon: 'info', label: 'config.about' })

  return tabs
})

/** 竖屏详情页标题复用导航项的 i18n key, 避免两处文案漂移 */
const currentTabLabel = computed<string>(
  () => visibleTabs.value.find((tab) => tab.id === activeTab.value)?.label ?? '',
)

/** 头部行只有这一处文案来源: 列表级报面板名, 详情级报当前页名 */
const barTitleKey = computed<string>(() =>
  mobileView.value === 'list' ? 'config.title' : currentTabLabel.value,
)
</script>

<style scoped>
.settings-panel {
  flex: 1;
  display: flex;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-container-low);
  /* 左 6vw 对齐播放器主界面左边距; 右侧刻意不在面板留白, 改由 .settings-content 的 padding-right
     承担, 内容区延伸到窗口右缘, 滚动条才贴得住右边 */
  padding-left: 6vw;
}

.settings-content {
  flex: 1;
  overflow-y: auto;
  /* 左 32px 与导航栏拉开距离; 右侧 = 32px + 5vw。滚动容器的 padding 渲染在滚动条内侧,
     滚动条落在窗口最右缘, 5vw 加 8px 滚动条后视觉上与左侧 6vw 齐平 */
  padding: 24px calc(32px + 5vw) 24px 32px;
}

/* 各设置页根容器不限宽, 铺满右侧可用宽度: 控件靠 space-between 推到右缘, 窗口变宽时自然拉开 */
.settings-content :deep(.tab-content),
.settings-content :deep(.audio-device-settings),
.settings-content :deep(.equalizer-settings) {
  width: 100%;
}

@media (max-width: 768px) {
  .settings-panel {
    flex-direction: column;
    /* 纵向堆叠时导航横跨整行, 右留白要回到面板上, 否则导航顶到窗口右缘 */
    padding-right: 6vw;
  }

  .settings-content {
    padding: 16px;
  }
}

/* 竖屏头部行: 只在手机竖屏渲染 (模板里 v-if), 其余情况不占位 */
.mobile-app-bar {
  display: none;
}

/* 两级布局来由见 isMobilePortrait 与 SettingsNav 的注释。用 [data-mobile='true'] 守卫:
   orientation: portrait 在桌面窄高窗口同样命中, 不加守卫会把并排布局一起改掉 */
@media (orientation: portrait) {
  .settings-panel[data-mobile='true'] {
    flex-direction: column;
    /* 列表页由导航撑满, 详情页由内容区撑满, 左右留白交给各自的容器 */
    padding: 0;
  }

  /* 列表页留头部行 + 整屏入口列表, 详情页留头部行 + 内容。.settings-nav 是子组件根元素,
     用 :deep 保证命中, 不依赖子组件根元素继承父 scope id */
  .settings-panel[data-mobile='true'][data-view='list'] .settings-content,
  .settings-panel[data-mobile='true'][data-view='detail'] :deep(.settings-nav) {
    display: none;
  }

  .settings-panel[data-mobile='true'] .settings-content {
    padding: 16px 16px calc(16px + env(safe-area-inset-bottom, 0px));
  }

  /* 头部行已带页名, 各面板自己的 h3 成了第二个标题, 竖屏隐藏它; 只剩标题的页整条收掉,
     否则留下一条 24px 空白带; 还有动作按钮的页把按钮推到右缘, 与头部行动作区同一条边线 */
  .settings-panel[data-mobile='true'][data-view='detail'] :deep(.content-header h3) {
    display: none;
  }

  .settings-panel[data-mobile='true'][data-view='detail'] :deep(.content-header) {
    justify-content: flex-end;
  }

  .settings-panel[data-mobile='true'][data-view='detail']
    :deep(.content-header:has(> h3:only-child)) {
    display: none;
  }

  /* 头部行 = M3 小号顶部应用栏。栅格: 图标落在 16px (返回箭头在 48px 触摸盒居中, 左外边距 4px),
     文字落在 56px (与 SettingsNav 竖屏列表标签同一条竖线)。它在滚动容器之外, 天然固定不用 sticky;
     也不铺不透明底色: 本项目没有 surface-container-* 全族, 铺了仍是透明, 内容会从下面透出来 */
  .mobile-app-bar {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
    min-height: 64px;
    padding: 4px 4px 4px 0;
  }

  .mobile-app-bar-title {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 22px;
    font-weight: 500;
    color: var(--md-sys-color-on-surface);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .mobile-app-bar-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
  }

  /* 明暗切换与主题色原是顶栏功能, 顶栏让位后由这一行接手; 触摸目标从 40px 提到 44px */
  .mobile-app-bar-actions .icon-button {
    width: 44px;
    height: 44px;
  }

  /* ThemeSelector 竖屏会藏起触发按钮、把入口让给顶栏溢出菜单 (见其样式), 这里没有那条菜单,
     按钮要显式放回来; :deep 带上本组件 scope 属性, 权重高于它自己那条, 不依赖样式表顺序 */
  .mobile-app-bar-actions :deep(.theme-selector[data-mobile='true'] > .icon-button) {
    display: flex;
  }

  .mobile-back-btn {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 48px;
    margin-left: 4px;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--md-sys-color-on-surface-variant);
    cursor: pointer;
  }
}
</style>
