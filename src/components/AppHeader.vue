<template>
  <!-- 拖拽区在 Android 上没有对应实现（系统标题栏/手势返回才是移动端范式） -->
  <header
    class="nav-bar"
    :data-mobile="isAndroid ? 'true' : undefined"
    :data-settings-open="settingsOpen ? 'true' : undefined"
    :data-tauri-drag-region="isAndroid ? null : ''"
  >
    <!-- 左侧控制区 -->
    <div class="nav-left">
      <!-- 音乐库入口。图标按平台分开：汉堡(menu) 的语义是"展开侧边栏抽屉"，
           在手机上指代"音乐库"太含糊，换成 library_music 直接表达内容。
           桌面端保留 menu —— 那里汉堡就是标准的侧栏开关。 -->
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
    <!-- 中间：当前曲目信息（双行显示） -->
    <div class="nav-center" data-tauri-drag-region>
      <Transition name="fade" mode="out-in">
        <div :key="currentTrack ? currentTrack.path : 'no-track-nav'" class="nav-track-info">
          <template v-if="currentTrack">
            <div
              class="nav-track-title"
              :title="getTrackTitle(currentTrack) || $t('player.noTrack')"
            >
              <span class="nav-track-title-text">
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
              class="nav-track-artist"
              :title="getTrackArtist(currentTrack)"
            >
              {{ getTrackArtist(currentTrack) }}
            </div>
          </template>
          <template v-else>
            <div class="nav-track-artist">{{ $t('player.noTrack') }}</div>
          </template>
        </div>
      </Transition>
    </div>
    <!-- 右侧控制区 -->
    <!-- 迷你模式 / 最小化 / 全屏 / 关闭都是桌面窗口概念：
         手机上本就是全屏 Activity，"最小化/关闭"由系统手势负责；
         留着四个按钮既出不了效果，又占掉顶栏一大半宽度 -->
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
        <div v-if="overflowOpen" ref="overflowMenuRef" class="overflow-menu" role="menu">
          <!-- 已经在设置面板里了就不再提供"设置"入口（点了等于自己关自己） -->
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

/** 顶栏导航(曲目信息 + 主题/窗口/库入口)。从 App.vue 拆出:自身状态走 store,窗口控制函数与全屏
 *  状态由父级注入(App 的 useWindowControls 实例是唯一状态源),避免二次订阅。 */
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

/** 溢出菜单只在手机竖屏出现（横屏与桌面端维持原来的平铺按钮） */
const showOverflowMenu = computed<boolean>(() => isAndroid.value && isPortrait.value)

/** 设置面板打开时顶栏让出中间那块曲名（面板自己带标题，同屏两份是重复信息） */
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

// 点菜单外的任意位置关闭。点触发按钮自身的冒泡由模板的 @click.stop 拦掉，
// 否则「打开」会在同一次点击里被这里立刻收回去。
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
/* 三列栅格把曲目信息真正钉在窗口正中。flex 下 .nav-center 只能在"左侧按钮之后剩下的空间"里居中，
   Android 上右侧窗口按钮整组被 v-if 掉，那 ~200px 就全成了偏移量，标题明显偏右；栅格左右各 1fr
   恒等宽，中间列的中心即窗口中心。中间列 minmax(0, auto) 让长曲名压成省略号而不顶掉两侧按钮。 */
.nav-bar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, auto) minmax(0, 1fr);
  align-items: center;
  column-gap: 8px;
}

.nav-left {
  justify-self: start;
}

.nav-right {
  justify-self: end;
}

/* 曲目信息：留一点内边距避免长曲名贴着按钮；
   原来那 120px 是 flex 时代给两侧按钮预留的对称留白，栅格下已无必要 */
.nav-center {
  justify-self: center;
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

/* 文件不存在警告图标（曲名右侧） */
.nav-file-warning {
  flex-shrink: 0;
  font-size: 16px;
  color: var(--md-sys-color-error);
}

/* 真正窄的屏幕上（手机竖屏）没法两全其美：左侧按钮群本身就占掉大半宽度，
   再把曲名塞进同一行就只能剩几十像素。这时才退化成两行——第一行按钮、第二行曲名。
   栅格版的"两行"要用 grid-template-areas 表达，flex 的 order/flex-basis 在栅格里不生效。 */
@media (max-width: 640px) {
  .nav-bar {
    grid-template-columns: 1fr auto;
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

/* 手机竖屏：低频操作收进溢出菜单，.nav-left 的盒子消失（display: contents）让子项直接成为栅格项。
   整块用 [data-mobile='true'] 守卫，(orientation: portrait) 在桌面把窗口拉成窄高时同样会命中。
   必须撤掉 640px 那条窄屏规则留下的两行 areas，否则曲名会被自动放置甩到第二行、顶栏高度翻倍。 */
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

  /* 2 = 设置、3 = 明暗：手机上收进溢出菜单。
     第 4 个是 ThemeSelector，由它自己在组件内部隐藏触发按钮 ——
     颜色面板还挂在那个节点上，整体 display:none 会把面板一起藏掉。 */
  .nav-bar[data-mobile='true'] .nav-left > :nth-child(2),
  .nav-bar[data-mobile='true'] .nav-left > :nth-child(3) {
    display: none;
  }

  .nav-bar[data-mobile='true'] .nav-center {
    grid-area: 1 / 2;
    padding: 0 8px;
  }

  /* 桌面窄高窗口的窗口按钮与溢出菜单互斥（v-if 条件不同），可共用第 3 列 */
  .nav-bar[data-mobile='true'] .nav-right,
  .nav-bar[data-mobile='true'] .nav-overflow-btn {
    grid-area: 1 / 3;
  }

  /* 覆盖上面 640px 那条：竖屏现在有位置了，艺术家要显示出来 */
  .nav-bar[data-mobile='true'] .nav-track-artist {
    display: block;
  }

  /* 设置面板打开时顶栏让出中间那块曲名（面板自带标题，同屏两份是重复信息）。
     用 visibility 而不是 display —— 保留它在栅格里的占位，进出设置时两侧按钮不跳。 */
  .nav-bar[data-mobile='true'][data-settings-open='true'] .nav-center {
    visibility: hidden;
  }
}

/* ===== 溢出菜单（只有手机竖屏会渲染）===== */
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
  /* 不能用 surface-container-* 系列：本应用主题只输出 29 个基础角色，
     那一族全部不存在，声明会静默失效变透明（见 tests/utils/themeTokens.test.ts
     的棘轮断言）。浮层用真实存在的 surface。 */
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

.overflow-item:hover {
  background-color: var(--md-sys-color-surface-variant);
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
