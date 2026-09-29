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
          <!-- data-upper-view 挂在 .player-main 上而不是 .player-upper 上：竖屏的布局规则现在要同时
               管到 .player-upper（面板互斥）和 .player-lower（曲目信息只有封面态才贴着进度条），
               .player-main 是这两块最近的共同祖先。 -->
          <div
            class="player-main"
            :data-upper-view="upperView"
            :data-mobile="isAndroid ? 'true' : undefined"
          >
            <!-- 上方区域：横屏是"左封面 + 右歌词"双栏；竖屏收成单面板，
                 由 upperView 决定显示封面 / 歌词中的哪一个 -->
            <div class="player-upper">
              <!-- 右上角控制区域。必须挂在 .player-upper 上而不是 .player-right 里：
                   竖屏切到封面时 .player-right 整体隐藏，按钮跟着一起消失的话
                   就再也切不回歌词/波形了。 -->
              <div class="view-controls-container">
                <!-- 在线歌词指示图标：只在桌面端显示。手机上它绝对定位在上部区域右上角，正好压在歌词
                     面板的第一行上，而"这句歌词来自在线歌词库"在手机上并没有可操作的后续动作，
                     所以按最小代价直接不渲染。 -->
                <div
                  v-if="lyricsSource === 'online' && !isAndroid"
                  class="online-lyrics-indicator"
                  :title="$t('lyrics.fromOnline')"
                >
                  <span class="material-symbols-rounded">cloud_done</span>
                </div>
                <!-- 沉浸封面没有独立的"退出"按钮：点左半区、按 Esc 都能退出
                     （见 handlePlayerLeftClick / handleCoverKeydown），
                     而手机竖屏下沉浸封面本来就不可达（点封面是"翻到歌词"）。 -->
                <!-- 视图切换：只在桌面端出现。手机上没有波形这一态，横屏本来就是"左封面 + 右歌词"双栏
                     不需要切换，竖屏靠点封面/点歌词空白处切换，顶栏空间宝贵。判据必须用平台（isAndroid）
                     而不是方向：桌面窗口绝大多数时候也是横屏，用方向判据会把桌面端一起改掉。 -->
                <button
                  v-if="!isAndroid"
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
                      <!-- 提取封面按钮：桌面端才有（鼠标移到封面右下角出现）。 -->
                      <button
                        v-if="!isAndroid && currentTrack && currentTrack.coverPath"
                        class="extract-cover-btn"
                        :class="{ show: showExtractButton }"
                        :title="$t('player.extractCover')"
                        @click="handleExtractCover"
                      >
                        <span class="material-symbols-rounded">download</span>
                      </button>
                      <!-- 手机端：长按封面弹出的菜单。触摸屏没有 hover，按钮点不出来，
                           所以换成显式手势。遮罩铺满封面区域，点空白处即可关闭。 -->
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
                  <!-- blank-click：点歌词面板的空白处。竖屏下用它返回封面
                       （判定逻辑在 LyricsDisplay 内部，那里才知道哪些元素可点） -->
                  <LyricsDisplay
                    v-if="panelView === 'lyrics'"
                    class="lyrics-container"
                    @blank-click="handleLyricsBlankClick"
                  />
                  <VisualizerPanel v-else class="lyrics-container" />
                </Transition>
              </div>
            </div>

            <!-- 下方区域：进度条 + 控制按钮 -->
            <div class="player-lower">
              <!-- 音频信息走 time-middle 插槽：竖屏下它与"已播 / 总时长"
                   同处一行（时间行只在竖屏常显）。横屏这一行整行 display:none，
                   音频信息仍在左下角 .audio-info-corner 里，不受影响。 -->
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
                <PlayerControls />
                <!-- 右下角：播放列表开关、桌面歌词与音量控制 -->
                <div class="side-controls">
                  <!-- 播放列表入口常显（用户要求）：刚装完还没有任何曲目时也要在，否则底部一行的右侧是
                       空的，用户不知道有这个功能；列表为空时抽屉里显示 playlist.empty。这也让底部一行的
                       按钮数量恒定，不会因为列表有无而把整行的间距改掉。 -->
                  <button
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
                  <!-- 音量：手机端不提供。安卓本身有音量键，且这个竖向拖拽滑块
                       在手指上并不好用；省下的宽度正好缓解底部一行的拥挤
                       （411px 宽的机器上，主按钮 + 播放列表 + 音量已经越过可用宽度）。 -->
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

    <!-- 错误通知浮层 -->
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

// 方向判定：竖屏把上部区域从"左封面+右歌词"改成单面板切换
const { isPortrait, isLandscape } = useOrientation()

// 初始化错误通知(浮层组件 ErrorNotifications 自行读取同一模块级单例)
const { showError, showSuccess, unsubscribe: unsubscribeErrorNotification } = useErrorNotification()

// script 里也要用到文案（提取封面的成功提示），模板侧继续走 $t
const { t } = useI18n()

const { currentTrack, playlist, audioInfo, currentTrackIndex } = storeToRefs(playerStore)

// 音轨信息：标题/艺术家的解析与展示现在统一由 AppHeader 负责（竖屏封面态也是），
// 这里只留 watchTrack —— 切歌时刷新标题解析缓存。
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

// 专辑封面交互：桌面端是右下角的悬停按钮，手机端是长按封面弹出的菜单
// （触摸屏没有 hover，按钮永远点不出来）
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

// 本地 UI 状态
const showLibrary = ref(false)
const showPlaylist = ref(false)
const immersiveCover = ref(false)

// 上部区域当前显示哪一块：封面 / 歌词 / 波形。横屏左栏恒为封面、不参与切换，所以是封面与波形两态；
// 竖屏只有封面与歌词两态，波形不进竖屏（手机上那块区域留给封面和歌词更值），切换不靠按钮，
// 而是点封面看歌词、点歌词空白处回封面。
type UpperView = 'cover' | 'lyrics' | 'visualizer'
const upperView = ref<UpperView>('cover')

const upperViewOrder = computed<UpperView[]>(() =>
  isPortrait.value ? ['cover', 'lyrics'] : ['cover', 'visualizer'],
)

const nextUpperView = computed<UpperView>(() => {
  const order = upperViewOrder.value
  const index = order.indexOf(upperView.value)
  // index < 0 理论上不会发生（'cover' 在两个顺序表里都在环上），
  // 留个兜底保证这个 computed 恒有值
  const nextIndex = index < 0 ? 0 : (index + 1) % order.length
  return order[nextIndex] ?? 'cover'
})

const cycleUpperView = (): void => {
  upperView.value = nextUpperView.value
}

// 右侧面板实际渲染什么：只有横屏才可能出现波形，竖屏恒为歌词。不能只判断 upperView，横屏停在波形时
// 转成竖屏，upperView 会短暂仍是 'visualizer'（下面的 watch 随后才把它归位），这里必须自己不渲染波形。
const panelView = computed<'lyrics' | 'visualizer'>(() =>
  !isPortrait.value && upperView.value === 'visualizer' ? 'visualizer' : 'lyrics',
)

// 按钮图标/提示都指向"按下去会看到的那一块"。不能直接拿 upperView 的名字取图标：横屏左栏恒为封面，
// 右栏在 'cover' 与 'lyrics' 两种状态下渲染的都是歌词（见 panelView），所以横屏的 'cover' 对用户来说
// 就是"歌词"，图标必须给 lyrics。这套图标桌面端与手机共用，改动会同时影响桌面端。
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

// 点封面：竖屏下翻到歌词，横屏/桌面仍进入沉浸式封面。
// 竖屏的封面本身已经占满整个宽度，"再叠一层全屏封面"是冗余的，
// 而"点封面看歌词"才是手机上最常用的动作。
const handleAlbumArtClick = (): void => {
  // 手机上长按封面会弹出菜单，手指抬起后浏览器仍会补一次 click。
  // 不吞掉的话菜单一出现就立刻被这次 click 切成歌词，等于按不出来。
  if (consumeLongPressClick()) return
  if (isPortrait.value) {
    upperView.value = 'lyrics'
    return
  }
  openImmersiveCover()
}

// 提取封面：桌面端是悬停按钮，手机端是长按菜单里的一项。
// 手机上"点了没反应"是最糟的反馈，所以成功给一条提示，
// 失败由 useAlbumArtInteraction 内部经 errorHandler 弹出具体原因。
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

// 切换上部面板后补一次 resize 广播：歌词组件靠 window resize 把当前行重新滚到中间，而竖屏下它可能刚
// 经历 display:none（隐藏期间设置的 scrollTop 不生效，必须重算）；波形画布同样只在 resize 时重新测量，
// 从隐藏恢复时要重测尺寸。
watch(upperView, () => {
  void nextTick(() => window.dispatchEvent(new Event('resize')))
})

// 转成竖屏时要收拾两件横屏留下的状态：一是退出沉浸式封面，竖屏没有"点左半区退出"的那块空白，沉浸层的
// 样式与单面板布局会互相打架（封面会被压成 0 宽而不可见）；二是若正停在波形态就归位到封面，竖屏没有
// 波形这一态。
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
// 状态栏在横屏或沉浸封面时收起，与界面控制栏走同一套判据
watch(
  [isAndroid, isLandscape, immersiveCover],
  ([android, landscape, immersive]) => {
    if (!android) return
    void setSystemUiHidden(landscape || immersive, true)
  },
  { immediate: true },
)

// 竖屏：点歌词面板的空白处就回封面（事件由 LyricsDisplay 判定后发出）。
// 横屏下这个事件不应有副作用（左栏恒为封面，语义上无从"返回"）。
const handleLyricsBlankClick = (): void => {
  if (isPortrait.value) {
    upperView.value = 'cover'
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
