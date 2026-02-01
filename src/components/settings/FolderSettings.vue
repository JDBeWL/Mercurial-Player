<template>
  <div class="tab-content">
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
</template>

<script setup>
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import { useMusicLibraryStore } from '../../stores/musicLibrary'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import logger from '../../utils/logger'
import { useErrorNotification } from '../../composables/useErrorNotification'

const configStore = useConfigStore()
const musicLibraryStore = useMusicLibraryStore()
const musicDirectories = computed(() => configStore.musicDirectories)
const { showError } = useErrorNotification()

const addFolder = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
    })
    if (selected && !musicDirectories.value.includes(selected)) {
      const result = await invoke('add_music_directory', { path: selected })
      configStore.musicDirectories = result
      musicLibraryStore.musicFolders = result
      
      if (musicDirectories.value.length === 0) {
        setTimeout(async () => {
          await musicLibraryStore.refreshMusicFolders()
        }, 100)
      }
    }
  } catch (error) {
    logger.error('Failed to add folder:', error)
    showError(String(error), 'warning')
  }
}

const removeFolder = async (index) => {
  try {
    const pathToRemove = musicDirectories.value[index]
    const result = await invoke('remove_music_directory', { path: pathToRemove })
    configStore.musicDirectories = result
    musicLibraryStore.musicFolders = result
  } catch (error) {
    logger.error('Failed to remove folder:', error)
  }
}
</script>

<style scoped>
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

.filled-tonal-button .material-symbols-rounded {
  font-size: 20px;
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
</style>
