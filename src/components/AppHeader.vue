<template>
  <header class="nav-bar" data-tauri-drag-region>
    <!-- 左侧控制区 -->
    <div class="nav-left">
      <button class="icon-button" data-tauri-drag-region="false" @click="emit('toggle-library')">
        <span class="material-symbols-rounded">menu</span>
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
      <ThemeSelector data-tauri-drag-region="false" />
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
    <div class="nav-right">
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
  </header>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { useConfigStore } from '@/stores/config'
import { usePlayerStore } from '@/stores/player'
import { useThemeStore } from '@/stores/theme'
import { useTrackInfo } from '@/composables/useTrackInfo'
import ThemeSelector from './ThemeSelector.vue'

/**
 * 顶栏导航(曲目信息 + 主题/窗口/库入口)。
 * 从 App.vue 拆出:自身状态走 store,窗口控制函数与全屏状态由父级注入
 * (App 的 useWindowControls 实例是唯一状态源),避免二次订阅。
 */
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
</script>

<style scoped>
/* 顶部中间：当前曲目信息 */
/* 左右各留 120px 间距；允许收缩，避免长曲名把两侧按钮挤出屏幕 */
.nav-center {
  min-width: 0;
  overflow: hidden;
  padding: 0 120px;
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

/* 窄窗口时顶部只显示曲名，隐藏艺术家 */
@media (max-width: 640px) {
  .nav-track-artist {
    display: none;
  }
}
</style>
