<template>
  <div class="settings-panel">
    <!-- 左侧导航 -->
    <nav class="settings-nav">
      <div class="nav-header">
        <h2>{{ $t('config.title') }}</h2>
        <button class="icon-button" @click="configStore.closeConfigPanel" :title="$t('common.close')">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
      
      <div class="nav-items">
        <button 
          v-for="tab in tabs" 
          :key="tab.id"
          class="nav-item"
          :class="{ active: activeTab === tab.id }"
          @click="activeTab = tab.id"
        >
          <span class="material-symbols-rounded">{{ tab.icon }}</span>
          <span class="nav-label">{{ $t(tab.label) }}</span>
        </button>
      </div>
    </nav>

    <!-- 右侧内容区 -->
    <div class="settings-content">
      <!-- 音乐文件夹设置 -->
      <div v-if="activeTab === 'folders'" class="tab-content">
        <div class="content-header">
          <h3>{{ $t('config.musicFolders') }}</h3>
          <button class="filled-tonal-button" @click="addFolder">
            <span class="material-symbols-rounded">add</span>
            {{ $t('config.addFolder') }}
          </button>
        </div>
        
        <div v-if="musicDirectories.length === 0" class="empty-state">
          <span class="material-symbols-rounded">folder_open</span>
          <p>{{ $t('config.noMusicFolders') }}</p>
        </div>
        
        <div v-else class="folder-list">
          <div v-for="(folder, index) in musicDirectories" :key="index" class="folder-item">
            <span class="material-symbols-rounded folder-icon">folder</span>
            <span class="folder-path">{{ folder }}</span>
            <button class="icon-button danger" @click="removeFolder(index)" :title="$t('config.remove')">
              <span class="material-symbols-rounded">delete</span>
            </button>
          </div>
        </div>
      </div>
      
      <!-- 通用设置 -->
      <div v-if="activeTab === 'general'" class="tab-content">
        <div class="content-header">
          <h3>{{ $t('config.generalSettings') }}</h3>
        </div>
        
        <div class="settings-section">
          <div class="setting-item" @click="toggleSetting('startupLoadLastConfig')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.startupLoadLastConfig') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.general.startupLoadLastConfig }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleSetting('autoSaveConfig')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.autoSaveConfig') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.general.autoSaveConfig }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleSetting('showAudioInfo')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.showAudioInfo') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.general.showAudioInfo }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
        </div>
        
        <div class="settings-section">
          <h4 class="section-title">{{ $t('config.display') || '显示' }}</h4>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.language') }}</span>
            </div>
            <select v-model="configStore.general.language" @change="handleLanguageChange" class="md3-select">
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </div>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.lyricsAlignment') }}</span>
            </div>
            <select v-model="configStore.general.lyricsAlignment" @change="saveConfig" class="md3-select">
              <option value="left">{{ $t('config.alignLeft') }}</option>
              <option value="center">{{ $t('config.alignCenter') }}</option>
              <option value="right">{{ $t('config.alignRight') }}</option>
            </select>
          </div>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.lyricsFontFamily') }}</span>
            </div>
            <select v-model="configStore.general.lyricsFontFamily" @change="saveConfig" class="md3-select">
              <option v-for="font in systemFonts" :key="font" :value="font">{{ font }}</option>
            </select>
          </div>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.lyricsStyle') }}</span>
            </div>
            <select v-model="configStore.general.lyricsStyle" @change="saveConfig" class="md3-select">
              <option value="modern">{{ $t('config.lyricsStyleModern') }}</option>
              <option value="classic">{{ $t('config.lyricsStyleClassic') }}</option>
            </select>
          </div>
        </div>
      </div>
      
      <!-- 标题提取设置 -->
      <div v-if="activeTab === 'titleExtraction'" class="tab-content">
        <div class="content-header">
          <h3>{{ $t('config.titleExtraction') }}</h3>
        </div>
        
        <div class="settings-section">
          <div class="setting-item" @click="toggleTitleSetting('preferMetadata')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.preferMetadata') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.titleExtraction.preferMetadata }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleTitleSetting('hideFileExtension')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.hideFileExtension') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.titleExtraction.hideFileExtension }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleTitleSetting('parseArtistTitle')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.parseArtistTitle') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.titleExtraction.parseArtistTitle }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.separator') }}</span>
            </div>
            <select v-model="configStore.titleExtraction.separator" @change="saveConfig" class="md3-select">
              <option v-for="sep in configStore.validSeparators" :key="sep" :value="sep">{{ sep }}</option>
            </select>
          </div>
        </div>
      </div>
      
      <!-- 歌词设置 -->
      <div v-if="activeTab === 'lyrics'" class="tab-content">
        <div class="content-header">
          <h3>{{ $t('config.lyricsSettings') }}</h3>
        </div>
        
        <div class="settings-section">
          <h4 class="section-title">{{ $t('config.onlineLyrics') || '在线歌词' }}</h4>
          
          <div class="setting-item" @click="toggleLyricsSetting('enableOnlineFetch')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.enableOnlineFetch') }}</span>
              <span class="setting-description">{{ $t('config.enableOnlineFetchDesc') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.lyrics?.enableOnlineFetch }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleLyricsSetting('autoSaveOnlineLyrics')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.autoSaveOnlineLyrics') }}</span>
              <span class="setting-description">{{ $t('config.autoSaveOnlineLyricsDesc') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.lyrics?.autoSaveOnlineLyrics }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="toggleLyricsSetting('preferTranslation')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.preferTranslation') }}</span>
              <span class="setting-description">{{ $t('config.preferTranslationDesc') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.lyrics?.preferTranslation }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
        </div>
      </div>
      
      <!-- 播放列表设置 -->
      <div v-if="activeTab === 'playlist'" class="tab-content">
        <div class="content-header">
          <h3>{{ $t('config.playlistSettings') }}</h3>
        </div>
        
        <div class="settings-section">
          <div class="setting-item" @click="togglePlaylistSetting('generateAllSongsPlaylist')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.generateAllSongsPlaylist') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.playlist.generateAllSongsPlaylist }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item" @click="togglePlaylistSetting('folderBasedPlaylists')">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.folderBasedPlaylists') }}</span>
            </div>
            <div class="switch" :class="{ active: configStore.playlist.folderBasedPlaylists }">
              <div class="switch-track"></div>
              <div class="switch-handle"></div>
            </div>
          </div>
          
          <div class="setting-item select">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.sortOrder') }}</span>
            </div>
            <select v-model="configStore.playlist.sortOrder" @change="saveConfig" class="md3-select">
              <option value="asc">A-Z (升序)</option>
              <option value="desc">Z-A (降序)</option>
            </select>
          </div>
          
          <div class="setting-item input">
            <div class="setting-info">
              <span class="setting-label">{{ $t('config.playlistNameFormat') }}</span>
            </div>
            <input 
              type="text" 
              v-model="configStore.playlist.playlistNameFormat" 
              @change="saveConfig"
              placeholder="{folderName}"
              class="md3-input"
            >
          </div>
        </div>
      </div>
      
      <!-- 音频设备设置 -->
      <div v-if="activeTab === 'audioDevice'" class="tab-content">
        <AudioDeviceSettings />
      </div>
      
      <!-- EQ 均衡器设置 -->
      <div v-if="activeTab === 'equalizer'" class="tab-content">
        <EqualizerSettings />
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue';
import { useConfigStore } from '../stores/config';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { setLocale } from '../i18n';
import { useI18n } from 'vue-i18n';
import AudioDeviceSettings from './AudioDeviceSettings.vue';
import EqualizerSettings from './EqualizerSettings.vue';
import logger from '../utils/logger';

const configStore = useConfigStore();
const activeTab = ref('folders');
const { locale } = useI18n();
const systemFonts = ref(['system-ui', 'sans-serif', 'serif', 'monospace']);

// 导航标签配置
const tabs = [
  { id: 'folders', icon: 'folder', label: 'config.musicFolders' },
  { id: 'general', icon: 'settings', label: 'config.generalSettings' },
  { id: 'lyrics', icon: 'lyrics', label: 'config.lyricsSettings' },
  { id: 'titleExtraction', icon: 'title', label: 'config.titleExtraction' },
  { id: 'playlist', icon: 'queue_music', label: 'config.playlistSettings' },
  { id: 'audioDevice', icon: 'speaker', label: 'config.audioDeviceSettings' },
  { id: 'equalizer', icon: 'equalizer', label: 'config.equalizer' },
];

// 获取系统字体列表
const loadSystemFonts = async () => {
  try {
    const fonts = await invoke('get_system_fonts');
    systemFonts.value = ['sans-serif', 'serif', 'monospace', ...fonts];
  } catch (error) {
    logger.error('Failed to load system fonts:', error);
  }
};

// 音乐文件夹相关
const musicDirectories = computed(() => configStore.musicDirectories);

const addFolder = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (selected && !musicDirectories.value.includes(selected)) {
      const result = await invoke('add_music_directory', { path: selected });
      configStore.musicDirectories = result;
      const { useMusicLibraryStore } = await import('../stores/musicLibrary');
      const musicLibraryStore = useMusicLibraryStore();
      musicLibraryStore.musicFolders = result;
      
      if (musicDirectories.value.length === 0) {
        setTimeout(async () => {
          await musicLibraryStore.refreshMusicFolders();
        }, 100);
      }
    }
  } catch (error) {
    logger.error('Failed to add folder:', error);
  }
};

const removeFolder = async (index) => {
  try {
    const pathToRemove = musicDirectories.value[index];
    const result = await invoke('remove_music_directory', { path: pathToRemove });
    configStore.musicDirectories = result;
    const { useMusicLibraryStore } = await import('../stores/musicLibrary');
    const musicLibraryStore = useMusicLibraryStore();
    musicLibraryStore.musicFolders = result;
  } catch (error) {
    logger.error('Failed to remove folder:', error);
  }
};

// 保存配置
const saveConfig = async () => {
  try {
    await configStore.saveConfigNow();
  } catch (error) {
    logger.error('Failed to save config:', error);
  }
};

// 切换通用设置
const toggleSetting = async (key) => {
  configStore.general[key] = !configStore.general[key];
  await saveConfig();
};

// 切换标题提取设置
const toggleTitleSetting = async (key) => {
  configStore.titleExtraction[key] = !configStore.titleExtraction[key];
  await saveConfig();
};

// 切换播放列表设置
const togglePlaylistSetting = async (key) => {
  configStore.playlist[key] = !configStore.playlist[key];
  await saveConfig();
};

// 切换歌词设置
const toggleLyricsSetting = async (key) => {
  if (!configStore.lyrics) {
    configStore.lyrics = {
      enableOnlineFetch: false,
      autoSaveOnlineLyrics: true,
      preferTranslation: true,
      onlineSource: 'netease'
    };
  }
  configStore.lyrics[key] = !configStore.lyrics[key];
  await saveConfig();
};

// 处理语言切换
const handleLanguageChange = async () => {
  try {
    setLocale(configStore.general.language);
    await configStore.saveConfigNow();
  } catch (error) {
    logger.error('Failed to change language:', error);
  }
};

onMounted(() => {
  loadSystemFonts();
});
</script>

<style scoped>
.settings-panel {
  flex: 1;
  display: flex;
  overflow: hidden;
  background-color: var(--md-sys-color-surface-container-low);
}

/* 左侧导航 */
.settings-nav {
  width: 280px;
  min-width: 240px;
  background-color: var(--md-sys-color-surface);
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--md-sys-color-outline-variant);
}

.nav-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px 16px 24px;
}

.nav-header h2 {
  margin: 0;
  font-size: 22px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.nav-items {
  flex: 1;
  padding: 0 12px;
  overflow-y: auto;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 16px;
  margin-bottom: 4px;
  border: none;
  border-radius: 28px;
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  font-weight: 500;
  text-align: left;
  transition: all 0.2s ease;
}

.nav-item:hover {
  background-color: var(--md-sys-color-surface-container-highest);
}

.nav-item.active {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.nav-item .material-symbols-rounded {
  font-size: 24px;
}

.nav-label {
  flex: 1;
}

/* 右侧内容区 */
.settings-content {
  flex: 1;
  overflow-y: auto;
  padding: 24px 32px;
}

.tab-content {
  max-width: 720px;
}

.content-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

/* 设置分组 */
.settings-section {
  margin-bottom: 32px;
}

.section-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-primary);
  margin: 0 0 16px 16px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

/* 设置项 */
.setting-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  margin-bottom: 2px;
  border-radius: 12px;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.setting-item:hover {
  background-color: var(--md-sys-color-surface-container);
}

.setting-item.select,
.setting-item.input {
  cursor: default;
}

.setting-info {
  flex: 1;
  min-width: 0;
}

.setting-label {
  font-size: 16px;
  color: var(--md-sys-color-on-surface);
}

.setting-description {
  display: block;
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin-top: 4px;
}

/* MD3 Switch */
.switch {
  position: relative;
  width: 52px;
  height: 32px;
  flex-shrink: 0;
}

.switch-track {
  position: absolute;
  width: 100%;
  height: 100%;
  background-color: var(--md-sys-color-surface-container-highest);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 16px;
  box-sizing: border-box;
  transition: all 0.2s ease;
}

.switch.active .switch-track {
  background-color: var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.switch-handle {
  position: absolute;
  top: 50%;
  left: 6px;
  transform: translateY(-50%);
  width: 16px;
  height: 16px;
  background-color: var(--md-sys-color-outline);
  border-radius: 50%;
  transition: all 0.2s ease;
}

.switch.active .switch-handle {
  left: 22px;
  width: 24px;
  height: 24px;
  background-color: var(--md-sys-color-on-primary);
}

/* MD3 Select */
.md3-select {
  min-width: 160px;
  padding: 12px 16px;
  padding-right: 40px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 8px;
  background-color: transparent;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='24' height='24' viewBox='0 0 24 24'%3E%3Cpath fill='%23666' d='M7 10l5 5 5-5z'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  transition: border-color 0.2s ease, outline 0.2s ease;
  outline: 1px solid transparent;
  outline-offset: -1px;
}

.md3-select:hover {
  border-color: var(--md-sys-color-on-surface);
}

.md3-select:focus {
  outline: 1px solid var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

/* MD3 Input */
.md3-input {
  min-width: 200px;
  padding: 12px 16px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 8px;
  background-color: transparent;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  transition: border-color 0.2s ease, outline 0.2s ease;
  outline: 1px solid transparent;
  outline-offset: -1px;
}

.md3-input:hover {
  border-color: var(--md-sys-color-on-surface);
}

.md3-input:focus {
  outline: 1px solid var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.md3-input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

/* 文件夹列表 */
.folder-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.folder-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
  transition: background-color 0.2s ease;
}

.folder-item:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.folder-icon {
  color: var(--md-sys-color-primary);
  font-size: 24px;
}

.folder-path {
  flex: 1;
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
  word-break: break-all;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.empty-state .material-symbols-rounded {
  font-size: 48px;
  margin-bottom: 16px;
  opacity: 0.6;
}

.empty-state p {
  margin: 0;
  font-size: 14px;
}

/* 按钮样式 */
.filled-tonal-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 24px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.filled-tonal-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-secondary-container));
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: var(--md-sys-shape-corner-large);
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  transition: all 0.2s ease;
}

.icon-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

.icon-button.danger {
  color: var(--md-sys-color-error);
}

.icon-button.danger:hover {
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

/* 响应式设计 */
@media (max-width: 768px) {
  .settings-panel {
    flex-direction: column;
  }
  
  .settings-nav {
    width: 100%;
    min-width: unset;
    border-right: none;
    border-bottom: 1px solid var(--md-sys-color-outline-variant);
  }
  
  .nav-items {
    display: flex;
    flex-wrap: nowrap;
    overflow-x: auto;
    padding: 8px 12px;
    gap: 8px;
  }
  
  .nav-item {
    flex-shrink: 0;
    padding: 12px 16px;
    margin-bottom: 0;
  }
  
  .nav-label {
    display: none;
  }
  
  .settings-content {
    padding: 16px;
  }
}
</style>
