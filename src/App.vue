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
    <AppHeader
      :is-fullscreen="isFullscreen"
      :file-exists="isTrackFileExists"
      :minimize-window="minimizeWindow"
      :toggle-fullscreen="toggleFullscreen"
      :close-window="closeWindow"
      @toggle-library="toggleLibrary"
    />

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
                  <!-- 视图切换按钮 -->
                  <button
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
                    v-if="viewMode === 'lyrics'"
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

    <!-- 错误通知浮层 -->
    <ErrorNotifications />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from './stores/player'
import { useThemeStore } from './stores/theme'
import { useConfigStore } from './stores/config'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { useErrorNotification } from './composables/useErrorNotification'
import errorHandler, { ErrorSeverity } from './utils/errorHandler'
import { useTrackInfo } from './composables/useTrackInfo'
import { useLyrics } from './composables/useLyrics'
import { useAutoUpdate } from './composables/useAutoUpdate'
import { useDesktopLyrics } from './composables/useDesktopLyrics'
import { useWindowControls } from './composables/useWindowControls'
import { useAlbumArtInteraction } from './composables/useAlbumArtInteraction'
import { useDominantColor } from './composables/useDominantColor'
import { useImmersiveAutoHide } from '@/composables/useImmersiveAutoHide'
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
import AppHeader from './components/AppHeader.vue'
import ErrorNotifications from './components/ErrorNotifications.vue'
import Settings from './components/Settings.vue'
import MiniPlayer from './components/MiniPlayer.vue'

const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const configStore = useConfigStore()

// 初始化错误通知(浮层组件 ErrorNotifications 自行读取同一模块级单例)
const { showError, unsubscribe: unsubscribeErrorNotification } = useErrorNotification()

const { currentTrack, playlist, audioInfo, currentTrackIndex } = storeToRefs(playerStore)

// 使用 composable 处理音轨信息(标题/艺术家解析逻辑在 AppHeader 中同样复用)
const { watchTrack } = useTrackInfo()

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

// ===== 沉浸式模式：控制栏自动隐藏（状态机实现见 composables/useImmersiveAutoHide） =====
const {
  immersiveControlsVisible,
  markImmersiveActivity,
  cleanup: cleanupImmersiveAutoHide,
} = useImmersiveAutoHide(immersiveCover)

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
  cleanupImmersiveAutoHide()
})

const toggleViewMode = () => {
  viewMode.value = viewMode.value === 'lyrics' ? 'visualizer' : 'lyrics'
}

const toggleLibrary = () => {
  showLibrary.value = !showLibrary.value
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
watch([immersiveCover, dominantLuminance], ([active, luminance]) => {
  if (!active) {
    themeStore.setImmersiveDarkMode(null)
    return
  }
  if ((configStore.general.immersiveAutoTheme ?? true) && luminance !== null) {
    themeStore.setImmersiveDarkMode(luminance < 0.5)
  }
})

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

// 检查当前歌曲文件是否存在：通过后端命令异步校验真实文件，
// 结果写入 ref 供模板绑定。切歌时自增请求序号防竞态——快速切歌时，
// 旧曲目的异步检查结果不会覆盖新曲目的状态。
const isTrackFileExists = ref(true)
let fileCheckSeq = 0

watch(
  () => currentTrack.value?.path,
  (path) => {
    const requestId = ++fileCheckSeq
    // 切歌/清空曲目时先复位为"存在"，避免展示上一首的过期结论
    isTrackFileExists.value = true
    if (!path) return

    invoke<boolean>('check_file_exists', { path })
      .then((exists) => {
        // 仅当仍是最新一次检查时才写入结果
        if (requestId === fileCheckSeq) {
          isTrackFileExists.value = exists
        }
      })
      .catch((e) => {
        // 检查失败（如非 Tauri 环境）时不打扰用户，保持"存在"的默认状态
        errorHandler.handle(e, { severity: ErrorSeverity.LOW, showToUser: false })
      })
  },
  { immediate: true },
)

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

<style scoped src="./App.css"></style>
