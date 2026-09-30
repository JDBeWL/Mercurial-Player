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
      <div v-for="(folder, index) in musicDirectories" :key="folder" class="folder-item">
        <span class="material-symbols-rounded folder-icon">folder</span>
        <span class="folder-path" :title="folder">{{ displayFolderName(folder) }}</span>
        <button
          class="icon-button danger"
          :title="$t('config.remove')"
          @click="removeFolder(index)"
        >
          <span class="material-symbols-rounded">delete</span>
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useConfigStore } from '../../stores/config'
import { useMusicLibraryStore } from '../../stores/musicLibrary'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import { getPlatform } from '../../services/appService'
import logger from '../../utils/logger'
import { useErrorNotification } from '../../composables/useErrorNotification'

const configStore = useConfigStore()
const musicLibraryStore = useMusicLibraryStore()
const musicDirectories = computed(() => configStore.musicDirectories)
const { showError } = useErrorNotification()

/** 静默等待（ms） */
const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

const addFolder = async (): Promise<void> => {
  try {
    // Android：走 SAF 系统目录选择器（返回媒体权限树 URI），桌面走原生目录对话框
    const platform = await getPlatform()
    if (platform === 'android') {
      await addFolderAndroid()
      return
    }

    const selected = await open({
      directory: true,
      multiple: false,
    })
    if (typeof selected === 'string' && !musicDirectories.value.includes(selected)) {
      await commitDirectory(selected)
    }
  } catch (error) {
    logger.error('Failed to add folder:', error)
    showError(String(error), 'warning')
  }
}

/** SAF 授权状态快照（version 每次成功授权都会递增） */
interface SafPickState {
  uri?: string | null
  version: number
  displayName?: string | null
}

/**
 * Android：调起 SAF 选择器，轮询等待授权结果并保存树 URI
 *
 * 判定依据是 `version` 而非"URI 是否变化"：删掉目录后再授权**同一个**目录时
 * URI 完全相同，比较 URI 会永远等不到结果（表现为"怎么都添加不上"）。
 */
const addFolderAndroid = async (): Promise<void> => {
  const before = await invoke<SafPickState>('saf_get_pick_state')
  await invoke('saf_request_pick')

  // 系统选择器为异步 UI：轮询直到授权版本号变化，超时 3 分钟
  const timeoutMs = 3 * 60 * 1000
  const startedAt = Date.now()
  let state: SafPickState = before
  while (Date.now() - startedAt < timeoutMs) {
    await sleep(500)
    state = await invoke<SafPickState>('saf_get_pick_state')
    if (state.version !== before.version && state.uri) break
  }

  if (!state.uri || state.version === before.version) {
    logger.warn('SAF pick cancelled or timed out')
    return
  }
  await commitDirectory(state.uri)
}

/** 把目录（绝对路径 / content URI 均可）写入配置并同步到媒体库 */
const commitDirectory = async (directory: string): Promise<void> => {
  if (musicDirectories.value.includes(directory)) return
  const result = await invoke<string[]>('add_music_directory', { path: directory })
  configStore.musicDirectories = result
  musicLibraryStore.musicFolders = result

  if (musicDirectories.value.length === 0) {
    setTimeout(async () => {
      await musicLibraryStore.refreshMusicFolders()
    }, 100)
  }
}

/**
 * 目录显示名：Android 的 SAF 树 URI（content://.../tree/primary%3AMusic）不能直接展示，
 * 转成可读的 "Music"；桌面端绝对路径原样显示（只保留末段避免过长）。
 */
const displayFolderName = (folder: string): string => {
  if (!folder.startsWith('content://')) return folder
  const tail = folder.split('/tree/')[1] ?? ''
  const decoded = decodeURIComponent(tail)
  const name = decoded.split(':').pop() || decoded.split('/').pop() || decoded
  return name || folder
}

const removeFolder = async (index: number): Promise<void> => {
  try {
    const pathToRemove = musicDirectories.value[index]

    // Android：同步清理 Kotlin 侧持久化的 SAF 树，否则残留状态会干扰下次授权
    if (pathToRemove?.startsWith('content://')) {
      await invoke('saf_clear_saved_tree').catch((err) =>
        logger.warn('Failed to clear SAF tree:', err),
      )
    }
    const result = await invoke<string[]>('remove_music_directory', { path: pathToRemove })
    configStore.musicDirectories = result
    musicLibraryStore.musicFolders = result
  } catch (error) {
    logger.error('Failed to remove folder:', error)
  }
}
</script>

<style scoped>
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

@media (hover: hover) {
  .folder-item:hover {
    background-color: var(--md-sys-color-surface-container-high);
  }
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

@media (hover: hover) {
  .filled-tonal-button:hover {
    background-color: color-mix(
      in srgb,
      var(--md-sys-color-on-surface) 8%,
      var(--md-sys-color-secondary-container)
    );
  }
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

@media (hover: hover) {
  .icon-button:hover {
    background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  }
}

.icon-button.danger {
  color: var(--md-sys-color-error);
}

@media (hover: hover) {
  .icon-button.danger:hover {
    background-color: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
}
</style>
