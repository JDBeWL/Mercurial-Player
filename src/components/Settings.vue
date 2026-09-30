<template>
  <div
    class="settings-panel"
    :data-mobile="isMobilePortrait ? 'true' : undefined"
    :data-view="mobileView"
  >
    <!-- 手机竖屏：面板唯一的头部行，列表级与详情级共用（此时顶栏整行隐藏，见 AppHeader）。
         必须排在导航和内容之前 —— 竖屏下面板是纵向 flex，顺序即上下关系。
         返回键在详情级是"回列表"、在列表级是"退出设置"，都由 requestBack 按历史层级分派。
         明暗切换与主题色本来是顶栏（竖屏收进 ⋮ 溢出菜单）的功能，顶栏让位后在这一行补回入口。 -->
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
const themeStore = useThemeStore()
const { developerMode } = useDeveloperMode()

const { isAndroid } = usePlatform()
const { isPortrait } = useOrientation()

/**
 * 手机竖屏：面板拆成「列表 → 详情」两级。480px 宽放不下"左栏 280px + 右内容"的并排布局，
 * 而窄屏降级出来的一排横向 tab 又因 .nav-item 没重置 width:100% 而失效（见 SettingsNav）。
 * 桌面端与横屏不走这里，behavior 不变。
 */
const isMobilePortrait = computed<boolean>(() => isAndroid.value && isPortrait.value)
// 竖屏列表页不预选任何一项：一进设置就看到第一项带选中底色，会让人误以为
// 已经停在某一页里。桌面端是并排布局，必须有个默认页，所以仍从 folders 开始。
const activeTab = ref<string>(isMobilePortrait.value ? '' : 'folders')
const mobileView = ref<'list' | 'detail'>('list')

/**
 * 系统返回键做成"应用内后退一级"。
 *
 * MainActivity 打开了 wry 的 handleBackNavigation，返回键会先走 webview.goBack()，
 * 于是每次返回落进这里的 popstate：详情页 → 列表页 → 关闭设置 → 再返回才退出应用。
 * 我们按层级压入等量的 history 条目（heldEntries 记账），页面自己关不掉时
 * （比如从顶栏按钮关）在卸载时把多余的条目消费掉，避免留下"按了没反应"的返回。
 */
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

/** 页内返回按钮：能交给历史就交给历史，让 popstate 成为唯一改状态的地方 */
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

// 用 select 事件而不是 watch(activeTab)：点"当前已选中"的那一项时值没变化，
// watch 不会触发，用户就卡在列表页进不去详情
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

/** 头部行只有这一处文案来源：列表级报面板名，详情级报当前页名 */
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

/* 竖屏头部行：只在手机竖屏渲染（模板里 v-if），其余情况不占位 */
.mobile-app-bar {
  display: none;
}

/* ===== 手机竖屏：列表 ↔ 详情 两级 =====
   来由见 isMobilePortrait 的注释，导航的列表形态见 SettingsNav。
   用 [data-mobile='true'] 守卫：@media (orientation: portrait) 在桌面把窗口
   拉成窄高时同样会命中，不加守卫会把桌面端的并排布局一起改掉。 */
@media (orientation: portrait) {
  .settings-panel[data-mobile='true'] {
    flex-direction: column;
    /* 列表页由导航自己撑满、详情页由内容区撑满，左右留白交给各自的容器 */
    padding: 0;
  }

  /* 列表页：头部行 + 整屏入口列表；详情页：头部行 + 内容。
     .settings-nav 是子组件的根元素，用 :deep 保证选择器一定命中
     （不依赖"子组件根元素继承父 scope id"这一行为） */
  .settings-panel[data-mobile='true'][data-view='list'] .settings-content,
  .settings-panel[data-mobile='true'][data-view='detail'] :deep(.settings-nav) {
    display: none;
  }

  .settings-panel[data-mobile='true'] .settings-content {
    padding: 16px 16px calc(16px + env(safe-area-inset-bottom, 0px));
  }

  /* 头部行已经带了页名，各面板自己的 h3 于是成了第二个标题，竖屏隐藏它。
     只剩标题的页整条收掉 —— 否则那里会留下一条 24px 的空白带（用户说的"没按钮也占着位置"）；
     还有动作按钮的页把按钮推到右缘，与头部行的动作区同一条边线。 */
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

  /* ===== 头部行 = M3 小号顶部应用栏 =====
     栅格：图标一律落在 16px（返回箭头在 48px 触摸盒里居中，左外边距 4px），
     文字一律落在 56px（与 SettingsNav 竖屏列表的标签同一条竖线）。
     它在滚动容器之外，天然固定不需要 sticky；也不铺不透明底色 ——
     本项目 surface-container-* 全族缺失，铺了也是透明，滚动时内容会从下面透出来。 */
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

  /* 明暗切换与主题色原来是顶栏（竖屏收进 ⋮）的功能，顶栏让位后由这一行接手，
     触摸目标从全局的 40px 提到 44px */
  .mobile-app-bar-actions .icon-button {
    width: 44px;
    height: 44px;
  }

  /* ThemeSelector 在竖屏会藏起自己的触发按钮、把入口让给顶栏溢出菜单（见其样式）。
     这里没有那条溢出菜单，按钮要显式放回来；:deep 带上本组件的 scope 属性，
     选择器权重高于它自己那条，不依赖样式表的先后顺序。 */
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
