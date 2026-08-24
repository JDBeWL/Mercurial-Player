<template>
  <MiniPlayer v-if="configStore.ui.miniMode" />
  <div
    v-else
    class="app-container"
    :class="{
      'immersive-cover': immersiveCover,
      'immersive-controls-hidden': immersiveCover && !immersiveControlsVisible,
    }"
    :data-fullscreen="isFullscreen"
    :data-maximized="isMaximized"
  >
    <!-- 沉浸式封面层：主色背景 + 左侧羽化封面 -->
    <Transition name="fade">
      <div
        v-if="immersiveCover"
        class="immersive-layer"
        :style="{ '--immersive-bg': immersiveBackground }"
      >
        <!-- 1. 底层：封面主色背景 -->
        <div class="background-layer"></div>
        <!-- 2. 中层：靠左放大、裁剪的封面，暗化+羽化双通道融入背景 -->
        <div class="cover-mask-layer">
          <img v-if="coverDisplayUrl" :src="coverDisplayUrl" class="full-cover" alt="" />
        </div>
      </div>
    </Transition>
    <header class="nav-bar" data-tauri-drag-region>
      <!-- 左侧控制区 -->
      <div class="nav-left">
        <button class="icon-button" data-tauri-drag-region="false" @click="toggleLibrary">
          <span class="material-symbols-rounded">menu</span>
        </button>
        <button
          class="icon-button"
          data-tauri-drag-region="false"
          :title="$t('nav.settings')"
          @click="toggleSettings"
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
                  v-if="!isTrackFileExists"
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

    <main class="main-content">
      <!-- 配置面板 -->
      <Transition name="fade" mode="out-in">
        <Settings v-if="configStore.ui.showConfigPanel" key="settings" />
        <div v-else key="player" class="player-container">
          <div class="player-main">
            <!-- 上方区域：左侧专辑封面，右侧歌词 -->
            <div class="player-upper">
              <!-- 左侧：专辑封面（沉浸式封面模式下点击退出） -->
              <div
                class="player-left"
                :title="immersiveCover ? $t('player.exitCoverFullscreen') : undefined"
                @click="handlePlayerLeftClick"
              >
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
                      <div
                        class="album-art"
                        :style="{ backgroundImage: currentTrackCover }"
                        :title="$t('player.viewCoverFullscreen')"
                        @click.stop="openImmersiveCover"
                      >
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
                    :title="
                      viewMode === 'lyrics'
                        ? $t('window.switchToVisualizer')
                        : $t('window.switchToLyrics')
                    "
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
              <div class="controls-area">
                <!-- 左下角：音频信息（与封面水平居中对齐） -->
                <div class="audio-info-corner">
                  <div
                    v-if="currentTrack && formattedAudioInfo && configStore.general.showAudioInfo"
                    class="audio-info-text"
                    :title="formattedAudioInfo"
                  >
                    {{ formattedAudioInfo }}
                  </div>
                </div>
                <PlayerControls />
                <!-- 右下角：播放列表开关、桌面歌词与音量控制 -->
                <div class="side-controls">
                  <button
                    v-if="playerStore.playlist.length > 0"
                    class="icon-button"
                    :class="{ active: showPlaylist }"
                    :title="$t('playlist.title')"
                    @click="togglePlaylist"
                  >
                    <span class="material-symbols-rounded">queue_music</span>
                  </button>
                  <button
                    class="icon-button"
                    :class="{ active: configStore.lyrics?.desktopLyrics?.enabled }"
                    :title="$t('config.toggleDesktopLyrics')"
                    @click="toggleDesktopLyrics"
                  >
                    <span class="material-symbols-rounded">subtitles</span>
                  </button>
                  <VolumeControl />
                </div>
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </main>

    <Transition name="slide-left">
      <MusicLibrary v-if="showLibrary" @close="showLibrary = false" />
    </Transition>

    <Transition name="slide-right">
      <PlaylistView v-if="showPlaylist" @close="showPlaylist = false" />
    </Transition>

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
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
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
import { useDominantColor } from './composables/useDominantColor'
import { useImmersiveCover } from './composables/useImmersiveCover'
import { useGlobalKeyboard } from './composables/useGlobalKeyboard'
import { useAppLifecycle } from './composables/useAppLifecycle'
import type { ImmersiveColorScheme } from './types'
import PlayerControls from './components/PlayerControls.vue'
import VolumeControl from './components/VolumeControl.vue'
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
const immersiveCover = ref(false)
const viewMode = ref('lyrics') // 'lyrics' or 'visualizer'

// 进入沉浸式封面模式（仅有封面时可用）
const openImmersiveCover = (): void => {
  if (currentTrack.value?.coverPath) {
    immersiveCover.value = true
  }
}

// 沉浸式封面模式下点击左侧区域退出
const handlePlayerLeftClick = (): void => {
  if (immersiveCover.value) {
    immersiveCover.value = false
  }
}

// Esc 退出沉浸式封面模式；其他按键视为用户活动（显示控制栏）
const handleCoverKeydown = (e: KeyboardEvent): void => {
  if (e.key === 'Escape' && immersiveCover.value) {
    immersiveCover.value = false
    return
  }
  if (immersiveCover.value) {
    markImmersiveActivity()
  }
}

// ===== 沉浸式模式：控制栏自动隐藏 =====
const IMMERSIVE_IDLE_DELAY = 3000 // 无操作隐藏延时（ms）
const IMMERSIVE_TOP_AREA = 96 // 顶部控制栏热区高度（覆盖顶栏及其下方缓冲区）
const IMMERSIVE_BOTTOM_AREA = 140 // 底部控制栏热区高度（覆盖磨砂玻璃底栏及其上方缓冲区）

const immersiveControlsVisible = ref(true)
let immersiveHideTimer: ReturnType<typeof setTimeout> | null = null
let immersivePointerY = -1

// 鼠标是否位于顶/底控制栏热区（悬停时不隐藏，保证可点击）
const immersivePointerInControls = (): boolean => {
  if (immersivePointerY < 0) return false
  return (
    immersivePointerY <= IMMERSIVE_TOP_AREA ||
    immersivePointerY >= window.innerHeight - IMMERSIVE_BOTTOM_AREA
  )
}

// 用户活动：显示控制栏并重置隐藏计时
const markImmersiveActivity = (): void => {
  immersiveControlsVisible.value = true
  if (immersiveHideTimer) clearTimeout(immersiveHideTimer)
  immersiveHideTimer = setTimeout(() => {
    immersiveHideTimer = null
    // 鼠标仍悬停在控制栏区域时继续等待，否则隐藏
    if (immersivePointerInControls()) {
      markImmersiveActivity()
      return
    }
    immersiveControlsVisible.value = false
  }, IMMERSIVE_IDLE_DELAY)
}

const handleImmersivePointerMove = (e: MouseEvent): void => {
  immersivePointerY = e.clientY
  markImmersiveActivity()
}

// 鼠标移出窗口：清空热区判定。
const handleImmersivePointerLeave = (): void => {
  immersivePointerY = -1
  markImmersiveActivity()
}

// 窗口失焦：用户已转向其他窗口（如点击桌面），立即隐藏控制栏
const handleImmersiveBlur = (): void => {
  immersivePointerY = -1
  if (immersiveHideTimer) {
    clearTimeout(immersiveHideTimer)
    immersiveHideTimer = null
  }
  immersiveControlsVisible.value = false
}

// 窗口重新聚焦：恢复控制栏并重新进入空闲计时
const handleImmersiveFocus = (): void => {
  markImmersiveActivity()
}

// 进入/退出沉浸模式时挂载/卸载自动隐藏逻辑
watch(immersiveCover, (active) => {
  if (active) {
    immersiveControlsVisible.value = true
    immersivePointerY = -1
    window.addEventListener('mousemove', handleImmersivePointerMove, { passive: true })
    document.addEventListener('mouseleave', handleImmersivePointerLeave)
    window.addEventListener('blur', handleImmersiveBlur)
    window.addEventListener('focus', handleImmersiveFocus)
    markImmersiveActivity()
  } else {
    window.removeEventListener('mousemove', handleImmersivePointerMove)
    document.removeEventListener('mouseleave', handleImmersivePointerLeave)
    window.removeEventListener('blur', handleImmersiveBlur)
    window.removeEventListener('focus', handleImmersiveFocus)
    if (immersiveHideTimer) {
      clearTimeout(immersiveHideTimer)
      immersiveHideTimer = null
    }
    immersiveControlsVisible.value = true
    immersivePointerY = -1
  }
})

// 打开设置时退出沉浸模式（设置面板为不透明界面，顶栏需恢复底色），关闭时恢复
const wasImmersiveBeforeSettings = ref(false)
watch(
  () => configStore.ui.showConfigPanel,
  (open) => {
    if (open) {
      wasImmersiveBeforeSettings.value = immersiveCover.value
      if (immersiveCover.value) immersiveCover.value = false
    } else if (wasImmersiveBeforeSettings.value) {
      wasImmersiveBeforeSettings.value = false
      immersiveCover.value = true
    }
  },
)

// 切换到迷你模式时退出沉浸模式（迷你窗口无沉浸层，避免主题深浅覆盖残留）
watch(
  () => configStore.ui.miniMode,
  (mini) => {
    if (mini && immersiveCover.value) {
      immersiveCover.value = false
    }
  },
)

onMounted(() => {
  window.addEventListener('keydown', handleCoverKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleCoverKeydown)
  window.removeEventListener('mousemove', handleImmersivePointerMove)
  document.removeEventListener('mouseleave', handleImmersivePointerLeave)
  window.removeEventListener('blur', handleImmersiveBlur)
  window.removeEventListener('focus', handleImmersiveFocus)
  if (immersiveHideTimer) {
    clearTimeout(immersiveHideTimer)
    immersiveHideTimer = null
  }
})

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

// 沉浸式封面：提取封面主色作为背景（提取失败时回退主题色）。
// 取色风格由设置控制：album = 专辑主题色（全图代表色），fusion = 封面融合（取右缘条带）
const immersiveColorScheme = computed<ImmersiveColorScheme>(
  () => configStore.general.immersiveColorScheme ?? 'album',
)
const { dominantColor, dominantLuminance } = useDominantColor(
  computed(() => currentTrack.value?.coverPath),
  immersiveColorScheme,
)
const immersiveBackground = computed(
  () => dominantColor.value || 'var(--md-sys-color-surface-container)',
)

// 沉浸式模式：根据封面主色亮度自动切换深/浅主题（亮背景配浅色主题、暗背景配深色主题），
// 保证前景文字可读；退出时恢复用户主题偏好。取色失败（亮度为 null）时保持当前主题。
watch(
  [immersiveCover, dominantLuminance],
  ([active, luminance]) => {
    if (!active) {
      themeStore.setImmersiveDarkMode(null)
      return
    }
    if ((configStore.general.immersiveAutoTheme ?? true) && luminance !== null) {
      themeStore.setImmersiveDarkMode(luminance < 0.5)
    }
  },
)

// 封面展示 URL（供沉浸式封面的 <img> 使用）：
// 源图不够大时用 pica(Lanczos3) 预放大到精确显示尺寸，避免浏览器双线性放大发糊
const { coverDisplayUrl } = useImmersiveCover(
  computed(() => currentTrack.value?.coverPath),
  immersiveCover,
)

// 音频信息文案（格式 | 比特率 | 采样率 | 位深度 | 声道）
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
  background-color: var(--md-sys-color-surface-container-low);
}

.main-content {
  flex: 1;
  display: flex;
  overflow: hidden;
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

/* 三列布局：左侧音频信息 / 中间控制组 / 右侧桌面歌词+音量 */
.controls-area {
  margin-top: 8px;
  display: flex;
  align-items: center;
  /* 两侧区域隐藏或内容为空时仍保持控制组居中 */
  justify-content: center;
}

.controls-area :deep(.player-controls) {
  width: auto;
}

/* 左下角：音频信息容器 */
.audio-info-corner {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  overflow: hidden;
}

/* 与 .player-left 相同的几何结构,空间不足时收窄避免与控制按钮重叠 */
.audio-info-text {
  flex: 0 0 auto;
  width: min(600px, 34vw);
  max-width: calc(100% - 6vw);
  margin-left: 6vw;
  font-size: 12px;
  font-weight: 400;
  line-height: 1.4;
  text-align: center;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--md-sys-color-on-surface-variant);
  /* 细微投影增加与背景的分离度，深浅主题下均适用 */
  text-shadow: 0 0px 1px rgba(0, 0, 0, 0.1);
}

/* 右下角：桌面歌词与音量，右边距与左侧一致 */
.side-controls {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  padding-right: 8px;
}

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

/* 窄窗口时隐藏两侧区域，避免与控制按钮组重叠 */
@media (max-width: 640px) {
  .audio-info-corner,
  .side-controls {
    display: none;
  }

  /* 窄窗口时顶部只显示曲名，隐藏艺术家 */
  .nav-track-artist {
    display: none;
  }
}

.global-progress-bar {
  width: calc(100% + 32px);
  margin-left: -16px;
  margin-right: -16px;
}

/* 左侧：专辑封面 */
.player-left {
  flex: 0 0 min(600px, 36vw);
  margin-left: 5vw;
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
  margin-right: 4vw;
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

  /* 窄屏下封面改为全宽居中，音频信息无法与其对齐，退化为左对齐 */
  .audio-info-text {
    width: auto;
    max-width: 100%;
    margin-left: 8px;
    text-align: left;
  }
}

.album-art-container {
  width: min(600px, 54vh);
  height: min(600px, 54vh);
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
  cursor: pointer;
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

.lyrics-container {
  height: 100%;
  width: 100%;
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}

/* 全屏和最大化状态下移除圆角 */
.app-container[data-fullscreen='true'],
.app-container[data-maximized='true'] {
  border-radius: 0;
}

/* ===== 沉浸式封面模式 ===== */

/* 沉浸层：铺满整个窗口，位于内容之下（nav-bar z-index 100 在其上） */
.immersive-layer {
  position: absolute;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  overflow: hidden;
}

/* 底层：封面主色背景（颜色由脚本提取后经 CSS 变量注入，失败时回退主题色） */
.background-layer {
  position: absolute;
  inset: 0;
  background-color: var(--immersive-bg);
}

/* 中层：靠左放大、裁剪的封面。
   融合采用双通道：::after 先把封面向背景色"染色"，
   mask 再缓慢淡出 alpha——观感是封面渐渐沉入背景，
   而非像素被直接擦除 */
.cover-mask-layer {
  position: absolute;
  left: 0;
  top: 0;
  height: 100%;
  aspect-ratio: 1 / 1; /* 宽度跟随窗口高度，封面区域始终为正方形，不被裁切成矩形 */

  /* alpha 通道：70% 后才启动、分段缓出，只负责终态的消失 */
  -webkit-mask-image: linear-gradient(
    to right,
    black 0%,
    black 70%,
    rgba(0, 0, 0, 0.74) 82%,
    rgba(0, 0, 0, 0.34) 92%,
    transparent 100%
  );
  mask-image: linear-gradient(
    to right,
    black 0%,
    black 70%,
    rgba(0, 0, 0, 0.74) 82%,
    rgba(0, 0, 0, 0.34) 92%,
    transparent 100%
  );
}

/* 染色层：封面右侧先向背景色降亮度沉入，纹理渐隐而非被擦除 */
.cover-mask-layer::after {
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(
    to right,
    transparent 58%,
    color-mix(in srgb, var(--immersive-bg) 26%, transparent) 72%,
    color-mix(in srgb, var(--immersive-bg) 68%, transparent) 87%,
    var(--immersive-bg) 100%
  );
}

.full-cover {
  width: 100%;
  height: 100%;
  object-fit: cover; /* 保证封面拉伸铺满不变形 */
  object-position: left center; /* 保持人物/主体居左显示 */
}

/* 沉浸模式底部控制区：通栏磨砂玻璃条，铺满窗口宽度和底边 */
.app-container.immersive-cover .player-lower {
  /* 负边距抵消 .player-container 的 12px 16px 内边距，玻璃条贴住窗口左右和底部 */
  margin-left: -16px;
  margin-right: -16px;
  margin-bottom: -16px;
  padding: 12px 16px 16px;
  background: rgba(255, 255, 255, 0);
  /* -webkit- 前缀必须在前。构建时 CSS 压缩器会把前缀版与标准版
     视为同一属性去重、仅保留最后一条；若标准版在前，产物会只剩 -webkit- 版，
     而 WebView2(Chromium) 只认不带前缀的 backdrop-filter */
  -webkit-backdrop-filter: blur(10px) saturate(0.5);
  backdrop-filter: blur(10px) saturate(0.5);
  border-top: 1px solid rgba(255, 255, 255, 0.18);
  box-shadow: 0 -4px 24px rgba(0, 0, 0, 0.1);
}

/* 全屏/最大化时窗口无圆角，玻璃条圆角同步归零 */
.app-container[data-fullscreen='true'].immersive-cover .player-lower,
.app-container[data-maximized='true'].immersive-cover .player-lower {
  border-radius: 0;
}

/* 沉浸模式开启自动深浅色后，文字颜色跟随主题切换，不再固定白色 */
.app-container.immersive-cover .audio-info-text {
  color: var(--md-sys-color-on-surface);
  /* 磨砂玻璃上增加细微投影，深浅主题下均保持与背景分离 */
  text-shadow: 0 0px 2px rgba(0, 0, 0, 0.15);
}

/* 进度条沿用默认的 -16px 通栏负边距，正好从窗口左边缘贯穿到右边缘 */
/* 顶/底控制栏及右上角歌词切换按钮的滑出/滑入过渡（仅沉浸模式启用） */
.app-container.immersive-cover .nav-bar,
.app-container.immersive-cover .player-lower,
.app-container.immersive-cover .view-controls-container {
  transition:
    transform 0.4s cubic-bezier(0.25, 0.46, 0.45, 0.94),
    opacity 0.3s ease;
}

.app-container.immersive-cover .player-lower {
  padding-top: 0;
}

/* 隐藏状态：顶栏向上滑出、底栏向下滑出，且不再拦截点击 */
.app-container.immersive-cover.immersive-controls-hidden .nav-bar {
  transform: translateY(-100%);
  opacity: 0;
  pointer-events: none;
}

.app-container.immersive-cover.immersive-controls-hidden .player-lower {
  transform: translateY(100%);
  opacity: 0;
  pointer-events: none;
}

/* 右上角歌词/波形切换按钮跟随顶栏一起隐藏 */
.app-container.immersive-cover.immersive-controls-hidden .view-controls-container {
  transform: translateY(-40px);
  opacity: 0;
  pointer-events: none;
}

/* 沉浸模式下内容浮于沉浸层之上，背景让位给封面 */
.app-container.immersive-cover .main-content {
  position: relative;
  z-index: 1;
  background-color: transparent;
}

.app-container.immersive-cover .player-container {
  background-color: transparent;
}

/* 顶栏背景完全透明，图标直接浮于封面之上 */
.app-container.immersive-cover .nav-bar {
  background-color: transparent;
}

/* 小封面由左半沉浸封面替代 */
.app-container.immersive-cover .album-art-container {
  display: none;
}

/* 沉浸模式下点击左半区域退出 */
.app-container.immersive-cover .player-left {
  cursor: pointer;
}

.app-container.immersive-cover .player-upper {
  flex-direction: row;
}

.app-container.immersive-cover .player-left {
  flex: 0 0 min(600px, 36vw);
  max-width: 600px;
  width: auto;
}

.app-container.immersive-cover .player-right {
  flex: 1;
  width: auto;
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
