<template>
  <MiniPlayer v-if="configStore.ui.miniMode" />
  <div v-else class="app-container" :data-fullscreen="isFullscreen" :data-maximized="isMaximized">
    <header class="nav-bar" data-tauri-drag-region>
      <!-- 左侧控制区 -->
      <div class="nav-left">
        <button class="icon-button" data-tauri-drag-region="false" @click="toggleLibrary">
          <span class="material-symbols-rounded">menu</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          title="设置"
          @click="toggleSettings"
        >
          <span class="material-symbols-rounded">settings</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          title="切换主题"
          @click="themeStore.toggleDarkMode"
        >
          <span class="material-symbols-rounded">{{
            themeStore.isDarkMode ? 'light_mode' : 'dark_mode'
          }}</span>
        </button>
        <ThemeSelector data-tauri-drag-region="false" />
      </div>
      <!-- 右侧控制区 -->
      <div class="nav-right">
        <button
          class="icon-button"
          :class="{ active: configStore.lyrics?.desktopLyrics?.enabled }"
          data-tauri-drag-region="false"
          :title="$t('config.toggleDesktopLyrics')"
          @click="toggleDesktopLyrics"
        >
          <span class="material-symbols-rounded">subtitles</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          title="Mini模式"
          @click="configStore.toggleMiniMode"
        >
          <span class="material-symbols-rounded">picture_in_picture_alt</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          title="最小化"
          @click="minimizeWindow"
        >
          <span class="material-symbols-rounded">minimize</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          :title="isFullscreen ? '退出全屏' : '全屏'"
          @click="toggleFullscreen"
        >
          <span class="material-symbols-rounded">{{
            isFullscreen ? 'fullscreen_exit' : 'fullscreen'
          }}</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          title="关闭"
          @click="closeWindow"
        >
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
    </header>

    <main class="main-content">
      <Transition name="slide-left">
        <MusicLibrary v-if="showLibrary" @close="showLibrary = false" />
      </Transition>

      <!-- 配置面板 - 替换主内容区域 -->
      <Transition name="fade" mode="out-in">
        <Settings v-if="configStore.ui.showConfigPanel" key="settings" />
        <div v-else key="player" class="player-container">
          <div class="player-main">
            <!-- 上方区域：左侧专辑封面和歌曲信息，右侧歌词 -->
            <div class="player-upper">
              <!-- 左侧：专辑封面和歌曲信息 -->
              <div class="player-left">
                <div class="album-art-container">
                  <Transition
                    :name="
                      transitionDirection === 'next'
                        ? 'album-art-slide-next'
                        : 'album-art-slide-prev'
                    "
                    mode="out-in"
                  >
                    <div
                      :key="currentTrack ? currentTrack.path : 'no-track'"
                      class="album-art-wrapper"
                      @mousemove="handleAlbumArtMouseMove"
                      @mouseleave="handleAlbumArtMouseLeave"
                    >
                      <div class="album-art" :style="{ backgroundImage: currentTrackCover }">
                        <div
                          v-if="!currentTrack || !currentTrack.coverPath"
                          class="album-art-placeholder"
                        >
                          <span class="material-symbols-rounded">album</span>
                        </div>
                      </div>
                      <!-- 提取封面按钮 -->
                      <button
                        v-if="currentTrack && currentTrack.coverPath"
                        class="extract-cover-btn"
                        :class="{ show: showExtractButton }"
                        :title="$t('player.extractCover')"
                        @click="extractCover"
                      >
                        <span class="material-symbols-rounded">download</span>
                      </button>
                    </div>
                  </Transition>
                </div>

                <Transition name="fade" mode="out-in">
                  <div :key="currentTrack ? currentTrack.path : 'no-track-info'">
                    <div v-if="currentTrack" class="track-info">
                      <h2
                        class="track-title"
                        :title="getTrackTitle(currentTrack) || $t('player.noTrack')"
                      >
                        {{ getTrackTitle(currentTrack) || $t('player.noTrack') }}
                      </h2>
                      <div
                        v-if="getTrackArtist(currentTrack)"
                        class="track-artist"
                        :title="getTrackArtist(currentTrack)"
                      >
                        {{ getTrackArtist(currentTrack) }}
                      </div>
                      <!-- 文件不存在提示 -->
                      <div v-if="currentTrack && !isTrackFileExists" class="file-missing-alert">
                        <span class="material-symbols-rounded">warning</span>
                        <span class="alert-text">{{ $t('player.fileNotFound') }}</span>
                      </div>
                    </div>
                    <div v-else class="track-info-placeholder">
                      <h2 class="track-title">{{ $t('player.noTrack') }}</h2>
                      <div class="track-artist">&nbsp;</div>
                    </div>

                    <div
                      v-if="currentTrack && formattedAudioInfo && configStore.general.showAudioInfo"
                      class="audio-info"
                    >
                      <span class="text-caption">{{ formattedAudioInfo }}</span>
                    </div>
                    <div v-else class="audio-info-placeholder">
                      <span class="text-caption">&nbsp;</span>
                    </div>
                  </div>
                </Transition>
              </div>

              <!-- 右侧：歌词/可视化 -->
              <div class="player-right">
                <!-- 右上角控制区域 -->
                <div class="view-controls-container">
                  <!-- 在线歌词指示图标 -->
                  <div
                    v-if="lyricsSource === 'online'"
                    class="online-lyrics-indicator"
                    :title="$t('lyrics.fromOnline')"
                  >
                    <span class="material-symbols-rounded">cloud_done</span>
                  </div>
                  <!-- 视图切换按钮（非独占模式） -->
                  <button
                    v-if="!configStore.audio.exclusiveMode"
                    class="icon-button view-toggle-btn"
                    :title="viewMode === 'lyrics' ? '切换到波形模式' : '切换到歌词模式'"
                    @click="toggleViewMode"
                  >
                    <span class="material-symbols-rounded">{{
                      viewMode === 'lyrics' ? 'equalizer' : 'lyrics'
                    }}</span>
                  </button>
                </div>

                <Transition name="fade" mode="out-in">
                  <LyricsDisplay
                    v-if="viewMode === 'lyrics' || configStore.audio.exclusiveMode"
                    class="lyrics-container"
                  />
                  <VisualizerPanel v-else class="lyrics-container" />
                </Transition>
              </div>
            </div>

            <!-- 下方区域：进度条和控制按钮 -->
            <div class="player-lower">
              <ProgressBar class="global-progress-bar" />
              <PlayerControls />
            </div>
          </div>
        </div>
      </Transition>
    </main>

    <Transition name="slide-right">
      <PlaylistView v-if="showPlaylist" @close="showPlaylist = false" />
    </Transition>

    <button v-if="playlist.length > 0" class="fab" @click="togglePlaylist">
      <span class="material-symbols-rounded">playlist_play</span>
    </button>

    <!-- 错误通知 -->
    <TransitionGroup name="error-notification" tag="div" class="error-notifications">
      <div
        v-for="notification in errorNotifications"
        :key="notification.id"
        :class="['error-notification', `error-notification--${notification.severity}`]"
        @click="removeError(notification.id)"
      >
        <span class="material-symbols-rounded error-icon">
          {{
            notification.severity === 'error'
              ? 'error'
              : notification.severity === 'warning'
                ? 'warning'
                : 'info'
          }}
        </span>
        <span class="error-message">{{ notification.message }}</span>
        <button class="error-close" @click.stop="removeError(notification.id)">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from './stores/player'
import { useThemeStore } from './stores/theme'
import { useConfigStore } from './stores/config'
import { convertFileSrc } from '@tauri-apps/api/core'
import { useErrorNotification } from './composables/useErrorNotification'
import { useTrackInfo } from './composables/useTrackInfo'
import { useLyrics } from './composables/useLyrics'
import { useAutoUpdate } from './composables/useAutoUpdate'
import { useDesktopLyrics } from './composables/useDesktopLyrics'
import { useWindowControls } from './composables/useWindowControls'
import { useAlbumArtInteraction } from './composables/useAlbumArtInteraction'
import { useGlobalKeyboard } from './composables/useGlobalKeyboard'
import { useAppLifecycle } from './composables/useAppLifecycle'
import PlayerControls from './components/PlayerControls.vue'
import ProgressBar from './components/ProgressBar.vue'
import LyricsDisplay from './components/LyricsDisplay.vue'
import VisualizerPanel from './components/VisualizerPanel.vue'
import MusicLibrary from './components/MusicLibrary.vue'
import PlaylistView from './components/PlaylistView.vue'
import ThemeSelector from './components/ThemeSelector.vue'
import Settings from './components/Settings.vue'
import MiniPlayer from './components/MiniPlayer.vue'

const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const configStore = useConfigStore()

// 初始化错误通知
const {
  errorNotifications,
  showError,
  removeError,
  unsubscribe: unsubscribeErrorNotification,
} = useErrorNotification()

const { currentTrack, playlist, audioInfo, currentTrackIndex } = storeToRefs(playerStore)

// 使用 composable 处理音轨信息
const { getTrackTitle, getTrackArtist, watchTrack } = useTrackInfo()

// 获取歌词来源
const { lyricsSource } = useLyrics()

useDesktopLyrics()

// 自动更新 composable（在 setup 顶层调用，确保内部生命周期钩子能正确注册）
const { checkForUpdates, updateAvailable, newVersion } = useAutoUpdate()

// 窗口控制（封装最小化/全屏/关闭及 isFullscreen/isMaximized 状态）
const {
  isFullscreen,
  isMaximized,
  minimizeWindow,
  toggleFullscreen,
  closeWindow,
  syncWindowState,
} = useWindowControls()

// 专辑封面交互（右下角提取封面按钮的显隐与导出逻辑）
const { showExtractButton, handleAlbumArtMouseMove, handleAlbumArtMouseLeave, extractCover } =
  useAlbumArtInteraction(currentTrack)

// 全局键盘事件（内部自注册 onMounted/onUnmounted 监听 keydown）
useGlobalKeyboard()

// 本地 UI 状态
const showLibrary = ref(false)
const showPlaylist = ref(false)
const viewMode = ref('lyrics') // 'lyrics' or 'visualizer'

const toggleViewMode = () => {
  viewMode.value = viewMode.value === 'lyrics' ? 'visualizer' : 'lyrics'
}

const toggleLibrary = () => {
  showLibrary.value = !showLibrary.value
}

const toggleSettings = () => {
  configStore.toggleConfigPanel()
}

const togglePlaylist = () => {
  showPlaylist.value = !showPlaylist.value
}

const toggleDesktopLyrics = () => {
  const current = configStore.lyrics?.desktopLyrics?.enabled ?? false
  configStore.setDesktopLyricsConfig({ enabled: !current })
}

// 计算属性
const currentTrackCover = computed(() => {
  if (currentTrack.value && currentTrack.value.coverPath) {
    // 使用 convertFileSrc 将本地文件路径转换为可渲染的 URL
    return `url('${convertFileSrc(currentTrack.value.coverPath)}')`
  }
  return 'none' // 如果没有封面，返回none
})

const formattedAudioInfo = computed(() => {
  const { bitrate, sampleRate, channels, bitDepth, format } = audioInfo.value

  const parts = []

  // 格式 (FLAC, MP3, WAV, etc.)
  if (format) {
    parts.push(format)
  }

  // 比特率
  if (bitrate) {
    parts.push(`${bitrate} kbps`)
  }

  // 采样率 (44100 -> 44.1 kHz)
  if (sampleRate) {
    const kHz = sampleRate >= 1000 ? (sampleRate / 1000).toFixed(1).replace(/\.0$/, '') : sampleRate
    parts.push(`${kHz} kHz`)
  }

  // 位深度
  if (bitDepth) {
    parts.push(`${bitDepth} bit`)
  }

  // 声道
  if (channels) {
    if (channels === 2) {
      parts.push('Stereo')
    } else if (channels === 1) {
      parts.push('Mono')
    } else {
      parts.push(`${channels}ch`)
    }
  }

  return parts.join(' | ')
})

// 检查当前歌曲文件是否存在
const isTrackFileExists = computed(() => {
  if (!currentTrack.value) return true
  return !!currentTrack.value.path
})

// 监听当前音轨变化，自动处理标题信息
const stopWatchTrack = watchTrack(() => currentTrack.value)

// 音轨切换动画方向
const transitionDirection = ref<string | null>(null)

// 监听当前音轨索引变化，决定切换动画方向
watch(currentTrackIndex, (newIndex, oldIndex) => {
  if (oldIndex === -1 || newIndex === -1) {
    transitionDirection.value = null
    return
  }

  // 如果播放列表为空，则不进行播放列表循环处理
  const playlistLength = playlist.value.length
  if (playlistLength === 0) {
    transitionDirection.value = null
    return
  }

  if (newIndex === (oldIndex + 1) % playlistLength) {
    transitionDirection.value = 'next'
  } else if (newIndex === (oldIndex - 1 + playlistLength) % playlistLength) {
    transitionDirection.value = 'prev'
  } else if (newIndex > oldIndex) {
    transitionDirection.value = 'next' // 播放列表向后跳选，与"下一首"动画一致
  } else if (newIndex < oldIndex) {
    transitionDirection.value = 'prev' // 播放列表向前跳选，与"上一首"动画一致
  } else {
    transitionDirection.value = null // 同一首歌不处理
  }
})

// 应用生命周期（onMounted/onUnmounted 由 composable 内部注册）
useAppLifecycle({
  checkForUpdates,
  updateAvailable,
  newVersion,
  showError,
  unsubscribeErrorNotification,
  syncWindowState,
  stopWatchTrack,
})
</script>

<style scoped>
/* 过渡动画 */
.slide-left-enter-active,
.slide-left-leave-active {
  transition: transform 0.3s cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

.slide-left-enter-from {
  transform: translateX(-100%);
}

.slide-left-leave-to {
  transform: translateX(-100%);
}

.slide-right-enter-active,
.slide-right-leave-active {
  transition:
    transform 0.3s cubic-bezier(0.25, 0.46, 0.45, 0.94),
    opacity 0.3s ease;
}

.slide-right-enter-from {
  transform: translateX(100%);
  opacity: 0;
}

.slide-right-leave-to {
  transform: translateX(100%);
  opacity: 0;
}

/* 专辑封面过渡动画 */
.album-art-slide-next-enter-active,
.album-art-slide-next-leave-active,
.album-art-slide-prev-enter-active,
.album-art-slide-prev-leave-active {
  transition: all 0.5s cubic-bezier(0.25, 0.46, 0.45, 0.94);
  width: 100%;
  height: 100%;
}

/* 下一首过渡 */
.album-art-slide-next-enter-from {
  transform: translateX(20%) scale(0.9);
  opacity: 0;
}

.album-art-slide-next-leave-to {
  transform: translateX(-20%) scale(0.9);
  opacity: 0;
}

/* 上一首过渡 */
.album-art-slide-prev-enter-from {
  transform: translateX(-20%) scale(0.9);
  opacity: 0;
}

.album-art-slide-prev-leave-to {
  transform: translateX(20%) scale(0.9);
  opacity: 0;
}

/* 过渡结束 */
.album-art-slide-next-enter-to,
.album-art-slide-next-leave-from,
.album-art-slide-prev-enter-to,
.album-art-slide-prev-leave-from {
  transform: translateX(0) scale(1);
  opacity: 1;
}

.app-container {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 12px;
  transition: border-radius 0.3s ease;
  background-color: var(--md-sys-color-surface-container-low);
}

.main-content {
  flex: 1;
  display: flex;
  overflow: hidden;
  padding: 0 0 0.5%;
  background-color: var(--md-sys-color-surface-container-low);
}

.player-container {
  flex: 1;
  display: flex;
  padding: 12px 16px;
  gap: 16px;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-container);
}

.player-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
}

/* 上方区域：左侧专辑封面和歌曲信息，右侧歌词 */
.player-upper {
  flex: 1;
  display: flex;
  gap: max(10px, min((41vw - 600px), 2vw));
  /* margin-bottom: 8px; */
  min-height: 0;
}

/* 下方区域：控制按钮 */
.player-lower {
  display: flex;
  flex-direction: column;
  gap: 0px;
}

.global-progress-bar {
  width: calc(100% + 32px);
  margin-left: -16px;
  margin-right: -16px;
}

/* 左侧：专辑封面和歌曲信息 */
.player-left {
  flex: 0 0 min(600px, 30vw);
  margin-left: 6vw;
  max-width: 600px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

/* 右侧：歌词 */
.player-right {
  flex: 1;
  min-width: 0;
  position: relative;
  margin-right: 6vw;
}

.view-controls-container {
  position: absolute;
  top: 0;
  right: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  gap: 8px;
}

.online-lyrics-indicator {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 50%;
  color: var(--md-sys-color-on-surface-variant);
}

.online-lyrics-indicator .material-symbols-rounded {
  font-size: 24px;
}

/* 响应式设计 */
@media (max-width: 768px) {
  .player-upper {
    flex-direction: column;
  }

  .player-left {
    flex: none;
    max-width: 100%;
    width: 100%;
  }

  .album-art-container {
    width: min(280px, 40vw);
    height: min(280px, 40vw);
  }

  .player-right {
    width: 100%;
  }

  /* 在小屏幕上调整标题字体大小 */
  .track-title {
    font-size: 24px;
    /* 允许最多3行 */
    -webkit-line-clamp: 3;
    line-clamp: 3;
    max-height: 3.9em;
  }

  .track-artist {
    font-size: 16px;
    /* 允许最多2行 */
    -webkit-line-clamp: 2;
    line-clamp: 2;
    max-height: 2.8em;
  }
}

.album-art-container {
  width: min(600px, 45vh);
  height: min(600px, 45vh);
  margin-bottom: 24px;
  flex-shrink: 0;
  position: relative;
}

.album-art-wrapper {
  position: absolute;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  border-radius: var(--md-sys-shape-corner-medium);
  overflow: hidden;
  box-shadow: var(--shadow-medium);
}

.album-art {
  width: 100%;
  height: 100%;
  background-size: cover;
  background-position: center;
  background-color: var(--md-sys-color-surface-variant);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform 0.5s cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

.album-art:hover {
  transform: scale(1.02);
}

/* 提取封面按钮 */
.extract-cover-btn {
  position: absolute;
  bottom: 12px;
  right: 12px;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: 50%;
  background-color: color-mix(in srgb, var(--md-sys-color-primary) 75%, transparent);
  color: var(--md-sys-color-on-primary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  opacity: 0;
  transition: all 0.2s ease;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.2);
  pointer-events: none;
}

/* 当鼠标在右下角区域时显示按钮 */
.extract-cover-btn.show {
  opacity: 1;
  pointer-events: auto;
}

.extract-cover-btn:hover {
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  transform: scale(1.1);
}

.extract-cover-btn .material-symbols-rounded {
  font-size: 20px;
}

.album-art-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--md-sys-color-on-surface-variant);
}

.album-art-placeholder .material-symbols-rounded {
  font-size: 64px;
  margin-bottom: 8px;
}

.track-info {
  width: 100%;
  text-align: center;
  margin-bottom: 16px;
  max-width: min(400px, 40vw);
  /* 添加容器以限制溢出 */
  overflow: hidden;
  position: relative;
  /* 确保可以正确计算高度 */
  min-height: 76px; /* 32px + 18px + 8px + 18px for artist and title */
}

.track-title {
  font-size: 32px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin: 0 0 8px 0;
  word-break: break-word;
  /* Allow long words to break */
  /* 添加多行文本限制 */
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2; /* 最多显示两行 */
  line-clamp: 2;
  -webkit-box-orient: vertical;
  /* 确保短标题不会有多余空间 */
  max-height: 2.6em; /* 约2行的高度 */
  line-height: 1.3;
  transition: all 0.3s ease;
}

.track-artist {
  font-size: 18px;
  color: var(--md-sys-color-on-surface-variant);
  margin-bottom: 8px;
  font-weight: 400;
  /* 添加多行文本限制 */
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 1; /* 最多显示一行 */
  line-clamp: 1;
  -webkit-box-orient: vertical;
  white-space: normal;
  /* 确保短艺术家名不会有多余空间 */
  max-height: 1.4em; /* 约1行的高度 */
  line-height: 1.4;
  transition: all 0.3s ease;
}

.audio-info {
  display: flex;
  justify-content: center;
  margin-top: 8px;
  color: var(--md-sys-color-on-surface-variant);
  max-width: min(400px, 40vw);
  text-align: center;
}

.lyrics-container {
  height: 100%;
  width: 100%;
}

/* 文件不存在提示样式 */
.file-missing-alert {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 8px 12px;
  margin-top: 8px;
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
  font-size: 14px;
  animation: fadeIn 0.3s ease-in-out;
}

.file-missing-alert .material-symbols-rounded {
  font-size: 18px;
}

.alert-text {
  font-weight: 500;
}

.track-info-placeholder {
  width: 100%;
  text-align: center;
  margin-bottom: 16px;
  max-width: min(400px, 40vw);
  visibility: hidden;
}

.audio-info-placeholder {
  display: flex;
  justify-content: center;
  margin-top: 8px;
  max-width: min(400px, 40vw);
  text-align: center;
  visibility: hidden;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }

  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* 全屏和最大化状态下移除圆角 */
.app-container[data-fullscreen='true'],
.app-container[data-maximized='true'] {
  border-radius: 0;
}

/* 错误通知样式 */
.error-notifications {
  position: fixed;
  top: 80px;
  right: 20px;
  z-index: 10000;
  display: flex;
  flex-direction: column;
  gap: 12px;
  pointer-events: none;
}

.error-notification {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-radius: var(--md-sys-shape-corner-medium);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  min-width: 300px;
  max-width: 500px;
  pointer-events: auto;
  cursor: pointer;
  transition: all 0.2s ease;
}

.error-notification:hover {
  transform: translateX(-4px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.error-notification--error {
  border-left: 4px solid var(--md-sys-color-error);
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

.error-notification--warning {
  border-left: 4px solid #f59e0b;
  background-color: #fef3c7;
  color: #92400e;
}

.error-notification--info {
  border-left: 4px solid var(--md-sys-color-on-primary-container);
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-primary-container);
}

.error-icon {
  font-size: 24px;
  flex-shrink: 0;
}

.error-message {
  flex: 1;
  font-size: 14px;
  line-height: 1.4;
  word-break: break-word;
}

.error-close {
  background: none;
  border: none;
  padding: 4px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  color: inherit;
  opacity: 0.7;
  transition: opacity 0.2s ease;
  flex-shrink: 0;
}

.error-close:hover {
  opacity: 1;
}

.error-close .material-symbols-rounded {
  font-size: 20px;
}

/* 错误通知动画 */
.error-notification-enter-active,
.error-notification-leave-active {
  transition: all 0.3s ease;
}

.error-notification-enter-from {
  opacity: 0;
  transform: translateX(100%);
}

.error-notification-leave-to {
  opacity: 0;
  transform: translateX(100%);
}
</style>
