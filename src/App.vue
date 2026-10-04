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
        <div class="background-layer"></div>
        <!-- 上层：靠左放大、裁剪的封面，暗化 + 羽化两路融入 background-layer 的主色 -->
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
      <Transition name="fade" mode="out-in">
        <Settings v-if="configStore.ui.showConfigPanel" key="settings" />
        <div v-else key="player" class="player-container">
          <!-- data-upper-view 挂在 .player-main 而不是 .player-upper：竖屏规则要同时管到 .player-upper
               （面板互斥）与 .player-lower（曲目信息只在封面态贴着进度条），.player-main 是两者最近的共同祖先 -->
          <div
            class="player-main"
            :data-upper-view="upperView"
            :data-mobile="isAndroid ? 'true' : undefined"
          >
            <!-- 上方区域：横屏是"左封面 + 右歌词"双栏，竖屏收成单面板，由 upperView 决定显示哪一块 -->
            <div class="player-upper">
              <!-- 必须挂在 .player-upper 而不是 .player-right 里：竖屏切到封面时 .player-right 整体隐藏，
                   按钮跟着消失就再也切不回歌词/波形了 -->
              <div class="view-controls-container">
                <!-- 在线歌词指示只在桌面端显示：手机上它绝对定位在右上角，正好压在歌词第一行上，
                     而"这句来自在线歌词库"在手机上没有可操作的后续动作 -->
                <div
                  v-if="lyricsSource === 'online' && !isAndroid"
                  class="online-lyrics-indicator"
                  :title="$t('lyrics.fromOnline')"
                >
                  <span class="material-symbols-rounded">cloud_done</span>
                </div>
                <!-- 手机竖屏隐藏：顶栏空间宝贵，竖屏靠点封面/歌词空白处切换；
                     判据要带上平台而非只看方向，桌面窗口绝大多数时候也是横屏 -->
                <button
                  v-if="!isAndroid || !isPortrait"
                  class="icon-button view-toggle-btn"
                  :title="$t(nextUpperViewTitleKey)"
                  @click="cycleUpperView"
                >
                  <span class="material-symbols-rounded">{{ nextUpperViewIcon }}</span>
                </button>
              </div>

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
                      @pointerdown="handleCoverPointerDown"
                      @pointermove="handleCoverPointerMove"
                      @pointerup="handleCoverPointerUp"
                      @pointercancel="handleCoverPointerUp"
                      @pointerleave="handleCoverPointerUp"
                    >
                      <div
                        class="album-art"
                        :style="{ backgroundImage: currentTrackCover }"
                        :title="
                          isPortrait
                            ? $t('window.switchToLyrics')
                            : $t('player.viewCoverFullscreen')
                        "
                        @click.stop="handleAlbumArtClick"
                      >
                        <div
                          v-if="!currentTrack || !currentTrack.coverPath"
                          class="album-art-placeholder"
                        >
                          <span class="material-symbols-rounded">album</span>
                        </div>
                      </div>
                      <!-- 提取封面按钮：桌面端才有，鼠标移到封面右下角时出现 -->
                      <button
                        v-if="!isAndroid && currentTrack && currentTrack.coverPath"
                        class="extract-cover-btn"
                        :class="{ show: showExtractButton }"
                        :title="$t('player.extractCover')"
                        @click="handleExtractCover"
                      >
                        <span class="material-symbols-rounded">download</span>
                      </button>
                      <!-- 手机端长按封面菜单：触摸屏没有 hover，改用显式手势；遮罩铺满封面区域，点空白处关闭 -->
                      <Transition name="fade">
                        <div
                          v-if="showCoverMenu"
                          class="cover-menu-backdrop"
                          @click.stop="closeCoverMenu"
                          @pointerdown.stop
                          @pointerup.stop
                        >
                          <div class="cover-menu" role="menu">
                            <button
                              class="cover-menu-item"
                              role="menuitem"
                              @click.stop="onCoverMenuExtract"
                            >
                              <span class="material-symbols-rounded">download</span>
                              <span>{{ $t('player.extractCover') }}</span>
                            </button>
                            <button
                              class="cover-menu-item"
                              role="menuitem"
                              @click.stop="closeCoverMenu"
                            >
                              <span class="material-symbols-rounded">close</span>
                              <span>{{ $t('common.cancel') }}</span>
                            </button>
                          </div>
                        </div>
                      </Transition>
                    </div>
                  </Transition>
                </div>
              </div>

              <!-- 右侧：歌词/可视化（竖屏下与封面二选一） -->
              <div class="player-right">
                <Transition name="fade" mode="out-in">
                  <!-- blank-click：点歌词面板空白处，竖屏下用它返回封面（哪些元素可点由 LyricsDisplay 判定） -->
                  <LyricsDisplay
                    v-if="panelView === 'lyrics'"
                    class="lyrics-container"
                    @blank-click="handleLyricsBlankClick"
                  />
                  <VisualizerPanel v-else class="lyrics-container" />
                </Transition>
              </div>
            </div>

            <div class="player-lower">
              <!-- 音频信息走 time-middle 插槽：竖屏下它与"已播 / 总时长"同处一行（时间行只在竖屏常显）。
                   横屏这一行整行 display:none，音频信息仍在左下角 .audio-info-corner，不受影响 -->
              <ProgressBar
                class="global-progress-bar"
                :data-mobile="isAndroid ? 'true' : undefined"
              >
                <template #time-middle>
                  <span
                    v-if="currentTrack && formattedAudioInfo && configStore.general.showAudioInfo"
                    :title="formattedAudioInfo"
                  >
                    {{ formattedAudioInfo }}
                  </span>
                </template>
              </ProgressBar>
              <div class="controls-area" :data-mobile="isAndroid ? 'true' : undefined">
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
                <PlayerControls>
                  <template v-if="playlistButtonInTransport" #after-next>
                    <!-- 播放列表入口常显（用户要求）：刚装完还没有任何曲目时也要在，否则用户不知道有这个功能；
                         列表为空时抽屉里显示 playlist.empty -->
                    <button
                      class="icon-button"
                      :class="{ active: showPlaylist }"
                      :title="$t('playlist.title')"
                      @click="togglePlaylist"
                    >
                      <span class="material-symbols-rounded">queue_music</span>
                    </button>
                  </template>
                </PlayerControls>
                <!-- 右下角：桌面歌词与音量控制；竖屏时播放列表开关也在这里（见上面的插槽） -->
                <div class="side-controls">
                  <button
                    v-if="!playlistButtonInTransport"
                    class="icon-button"
                    :class="{ active: showPlaylist }"
                    :title="$t('playlist.title')"
                    @click="togglePlaylist"
                  >
                    <span class="material-symbols-rounded">queue_music</span>
                  </button>
                  <!-- 桌面歌词是一块独立的桌面级浮窗，Android 上没有对应实现 -->
                  <button
                    v-if="!isAndroid"
                    class="icon-button"
                    :class="{ active: configStore.lyrics?.desktopLyrics?.enabled }"
                    :title="$t('config.toggleDesktopLyrics')"
                    @click="toggleDesktopLyrics"
                  >
                    <span class="material-symbols-rounded">subtitles</span>
                  </button>
                  <!-- 音量：手机端不提供。安卓本身有音量键，竖向拖拽滑块在手指上并不好用；
                       省下的宽度正好缓解底部一行的拥挤（411px 宽的机器上主按钮 + 播放列表 + 音量已越过可用宽度） -->
                  <VolumeControl v-if="!isAndroid" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </main>

    <Transition name="slide-left">
      <MusicLibrary
        v-if="showLibrary"
        :data-mobile="isAndroid ? 'true' : undefined"
        @close="showLibrary = false"
      />
    </Transition>

    <Transition name="slide-right">
      <PlaylistView
        v-if="showPlaylist"
        :data-mobile="isAndroid ? 'true' : undefined"
        @close="showPlaylist = false"
      />
    </Transition>

    <ErrorNotifications />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
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
import { usePlatform } from './composables/usePlatform'
import { useOrientation } from './composables/useOrientation'
import { setSystemUiHidden } from './services/appService'
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

// Android 降级：隐藏桌面专属入口（如独立的桌面歌词浮窗）
const { isAndroid } = usePlatform()

// 竖屏把上部区域从"左封面 + 右歌词"双栏改成单面板切换
const { isPortrait, isLandscape } = useOrientation()

// 浮层组件 ErrorNotifications 自行读取同一个模块级单例，这里只取命令式 API
const { showError, showSuccess, unsubscribe: unsubscribeErrorNotification } = useErrorNotification()

// script 里也要文案（提取封面的成功提示），模板侧继续走 $t
const { t } = useI18n()

const { currentTrack, playlist, audioInfo, currentTrackIndex } = storeToRefs(playerStore)

// 标题/艺术家的解析与展示统一由 AppHeader 负责（竖屏封面态也是），这里只留切歌时刷新解析缓存的 watchTrack
const { watchTrack } = useTrackInfo()

const { lyricsSource } = useLyrics()

useDesktopLyrics()

// 必须在 setup 顶层调用，内部的生命周期钩子才能正确注册
const { checkForUpdates, updateAvailable, newVersion } = useAutoUpdate()

// 最小化/全屏/关闭与 isFullscreen/isMaximized 状态
const {
  isFullscreen,
  isMaximized,
  minimizeWindow,
  toggleFullscreen,
  closeWindow,
  syncWindowState,
} = useWindowControls()

// 封面交互：桌面端是右下角的悬停按钮，手机端是长按封面弹出的菜单（触摸屏没有 hover，按钮点不出来）
const {
  showExtractButton,
  showCoverMenu,
  handleAlbumArtMouseMove,
  handleAlbumArtMouseLeave,
  handleCoverPointerDown,
  handleCoverPointerMove,
  handleCoverPointerUp,
  closeCoverMenu,
  consumeLongPressClick,
  extractCover,
} = useAlbumArtInteraction(currentTrack, { longPressEnabled: isAndroid })

// 全局键盘事件（内部自注册 onMounted/onUnmounted 监听 keydown）
useGlobalKeyboard()

const showLibrary = ref(false)
const showPlaylist = ref(false)
const immersiveCover = ref(false)

// 播放列表按钮的落点：桌面端与手机横屏紧跟"下一曲"（PlayerControls 的 after-next 插槽）
// 竖屏那一行已接近满宽（见 App.css 的 @media (orientation: portrait)），继续留在右下角那一组
const playlistButtonInTransport = computed<boolean>(() => !isAndroid.value || isLandscape.value)

// 上部区域显示哪一块：横屏左栏恒为封面、不参与切换，所以是封面与波形两态；
// 竖屏只有封面与歌词两态，波形不进竖屏（那块区域留给封面和歌词更值），切换靠点封面/点歌词空白处而不是按钮
type UpperView = 'cover' | 'lyrics' | 'visualizer'
const upperView = ref<UpperView>('cover')

const upperViewOrder = computed<UpperView[]>(() =>
  isPortrait.value ? ['cover', 'lyrics'] : ['cover', 'visualizer'],
)

const nextUpperView = computed<UpperView>(() => {
  const order = upperViewOrder.value
  const index = order.indexOf(upperView.value)
  // index < 0 理论上不会发生（'cover' 在两个顺序表里都在环上），留个兜底保证 computed 恒有值
  const nextIndex = index < 0 ? 0 : (index + 1) % order.length
  return order[nextIndex] ?? 'cover'
})

const cycleUpperView = (): void => {
  upperView.value = nextUpperView.value
}

// 右侧面板实际渲染什么：只有横屏才可能出现波形，竖屏恒为歌词。不能只判断 upperView，横屏停在波形时
// 转成竖屏，upperView 会短暂仍是 'visualizer'（下面的 watch 随后才归位），这里必须自己不渲染波形
const panelView = computed<'lyrics' | 'visualizer'>(() =>
  !isPortrait.value && upperView.value === 'visualizer' ? 'visualizer' : 'lyrics',
)

// 图标/提示都指向"按下去会看到的那一块"，不能直接拿 upperView 的名字取：横屏的 'cover' 态
// 右栏渲染的是歌词（见 panelView），图标就必须给 lyrics。这套图标桌面与手机共用
const nextUpperViewIcon = computed(() => {
  const next = nextUpperView.value
  if (next === 'visualizer') return 'equalizer'
  return next === 'cover' && isPortrait.value ? 'album' : 'lyrics'
})

const nextUpperViewTitleKey = computed(() => {
  const next = nextUpperView.value
  if (next === 'visualizer') return 'window.switchToVisualizer'
  return next === 'cover' && isPortrait.value ? 'window.switchToCover' : 'window.switchToLyrics'
})

// 进入沉浸式封面模式（仅有封面时可用）
const openImmersiveCover = (): void => {
  if (currentTrack.value?.coverPath) {
    immersiveCover.value = true
  }
}

// 点封面：竖屏翻到歌词，横屏/桌面进入沉浸式封面（竖屏封面已占满整宽，再叠一层全屏封面是冗余的）
const handleAlbumArtClick = (): void => {
  // 长按菜单弹出后浏览器还会补一次 click，不吞掉的话菜单一出现就被这次 click 切成歌词
  if (consumeLongPressClick()) return
  if (isPortrait.value) {
    upperView.value = 'lyrics'
    return
  }
  openImmersiveCover()
}

// 手机上"点了没反应"是最糟的反馈，所以成功给提示；失败由 useAlbumArtInteraction 经 errorHandler 弹具体原因
const handleExtractCover = async (): Promise<void> => {
  const result = await extractCover()
  if (result === 'saved') {
    showSuccess(t('player.extractCoverSuccess'))
  }
}

const onCoverMenuExtract = async (): Promise<void> => {
  closeCoverMenu()
  await handleExtractCover()
}

// 切换上部面板后补一次 resize 广播：歌词靠 resize 把当前行重新滚到中间，竖屏下它可能刚经历 display:none
//（隐藏期间设置的 scrollTop 不生效，必须重算）；波形画布同样只在 resize 时重测尺寸
watch(upperView, () => {
  void nextTick(() => window.dispatchEvent(new Event('resize')))
})

// 转竖屏要收拾横屏留下的状态：退出沉浸式封面（竖屏没有"点左半区退出"的空白，沉浸层样式会与单面板布局打架，
// 封面被压成 0 宽而不可见）；停在波形态时归位到封面（竖屏无波形这一态）
watch(isPortrait, (portrait) => {
  if (!portrait) return
  if (immersiveCover.value) {
    immersiveCover.value = false
  }
  if (upperView.value === 'visualizer') {
    upperView.value = 'cover'
  }
})

// Android 系统栏：导航条常驻会在底部留一条白带，一律收起（从边缘滑动仍可临时唤出）；
// 状态栏在横屏或沉浸封面时收起。控制栏另有 3s 空闲自动隐藏，两者判据不同
watch(
  [isAndroid, isLandscape, immersiveCover],
  ([android, landscape, immersive]) => {
    if (!android) return
    void setSystemUiHidden(landscape || immersive, true)
  },
  { immediate: true },
)

// 竖屏点歌词空白处回封面；横屏左栏恒为封面，这个事件不应有副作用
const handleLyricsBlankClick = (): void => {
  if (isPortrait.value) {
    upperView.value = 'cover'
  }
}

// 沉浸式封面没有退出按钮：点左半区或按 Esc 都能退出；手机竖屏下沉浸封面本来就不可达
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

// 控制栏自动隐藏的状态机见 composables/useImmersiveAutoHide
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

const currentTrackCover = computed(() => {
  if (currentTrack.value && currentTrack.value.coverPath) {
    // 使用 convertFileSrc 将本地文件路径转换为可渲染的 URL
    return `url('${convertFileSrc(currentTrack.value.coverPath)}')`
  }
  return 'none'
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

  if (format) {
    parts.push(format)
  }

  if (bitrate) {
    parts.push(`${bitrate} kbps`)
  }

  // 采样率 (44100 -> 44.1 kHz)
  if (sampleRate) {
    const kHz = sampleRate >= 1000 ? (sampleRate / 1000).toFixed(1).replace(/\.0$/, '') : sampleRate
    parts.push(`${kHz} kHz`)
  }

  if (bitDepth) {
    parts.push(`${bitDepth} bit`)
  }

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

// 通过后端命令异步校验当前曲目文件是否真的存在，结果写入 ref 供模板绑定
// 切歌时自增请求序号防竞态：旧曲目的异步检查结果不会覆盖新曲目的状态
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

const stopWatchTrack = watchTrack(() => currentTrack.value)

const transitionDirection = ref<string | null>(null)

// 索引变化决定动画方向；循环模式下首尾相接，相邻判断要按取模
watch(currentTrackIndex, (newIndex, oldIndex) => {
  if (oldIndex === -1 || newIndex === -1) {
    transitionDirection.value = null
    return
  }

  // 空列表会让下面的取模得到 NaN，先归位为无方向动画
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
    transitionDirection.value = 'next' // 前向跳选沿用"下一首"动画
  } else if (newIndex < oldIndex) {
    transitionDirection.value = 'prev' // 后向跳选沿用"上一首"动画
  } else {
    transitionDirection.value = null // 同一首歌不做方向动画
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
