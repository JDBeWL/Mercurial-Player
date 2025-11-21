<template>
  <Transition name="slide-down">
    <div v-if="configStore.ui.showConfigPanel" class="settings-modal">
      <div class="settings-content">
        <div class="settings-header">
          <h2>{{ $t('config.title') }}</h2>
          <button class="icon-button" @click="configStore.closeConfigPanel">
            <span class="material-symbols-rounded">close</span>
          </button>
        </div>
        <div class="settings-body">
          <div class="tabs">
            <button 
              @click="activeTab = 'folders'" 
              :class="{ active: activeTab === 'folders' }"
            >
              {{ $t('config.musicFolders') }}
            </button>
            <button 
              @click="activeTab = 'general'" 
              :class="{ active: activeTab === 'general' }"
            >
              {{ $t('config.generalSettings') }}
            </button>
            <button 
              @click="activeTab = 'titleExtraction'" 
              :class="{ active: activeTab === 'titleExtraction' }"
            >
              {{ $t('config.titleExtraction') }}
            </button>
            <button 
              @click="activeTab = 'playlist'" 
              :class="{ active: activeTab === 'playlist' }"
            >
              {{ $t('config.playlistSettings') }}
            </button>
            <button 
              @click="activeTab = 'audioDevice'" 
              :class="{ active: activeTab === 'audioDevice' }"
            >
              {{ $t('config.audioDeviceSettings') }}
            </button>
          </div>
          
          <!-- 音乐文件夹设置 -->
          <div v-if="activeTab === 'folders'" class="tab-content">
            <div class="tab-header">
              <h3>{{ $t('config.musicFolders') }}</h3>
              <button class="add-folder-btn" @click="addFolder">
                <span class="material-symbols-rounded">add</span>
                {{ $t('config.addFolder') }}
              </button>
            </div>
            <div v-if="musicDirectories.length === 0" class="empty-state">
              <p>{{ $t('config.noMusicFolders') }}</p>
            </div>
            <ul v-else>
              <li v-for="(folder, index) in musicDirectories" :key="index" class="folder-item">
                <span class="folder-path">{{ folder }}</span>
                <button class="icon-button danger" @click="removeFolder(index)" :title="$t('config.remove')">
                  <span class="material-symbols-rounded">delete</span>
                </button>
              </li>
            </ul>
          </div>
          
          <!-- 通用设置 -->
          <div v-if="activeTab === 'general'" class="tab-content">
            <h3>{{ $t('config.generalSettings') }}</h3>
            <div class="settings-group">
              <label>
                <input type="checkbox" v-model="configStore.general.startupLoadLastConfig" @change="saveConfig">
                {{ $t('config.startupLoadLastConfig') }}
              </label>
              <label>
                <input type="checkbox" v-model="configStore.general.autoSaveConfig" @change="saveConfig">
                {{ $t('config.autoSaveConfig') }}
              </label>
              <label>
                <input type="checkbox" v-model="configStore.general.showAudioInfo" @change="saveConfig">
                {{ $t('config.showAudioInfo') }}
              </label>
              <div class="select-field">
                <label for="language">{{ $t('config.language') }}:</label>
                <select id="language" v-model="configStore.general.language" @change="handleLanguageChange">
                  <option value="zh">中文</option>
                  <option value="en">English</option>
                </select>
              </div>
              <div class="select-field">
                <label for="lyricsAlignment">{{ $t('config.lyricsAlignment') }}:</label>
                <select id="lyricsAlignment" v-model="configStore.general.lyricsAlignment" @change="saveConfig">
                  <option value="left">{{ $t('config.alignLeft') }}</option>
                  <option value="center">{{ $t('config.alignCenter') }}</option>
                  <option value="right">{{ $t('config.alignRight') }}</option>
                </select>
              </div>
              <div class="select-field">
                <label for="lyricsFontFamily">{{ $t('config.lyricsFontFamily') }}:</label>
                <select id="lyricsFontFamily" v-model="configStore.general.lyricsFontFamily" @change="saveConfig">
                  <option v-for="font in systemFonts" :key="font" :value="font">{{ font }}</option>
                </select>
              </div>
              <div class="select-field">
                <label for="lyricsStyle">{{ $t('config.lyricsStyle') }}:</label>
                <select id="lyricsStyle" v-model="configStore.general.lyricsStyle" @change="saveConfig">
                  <option value="modern">{{ $t('config.lyricsStyleModern') }}</option>
                  <option value="classic">{{ $t('config.lyricsStyleClassic') }}</option>
                </select>
              </div>
            </div>
          </div>
          
          <!-- 标题提取设置 -->
          <div v-if="activeTab === 'titleExtraction'" class="tab-content">
            <h3>{{ $t('config.titleExtraction') }}</h3>
            <div class="settings-group">
              <label>
                <input type="checkbox" v-model="configStore.titleExtraction.preferMetadata" @change="saveConfig">
                {{ $t('config.preferMetadata') }}
              </label>
              <label>
                <input type="checkbox" v-model="configStore.titleExtraction.hideFileExtension" @change="saveConfig">
                {{ $t('config.hideFileExtension') }}
              </label>
              <label>
                <input type="checkbox" v-model="configStore.titleExtraction.parseArtistTitle" @change="saveConfig">
                {{ $t('config.parseArtistTitle') }}
              </label>
              <div class="select-field">
                <label for="separator">{{ $t('config.separator') }}:</label>
                <select id="separator" v-model="configStore.titleExtraction.separator" @change="saveConfig">
                  <option v-for="sep in configStore.validSeparators" :key="sep" :value="sep">{{ sep }}</option>
                </select>
              </div>
            </div>
          </div>
          
          <!-- 播放列表设置 -->
          <div v-if="activeTab === 'playlist'" class="tab-content">
            <h3>{{ $t('config.playlistSettings') }}</h3>
            <div class="settings-group">
              <label>
                <input type="checkbox" v-model="configStore.playlist.generateAllSongsPlaylist" @change="saveConfig">
                {{ $t('config.generateAllSongsPlaylist') }}
              </label>
              <label>
                <input type="checkbox" v-model="configStore.playlist.folderBasedPlaylists" @change="saveConfig">
                {{ $t('config.folderBasedPlaylists') }}
              </label>
              <div class="select-field">
                <label for="sortOrder">{{ $t('config.sortOrder') }}:</label>
                <select id="sortOrder" v-model="configStore.playlist.sortOrder" @change="saveConfig">
                  <option value="asc">A-Z (升序)</option>
                  <option value="desc">Z-A (降序)</option>
                </select>
              </div>
              <div class="input-field">
                <label for="playlistNameFormat">{{ $t('config.playlistNameFormat') }}:</label>
                <input 
                  id="playlistNameFormat" 
                  type="text" 
                  v-model="configStore.playlist.playlistNameFormat" 
                  @change="saveConfig"
                  placeholder="{folderName}"
                >
              </div>
            </div>
          </div>
          
          <!-- 音频设备设置 -->
          <div v-if="activeTab === 'audioDevice'" class="tab-content">
            <AudioDeviceSettings />
          </div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script setup>
import { ref, computed, onMounted } from 'vue';
import { useConfigStore } from '../stores/config';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { setLocale } from '../i18n';
import { useI18n } from 'vue-i18n';
import AudioDeviceSettings from './AudioDeviceSettings.vue';

const configStore = useConfigStore();
const activeTab = ref('folders');
const { locale } = useI18n();
const systemFonts = ref(['system-ui', 'sans-serif', 'serif', 'monospace']);

// 获取系统字体列表
const loadSystemFonts = async () => {
  try {
    const fonts = await invoke('get_system_fonts');
    // 不再额外添加system-ui，因为后端已经包含了
    systemFonts.value = ['sans-serif', 'serif', 'monospace', ...fonts];
  } catch (error) {
    console.error('Failed to load system fonts:', error);
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
      // 通知音乐库存储也更新其状态
      const { useMusicLibraryStore } = await import('../stores/musicLibrary');
      const musicLibraryStore = useMusicLibraryStore();
      musicLibraryStore.musicFolders = result;
      
      // 如果是首次添加音乐库，则刷新音乐库界面
      if (musicDirectories.value.length === 0) {
        // 延迟刷新确保DOM更新完成
        setTimeout(async () => {
          await musicLibraryStore.refreshMusicFolders();
        }, 100);
      }
    }
  } catch (error) {
    console.error('Failed to add folder:', error);
  }
};

const removeFolder = async (index) => {
  try {
    const pathToRemove = musicDirectories.value[index];
    const result = await invoke('remove_music_directory', { path: pathToRemove });
    configStore.musicDirectories = result;
    // 通知音乐库存储也更新其状态
    const { useMusicLibraryStore } = await import('../stores/musicLibrary');
    const musicLibraryStore = useMusicLibraryStore();
    musicLibraryStore.musicFolders = result;
  } catch (error) {
    console.error('Failed to remove folder:', error);
  }
};

// 保存配置
const saveConfig = async () => {
  try {
    // 对于手动操作，立即保存配置
    await configStore.saveConfigNow();
  } catch (error) {
    console.error('Failed to save config:', error);
  }
};

// 处理语言切换
const handleLanguageChange = async () => {
  try {
    // 设置新的语言
    setLocale(configStore.general.language);
    // 保存配置
    await configStore.saveConfigNow();
  } catch (error) {
    console.error('Failed to change language:', error);
  }
};

// 初始化组件时加载系统字体
onMounted(() => {
  loadSystemFonts();
});
</script>

<style scoped>
.settings-modal {
  position: fixed;
  top: 0;
  left: 0;
  width: 100%;
  height: 100%;
  background-color: rgba(0, 0, 0, 0.5);
  display: flex;
  justify-content: center;
  align-items: center;
  z-index: 1000;
  backdrop-filter: blur(4px);
}

.settings-content {
  background-color: var(--md-sys-color-surface);
  padding: 24px;
  border-radius: var(--md-sys-shape-corner-large);
  width: 80%;
  max-width: 700px;
  max-height: 80vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  box-shadow: var(--md-sys-elevation-level4);
}

.settings-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.settings-header h2 {
  margin: 0;
  font-size: 24px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.settings-body {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.tabs {
  display: flex;
  margin-bottom: 16px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
  flex-shrink: 0;
}

.tabs button {
  padding: 12px 16px;
  margin-right: 8px;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.tabs button.active {
  border-bottom-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-surface);
}

.tab-content {
  flex: 1;
  overflow-y: auto;
  padding-right: 8px;
}

.tab-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.tab-content h3 {
  margin: 0;
  font-size: 20px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.empty-state {
  text-align: center;
  padding: 32px;
  color: var(--md-sys-color-on-surface-variant);
}

ul {
  list-style: none;
  padding: 0;
  margin: 0;
}

.folder-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  margin-bottom: 8px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: var(--md-sys-shape-corner-small);
}

.folder-path {
  flex: 1;
  margin-right: 12px;
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
  word-break: break-all;
}

.add-folder-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  border: none;
  border-radius: var(--md-sys-shape-corner-small);
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: background-color 0.2s ease;
}

.add-folder-btn:hover {
  background-color: var(--md-sys-color-outline);
  color: var(--md-sys-color-on-primary-container);
}

.settings-group {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.settings-group label {
  display: flex;
  align-items: center;
  gap: 12px;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  cursor: pointer;
}

.settings-group input[type="checkbox"] {
  width: 18px;
  height: 18px;
  accent-color: var(--md-sys-color-primary);
}

.select-field, .input-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.select-field label, .input-field label {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.select-field select, .input-field input {
  padding: 12px 16px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
}

.select-field select:focus, .input-field input:focus {
  outline: none;
  border-color: var(--md-sys-color-primary);
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border: none;
  border-radius: var(--md-sys-shape-corner-small);
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface);
  transition: all 0.2s ease;
}

.icon-button:hover {
  background-color: var(--md-sys-color-surface-variant);
}

.icon-button.danger {
  color: var(--md-sys-color-error);
}

.icon-button.danger:hover {
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

.material-symbols-rounded {
  font-size: 20px;
}

/* 过渡动画 */
.slide-down-enter-active,
.slide-down-leave-active {
  transition: opacity 0.2s ease, transform 0.3s cubic-bezier(0.25, 0.46, 0.45, 0.94);
}

.slide-down-enter-from {
  opacity: 0;
  transform: translateY(-32px);
}

.slide-down-leave-to {
  opacity: 0;
  transform: translateY(-32px);
}
</style>
