<template>
  <!-- Android 没有拖拽区实现（系统标题栏/手势返回才是移动端范式）。本窗口无边框，顶栏是移动窗口的唯一手段，
       所以整条用 "deep"：空值/"true" 会在遍历中提前 return、短路掉外层 header 的 "deep"，.nav-center 也必须 "deep" -->
  <header
    class="nav-bar"
    :data-mobile="isAndroid ? 'true' : undefined"
    :data-settings-open="settingsOpen ? 'true' : undefined"
    :data-tauri-drag-region="isAndroid ? null : 'deep'"
  >
    <div class="nav-left">
      <!-- 图标按平台分开：menu 的语义是"展开侧边栏抽屉"，手机上指代"音乐库"太含糊，改用 library_music；
           桌面端保留 menu，那里汉堡就是标准的侧栏开关 -->
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="$t('nav.library')"
        @click="emit('toggle-library')"
      >
        <span class="material-symbols-rounded">{{ isAndroid ? 'library_music' : 'menu' }}</span>
      </button>
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="$t('nav.settings')"
        @click="configStore.toggleConfigPanel()"
      >
        <span class="material-symbols-rounded">settings</span>
      </button>
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="themeStore.isDarkMode ? $t('nav.theme.light') : $t('nav.theme.dark')"
        @click="themeStore.toggleDarkMode"
      >
        <span class="material-symbols-rounded">{{
          themeStore.isDarkMode ? 'light_mode' : 'dark_mode'
        }}</span>
      </button>
      <ThemeSelector ref="themeSelectorRef" data-tauri-drag-region="false" />
    </div>
    <div class="nav-center" data-tauri-drag-region="deep">
      <Transition name="fade" mode="out-in">
        <div :key="currentTrack ? currentTrack.path : 'no-track-nav'" class="nav-track-info">
          <template v-if="currentTrack">
            <div
              class="nav-track-title"
              :title="getTrackTitle(currentTrack) || $t('player.noTrack')"
            >
              <span class="nav-track-title-text selectable" data-tauri-drag-region="false">
                {{ getTrackTitle(currentTrack) || $t('player.noTrack') }}
              </span>
              <span
                v-if="!fileExists"
                class="material-symbols-rounded nav-file-warning"
                :title="$t('player.fileNotFound')"
              >
                warning
              </span>
            </div>
            <div
              v-if="getTrackArtist(currentTrack)"
              class="nav-track-artist selectable"
              :title="getTrackArtist(currentTrack)"
              data-tauri-drag-region="false"
            >
              {{ getTrackArtist(currentTrack) }}
            </div>
          </template>
          <template v-else>
            <!-- 占位文案与真实艺术家同一套处理：可见的文字都不当拖拽区 -->
            <div class="nav-track-artist selectable" data-tauri-drag-region="false">
              {{ $t('player.noTrack') }}
            </div>
          </template>
        </div>
      </Transition>
    </div>
    <div v-if="!isAndroid" class="nav-right">
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="$t('window.miniMode')"
        @click="configStore.toggleMiniMode"
      >
        <span class="material-symbols-rounded">picture_in_picture_alt</span>
      </button>
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="$t('window.minimize')"
        @click="minimizeWindow"
      >
        <span class="material-symbols-rounded">minimize</span>
      </button>
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="isFullscreen ? $t('window.exitFullscreen') : $t('window.fullscreen')"
        @click="toggleFullscreen"
      >
        <span class="material-symbols-rounded">{{
          isFullscreen ? 'fullscreen_exit' : 'fullscreen'
        }}</span>
      </button>
      <button
        class="icon-button"
        data-tauri-drag-region="false"
        :title="$t('window.close')"
        @click="closeWindow"
      >
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>

    <!-- 手机竖屏：设置 / 明暗 / 主题色收进溢出菜单。窄顶栏放 4 个按钮时相邻间距只剩 8px（有误触
         风险）、曲名也被挤得看不全，而这三项都是装好就很少改的。横屏与桌面端不进这个分支。 -->
    <template v-if="showOverflowMenu">
      <button
        class="icon-button nav-overflow-btn"
        data-tauri-drag-region="false"
        :title="$t('nav.more')"
        aria-haspopup="menu"
        :aria-expanded="overflowOpen"
        @click.stop="overflowOpen = !overflowOpen"
      >
        <span class="material-symbols-rounded">more_vert</span>
      </button>
      <Transition name="menu-fade">
        <div
          v-if="overflowOpen"
          ref="overflowMenuRef"
          class="overflow-menu"
          role="menu"
          data-tauri-drag-region="false"
        >
          <button
            v-if="!settingsOpen"
            class="overflow-item"
            role="menuitem"
            @click="onOverflowSelect('settings')"
          >
            <span class="material-symbols-rounded">settings</span>
            <span class="overflow-item-label">{{ $t('nav.settings') }}</span>
          </button>
          <button class="overflow-item" role="menuitem" @click="onOverflowSelect('theme')">
            <span class="material-symbols-rounded">{{
              themeStore.isDarkMode ? 'light_mode' : 'dark_mode'
            }}</span>
            <span class="overflow-item-label">{{
              themeStore.isDarkMode ? $t('nav.theme.light') : $t('nav.theme.dark')
            }}</span>
          </button>
          <button class="overflow-item" role="menuitem" @click="onOverflowSelect('color')">
            <span class="material-symbols-rounded">palette</span>
            <span class="overflow-item-label">{{ $t('nav.themeColor') }}</span>
          </button>
        </div>
      </Transition>
    </template>
  </header>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useConfigStore } from '@/stores/config'
import { usePlayerStore } from '@/stores/player'
import { useThemeStore } from '@/stores/theme'
import { useTrackInfo } from '@/composables/useTrackInfo'
import { usePlatform } from '@/composables/usePlatform'
import { useOrientation } from '@/composables/useOrientation'
import ThemeSelector from './ThemeSelector.vue'

/** 顶栏导航(曲目信息 + 主题/窗口/库入口)。自身状态走 store，窗口控制函数与全屏状态由父级注入:
 *  App 的 useWindowControls 实例是唯一状态源，避免二次订阅 */
defineProps<{
  /** 全屏状态(决定全屏按钮标题与图标) */
  isFullscreen: boolean
  /** 当前曲目文件是否存在(文件缺失时展示警告图标) */
  fileExists: boolean
  minimizeWindow: () => void
  toggleFullscreen: () => void
  closeWindow: () => void
}>()

const emit = defineEmits<{
  'toggle-library': []
}>()

const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const configStore = useConfigStore()
const { currentTrack } = storeToRefs(playerStore)
const { getTrackTitle, getTrackArtist } = useTrackInfo()
// Android 上隐藏窗口控制按钮与拖拽区
const { isAndroid } = usePlatform()
const { isPortrait } = useOrientation()

/** 溢出菜单只在手机竖屏出现 */
const showOverflowMenu = computed<boolean>(() => isAndroid.value && isPortrait.value)

/** 设置面板打开时顶栏让出中间那块曲名 */
const settingsOpen = computed<boolean>(() => configStore.ui.showConfigPanel)

const overflowOpen = ref<boolean>(false)
const overflowMenuRef = ref<HTMLElement | null>(null)
/** 主题色面板由 ThemeSelector 持有，这里只负责把它的 open() 暴露给菜单项 */
const themeSelectorRef = ref<{ open: () => void } | null>(null)

const closeOverflow = (): void => {
  overflowOpen.value = false
}

type OverflowAction = 'settings' | 'theme' | 'color'

const onOverflowSelect = (action: OverflowAction): void => {
  closeOverflow()
  switch (action) {
    case 'settings':
      configStore.toggleConfigPanel()
      break
    case 'theme':
      void themeStore.toggleDarkMode()
      break
    case 'color':
      themeSelectorRef.value?.open()
      break
  }
}

// 点菜单外任意位置关闭。点触发按钮自身的冒泡由模板的 @click.stop 拦掉，否则"打开"会在同一次点击里被这里收回去
const handleDocumentClick = (event: MouseEvent): void => {
  if (!overflowOpen.value) return
  if (overflowMenuRef.value?.contains(event.target as Node)) return
  closeOverflow()
}

onMounted(() => document.addEventListener('click', handleDocumentClick))
onUnmounted(() => document.removeEventListener('click', handleDocumentClick))

// 转回横屏后菜单失去意义（按钮本身也被 v-if 掉了），顺手关闭
watch(showOverflowMenu, (visible) => {
  if (!visible) closeOverflow()
})
</script>

<style scoped>
/* 三列栅格把曲目信息钉在窗口正中（flex 下 Android 右侧按钮被 v-if 掉会让标题偏右） */
.nav-bar {
  display: grid;
  grid-template-columns:
    minmax(min-content, 1fr)
    minmax(0, auto)
    minmax(min-content, 1fr);
  align-items: center;
  column-gap: 8px;
}

.nav-left {
  justify-self: start;
}

.nav-right {
  justify-self: end;
}

/* 上下各 8px 内边距避免长曲名贴着按钮。box-sizing 必须 border-box：项目只在 #app 设了它，
   content-box 下 padding 会让整盒比栅格列宽出 16px、两侧各溢出 8px */
.nav-center {
  justify-self: center;
  box-sizing: border-box;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  padding: 0 8px;
}

/* 双行显示：第一行曲名 16px，第二行艺术家 14px */
.nav-track-info {
  display: flex;
  flex-direction: column;
  align-items: center;
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
}

.nav-track-title {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  max-width: 100%;
  font-size: 16px;
  font-weight: 500;
  line-height: 1.4;
  color: var(--md-sys-color-on-surface);
  white-space: nowrap;
}

.nav-track-title-text {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-track-artist {
  max-width: 100%;
  font-size: 14px;
  line-height: 1.3;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.nav-file-warning {
  flex-shrink: 0;
  font-size: 16px;
  color: var(--md-sys-color-error);
}

/* 窄到 640px 以下（手机竖屏）：左侧按钮群已占掉大半宽度，曲名塞进同一行只剩几十像素，才退化成两行
   （第一行按钮、第二行曲名）。栅格版的两行要用 grid-template-areas 表达，flex 的 order/flex-basis 在栅格里不生效 */
@media (max-width: 640px) {
  .nav-bar {
    grid-template-columns: minmax(min-content, 1fr) auto;
    grid-template-areas:
      'left right'
      'center center';
    height: auto;
    row-gap: 2px;
  }

  .nav-left {
    grid-area: left;
  }

  .nav-right {
    grid-area: right;
  }

  .nav-center {
    grid-area: center;
    padding: 0 8px;
  }

  /* 独占一行后宽度仍然是有限的，艺术家照旧收起，只留曲名 */
  .nav-track-artist {
    display: none;
  }
}

/* 手机竖屏：.nav-left 用 display: contents 让子项直接成为栅格项，低频操作收进溢出菜单。
   整块用 [data-mobile='true'] 守卫，(orientation: portrait) 在桌面把窗口拉成窄高时同样会命中。
   必须撤掉 640px 那条留下的两行 areas，否则曲名会被自动放置甩到第二行、顶栏高度翻倍 */
@media (orientation: portrait) {
  .nav-bar[data-mobile='true'] .nav-left {
    display: contents;
  }

  /* 单行三区：音乐库 | 曲目信息 | 溢出菜单 */
  .nav-bar[data-mobile='true'] {
    grid-template-columns: auto minmax(0, 1fr) auto;
    grid-template-rows: auto;
    grid-template-areas: none;
    column-gap: 4px;
    row-gap: 0;
    /* 溢出菜单的定位基准 */
    position: relative;
  }

  /* 1 = 音乐库 */
  .nav-bar[data-mobile='true'] .nav-left > :nth-child(1) {
    grid-area: 1 / 1;
  }

  /* 2 = 设置、3 = 明暗：手机上收进溢出菜单。第 4 个是 ThemeSelector，它自己在组件内部隐藏触发按钮：
     颜色面板还挂在那个节点上，整体 display:none 会把面板一起藏掉 */
  .nav-bar[data-mobile='true'] .nav-left > :nth-child(2),
  .nav-bar[data-mobile='true'] .nav-left > :nth-child(3) {
    display: none;
  }

  .nav-bar[data-mobile='true'] .nav-center {
    grid-area: 1 / 2;
    padding: 0 8px;
  }

  /* 溢出菜单按钮占第 3 列。.nav-right（窗口按钮）在 Android 上不渲染，不必参与栅格 */
  .nav-bar[data-mobile='true'] .nav-overflow-btn {
    grid-area: 1 / 3;
  }

  /* 覆盖上面 640px 那条：竖屏现在有位置了，艺术家要显示出来 */
  .nav-bar[data-mobile='true'] .nav-track-artist {
    display: block;
  }

  /* 设置面板打开时整行让位：Settings 的 mobile-app-bar 已收进返回、标题和这里的明暗/主题色入口，
     两行摞在一起会有两个返回键，还白吃 60px 高度；safe-area 的 padding 挂在 #app 上，不受影响 */
  .nav-bar[data-mobile='true'][data-settings-open='true'] {
    display: none;
  }
}

/* 溢出菜单（只有手机竖屏会渲染） */
.overflow-menu {
  position: absolute;
  top: calc(100% - 2px);
  right: 8px;
  z-index: 200;
  display: flex;
  flex-direction: column;
  min-width: 180px;
  padding: 6px;
  border: 1px solid var(--md-sys-color-outline-variant);
  border-radius: 16px;
  /* 不能用 surface-container-* 系列：本应用主题只输出 29 个基础角色，那一族全部不存在，
     声明会静默失效变透明（见 tests/utils/themeTokens.test.ts 的棘轮断言）。浮层用真实存在的 surface */
  background-color: var(--md-sys-color-surface);
  box-shadow: var(--md-sys-elevation-level3);
}

.overflow-item {
  display: flex;
  align-items: center;
  gap: 14px;
  width: 100%;
  min-height: 48px;
  padding: 0 14px;
  border: none;
  border-radius: 10px;
  background: none;
  color: var(--md-sys-color-on-surface);
  font-family: inherit;
  font-size: 15px;
  text-align: left;
  cursor: pointer;
}

@media (hover: hover) {
  .overflow-item:hover {
    background-color: var(--md-sys-color-surface-variant);
  }
}

.overflow-item .material-symbols-rounded {
  flex-shrink: 0;
  font-size: 22px;
  color: var(--md-sys-color-on-surface-variant);
}

.overflow-item-label {
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.menu-fade-enter-active,
.menu-fade-leave-active {
  transform-origin: top right;
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}

.menu-fade-enter-from,
.menu-fade-leave-to {
  opacity: 0;
  transform: scale(0.94) translateY(-6px);
}
</style>
