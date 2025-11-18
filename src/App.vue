<template>
  <div class="app-container" :data-fullscreen="isFullscreen" :data-maximized="isMaximized">
    <header class="nav-bar" data-tauri-drag-region>
      <!-- 左侧控制区 -->
      <div class="nav-left">
        <button class="icon-button" data-tauri-drag-region="false" @click="toggleLibrary">
          <span class="material-symbols-rounded">menu</span>
        </button>
        <button class="icon-button" data-tauri-drag-region="false" @click="toggleSettings" title="设置">
          <span class="material-symbols-rounded">settings</span>
        </button>
        <button class="icon-button" data-tauri-drag-region="false" @click="themeStore.toggleDarkMode" title="切换主题">
          <span class="material-symbols-rounded">{{ themeStore.isDarkMode ? 'light_mode' : 'dark_mode' }}</span>
        </button>
        <ThemeSelector data-tauri-drag-region="false" />
      </div>
      <!-- 右侧控制区 -->
      <div class="nav-right">
        <button class="icon-button" data-tauri-drag-region="false" @click="minimizeWindow" title="最小化">
          <span class="material-symbols-rounded">minimize</span>
        </button>
        <button class="icon-button" data-tauri-drag-region="false" @click="toggleFullscreen"
          :title="isFullscreen ? '退出全屏' : '全屏'">
          <span class="material-symbols-rounded">{{ isFullscreen ? 'fullscreen_exit' : 'fullscreen' }}</span>
        </button>
        <button class="icon-button" data-tauri-drag-region="false" @click="closeWindow" title="关闭">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
    </header>

    <main class="main-content">
      <Transition name="slide-left">
        <MusicLibrary v-if="showLibrary" @close="showLibrary = false" />
      </Transition>

      <div class="player-container">
        <div class="player-main">
          <!-- 上方区域：左侧专辑封面和歌曲信息，右侧歌词 -->
          <div class="player-upper">
            <!-- 左侧：专辑封面和歌曲信息 -->
            <div class="player-left">
              <div class="album-art-container">
                <Transition :name="transitionDirection === 'next' ? 'album-art-slide-next' : 'album-art-slide-prev'"
                  mode="out-in">
                  <div :key="currentTrack ? currentTrack.path : 'no-track'" class="album-art-wrapper">
                    <div class="album-art" :style="{ backgroundImage: currentTrackCover }">
                      <div v-if="!currentTrack || !currentTrack.cover" class="album-art-placeholder">
                        <span class="material-symbols-rounded">music_note</span>
                      </div>
                    </div>
                  </div>
                </Transition>
              </div>

              <Transition name="fade" mode="out-in">
                <div :key="currentTrack ? currentTrack.path : 'no-track-info'">
                  <div class="track-info" v-if="currentTrack">
                    <h2 
                      class="track-title" 
                      :title="getTrackTitle(currentTrack) || $t('player.noTrack')"
                    >{{ getTrackTitle(currentTrack) || $t('player.noTrack') }}</h2>
                    <div 
                      class="track-artist" 
                      v-if="getTrackArtist(currentTrack)" 
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
                  <div class="track-info-placeholder" v-else>
                    <h2 class="track-title">{{ $t('player.noTrack') }}</h2>
                    <div class="track-artist">&nbsp;</div>
                  </div>

                  <div class="audio-info"
                    v-if="currentTrack && formattedAudioInfo && configStore.general.showAudioInfo">
                    <span class="text-caption">{{ formattedAudioInfo }}</span>
                  </div>
                  <div class="audio-info-placeholder" v-else>
                    <span class="text-caption">&nbsp;</span>
                  </div>
                </div>
              </Transition>
            </div>

            <!-- 右侧：歌词 -->
            <div class="player-right">
              <LyricsDisplay class="lyrics-container" />
            </div>
          </div>

          <!-- 下方区域：进度条和控制按钮 -->
          <div class="player-lower">
            <ProgressBar />
            <PlayerControls />
          </div>
        </div>
      </div>
    </main>

    <Transition name="slide-right">
      <PlaylistView v-if="showPlaylist" @close="showPlaylist = false" />
    </Transition>

    <!-- 配置面板 -->
    <Settings />

    <button class="fab" @click="togglePlaylist" v-if="playlist.length > 0">
      <span class="material-symbols-rounded">playlist_play</span>
    </button>
  </div>
</template>

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
  transition: transform 0.3s cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

.slide-right-enter-from {
  transform: translateX(100%);
}

.slide-right-leave-to {
  transform: translateX(100%);
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
</style>

<script setup>
import { computed, ref, onMounted, onUnmounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { usePlayerStore } from './stores/player'
import { useThemeStore } from './stores/theme'
import { useConfigStore } from './stores/config'
import { getCurrentWindow } from '@tauri-apps/api/window'
import PlayerControls from './components/PlayerControls.vue'
import ProgressBar from './components/ProgressBar.vue'
import LyricsDisplay from './components/LyricsDisplay.vue'
import MusicLibrary from './components/MusicLibrary.vue'
import PlaylistView from './components/PlaylistView.vue'
import ThemeSelector from './components/ThemeSelector.vue'
import Settings from './components/Settings.vue'
import FileUtils from './utils/fileUtils'
import { TitleExtractor } from './utils/titleExtractor'

const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const configStore = useConfigStore()

// 获取当前窗口实例
const appWindow = getCurrentWindow()

const { currentTrack, playlist, audioInfo, currentTrackIndex, lastTrackIndex } = storeToRefs(playerStore)

const showLibrary = ref(false)
const showPlaylist = ref(false)
const isFullscreen = ref(false)
const isMaximized = ref(false)

// 存储处理后的音轨信息
const processedTracks = ref({})

// 智能获取音轨标题 - 优化显示流程
const getTrackTitle = (track) => {
  if (!track || !track.path) {
    return ''
  }

  const trackPath = track.path

  // 如果已经处理过该音轨，直接返回结果
  if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
    return processedTracks.value[trackPath].title
  }
  
  // 异步处理音轨信息，但不阻塞当前渲染
  if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
    processTrackInfo(trackPath)
  }

  // 如果还没处理完，暂时返回track中已有的name或文件名
  // 这样可以避免闪烁，因为track中的name可能已经是处理过的
  return track.name || FileUtils.getFileName(trackPath)
}

// 智能获取音轨艺术家 - 优化显示流程
const getTrackArtist = (track) => {
  if (!track || !track.path) {
    return ''
  }

  const trackPath = track.path

  // 如果已经处理过该音轨，直接返回结果
  if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
    return processedTracks.value[trackPath].artist
  }

  // 异步处理音轨信息，但不阻塞当前渲染
  if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
    processTrackInfo(trackPath)
  }

  // 如果还没处理完，暂时返回track中已有的artist信息
  return track.artist || ''
}

// 异步处理音轨信息
const processTrackInfo = async (trackPath) => {
  try {
    // 如果已经在处理中，跳过
    if (processedTracks.value[trackPath]?.processing) return

    // 标记为处理中
    processedTracks.value[trackPath] = { processing: true }

    // 获取配置
    const config = {
      preferMetadata: configStore.titleExtraction?.preferMetadata ?? true,
      hideFileExtension: configStore.titleExtraction?.hideFileExtension ?? true,
      parseArtistTitle: configStore.titleExtraction?.parseArtistTitle ?? true,
      separator: configStore.titleExtraction?.separator ?? '-',
      customSeparators: configStore.titleExtraction?.customSeparators ?? ['-', '_', '.', ' ']
    }

    // 使用 TitleExtractor 智能提取标题信息
    const titleInfo = await TitleExtractor.extractTitle(trackPath, config)

    // 更新处理结果
    processedTracks.value[trackPath] = {
      processing: false,
      ...titleInfo
    }

  } catch (error) {
    console.error('处理音轨信息失败:', trackPath, error)
    // 出错时使用文件名作为标题
    processedTracks.value[trackPath] = {
      processing: false,
      title: FileUtils.getFileName(trackPath),
      artist: '',
      fileName: FileUtils.getFileName(trackPath),
      isFromMetadata: false
    }
  }
}

const currentTrackCover = computed(() => {
  if (currentTrack.value && currentTrack.value.cover) {
    return `url('${currentTrack.value.cover}')`
  }
  return 'none' // 如果没有封面，返回none
})

const formattedAudioInfo = computed(() => {
  const { bitrate, sampleRate, channels } = audioInfo.value;

  const parts = [];
  if (bitrate && bitrate !== 'N/A') {
    parts.push(`${bitrate} kbps`);
  }
  if (sampleRate && sampleRate !== 'N/A') {
    parts.push(`${sampleRate} kHz`);
  }
  if (channels && channels !== 'N/A') {
    // 将 "2 channels" 显示为 "Stereo"
    if (channels == 2) {
      parts.push('Stereo');
    } else if (channels == 1) {
      parts.push('Mono');
    } else {
      parts.push(`${channels} channels`);
    }
  }

  return parts.join(' • ');
});

// 检查当前歌曲文件是否存在
const isTrackFileExists = computed(() => {
  if (!currentTrack.value) return true;

  // 由于文件存在性检查需要异步操作，这里先简单检查path是否存在
  // 实际的文件存在性检查在播放器状态管理中进行
  return !!currentTrack.value.path;
});

// 全局键盘事件处理
const handleKeyDown = (event) => {
  // 只在用户没有在输入框等元素中编辑时响应键盘事件
  const isInputFocused = document.activeElement.tagName === 'INPUT' ||
    document.activeElement.tagName === 'TEXTAREA' ||
    document.activeElement.isContentEditable;

  if (isInputFocused) return;

  // 空格键暂停/恢复播放
  if (event.code === 'Space') {
    event.preventDefault(); // 防止页面滚动
    playerStore.togglePlay();
  }

  // 方向键控制
  switch (event.code) {
    case 'ArrowLeft':
      // 左方向键：上一首
      event.preventDefault();
      if (playerStore.hasPreviousTrack) {
        playerStore.previousTrack();
      }
      break;
    case 'ArrowRight':
      // 右方向键：下一首
      event.preventDefault();
      if (playerStore.hasNextTrack) {
        playerStore.nextTrack();
      }
      break;
    case 'ArrowUp':
      // 上方向键：音量增加
      event.preventDefault();
      const newVolumeUp = Math.min(1, playerStore.volume + 0.05);
      playerStore.setVolume(newVolumeUp);
      break;
    case 'ArrowDown':
      // 下方向键：音量减少
      event.preventDefault();
      const newVolumeDown = Math.max(0, playerStore.volume - 0.05);
      playerStore.setVolume(newVolumeDown);
      break;
  }
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

const minimizeWindow = async () => {
  try {
    await appWindow.minimize()
  } catch (error) {
    console.error('Failed to minimize window:', error)
  }
}

const toggleFullscreen = async () => {
  try {
    if (isFullscreen.value) {
      // 退出全屏
      await appWindow.setFullscreen(false)
      isFullscreen.value = false
    } else {
      // 进入全屏前先检查并取消最大化状态
      const isMaximized = await appWindow.isMaximized()
      if (isMaximized) {
        await appWindow.unmaximize()
      }
      await appWindow.setFullscreen(true)
      isFullscreen.value = true
    }
  } catch (error) {
    console.error('Failed to toggle fullscreen:', error)
  }
}

const closeWindow = async () => {
  try {
    await appWindow.close()
  } catch (error) {
    console.error('Failed to close window:', error)
  }
}



// 监听当前音轨变化，自动处理标题信息
watch(currentTrack, (newTrack) => {
  if (newTrack && newTrack.path) {
    processTrackInfo(newTrack.path)
  }
}, { immediate: true })

const transitionDirection = ref(null)

// 监听当前音轨索引变化，自动处理标题信息
watch(currentTrackIndex, (newIndex, oldIndex) => {
  if (oldIndex === -1 || newIndex === -1) {
    transitionDirection.value = null;
    return;
  }

  // 如果播放列表为空，则不进行播放列表循环处理
  const playlistLength = playlist.value.length;
  if (playlistLength === 0) {
    transitionDirection.value = null;
    return;
  }

  if (newIndex === (oldIndex + 1) % playlistLength) {
    transitionDirection.value = 'next';
  } else if (newIndex === (oldIndex - 1 + playlistLength) % playlistLength) {
    transitionDirection.value = 'prev';
  } else {
    transitionDirection.value = null; // 其他情况不进行处理
  }
});

onMounted(async () => {
  // 加载配置文件
  try {
    await configStore.loadConfig()
  } catch (error) {
    console.warn('Failed to load configuration:', error)
  }

  // 设置语言
  try {
    const { setLocale } = await import('./i18n')
    setLocale(configStore.general.language || 'zh')
  } catch (error) {
    console.error('Failed to apply language from config:', error)
  }

  // 应用主题
  themeStore.applyTheme()

  // 初始化音频播放器
  playerStore.initAudio()

  // 检查当前窗口是否处于全屏状态
  try {
    isFullscreen.value = await appWindow.isFullscreen()
    isMaximized.value = await appWindow.isMaximized()
  } catch (error) {
    console.error('Failed to check window state:', error)
  }

  // 添加全局键盘事件监听器
  document.addEventListener('keydown', handleKeyDown)
})

onUnmounted(() => {
  // 清理键盘事件监听器
  document.removeEventListener('keydown', handleKeyDown)
})
</script>

<style scoped>
.app-container {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: 12px;
  transition: border-radius 0.3s ease;
}

.main-content {
  flex: 1;
  display: flex;
  overflow: hidden;
  padding: 0.5% 6%;
  background-color: var(--md-sys-color-surface-container-low);
}

.player-container {
  flex: 1;
  display: flex;
  padding: 16px;
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
  gap: 5vw;
  margin-bottom: 16px;
  min-height: 0;
}

/* 下方区域：进度条和控制按钮 */
.player-lower {
  display: flex;
  flex-direction: column;
  gap: 0px;
}

/* 左侧：专辑封面和歌曲信息 */
.player-left {
  flex: 0 0 min(200px, 40vw);
  margin-left: 2vw;
  max-width: 400px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
}

/* 右侧：歌词 */
.player-right {
  flex: 1;
  min-width: 0;
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
    max-height: 3.9em;
  }

  .track-artist {
    font-size: 16px;
    /* 允许最多2行 */
    -webkit-line-clamp: 2;
    max-height: 2.8em;
  }
}

.album-art-container {
  width: min(400px, 45vh);
  height: min(400px, 45vh);
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
.app-container[data-fullscreen="true"],
.app-container[data-maximized="true"] {
  border-radius: 0;
}
</style>