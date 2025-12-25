<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.plugins') || '插件' }}</h3>
      <div class="header-actions">
        <button class="filled-tonal-button" @click="openPluginsFolder">
          <span class="material-symbols-rounded">folder_open</span>
          {{ $t('config.openPluginsFolder') || '打开插件目录' }}
        </button>
        <button class="filled-tonal-button" @click="refreshPlugins">
          <span class="material-symbols-rounded">refresh</span>
          {{ $t('config.refresh') || '刷新' }}
        </button>
      </div>
    </div>

    <div v-if="plugins.length === 0" class="empty-state">
      <span class="material-symbols-rounded">extension</span>
      <p>{{ $t('config.noPlugins') || '暂无已安装的插件' }}</p>
      <p class="hint">{{ $t('config.pluginHint') || '将插件文件夹放入插件目录即可安装' }}</p>
    </div>

    <div v-else class="plugin-list">
      <div 
        v-for="plugin in plugins" 
        :key="plugin.id" 
        class="plugin-item"
        :class="{ 'plugin-item--active': plugin.state === 'active', 'plugin-item--error': plugin.state === 'error' }"
      >
        <div class="plugin-info">
          <div class="plugin-header">
            <span class="plugin-name">{{ plugin.name }}</span>
            <span v-if="plugin.id.startsWith('builtin-')" class="plugin-builtin">内置</span>
            <span class="plugin-state" :class="`state-${plugin.state}`">
              {{ getStateText(plugin.state) }}
            </span>
          </div>
          <p class="plugin-description">{{ plugin.description || '暂无描述' }}</p>
          <div class="plugin-meta">
            <span class="plugin-author">
              <span class="material-symbols-rounded">person</span>
              {{ plugin.author }}
            </span>
            <span class="plugin-version">v{{ plugin.version }}</span>
            <span class="plugin-permissions" v-if="plugin.permissions?.length">
              <span class="material-symbols-rounded">security</span>
              {{ plugin.permissions.length }} 项权限
            </span>
          </div>
          <p v-if="plugin.error" class="plugin-error">{{ plugin.error }}</p>
        </div>
        <div class="plugin-actions">
          <button 
            v-if="plugin.state === 'inactive'" 
            class="icon-button" 
            @click="activatePlugin(plugin.id)"
            :title="$t('config.activate') || '激活'"
          >
            <span class="material-symbols-rounded">play_arrow</span>
          </button>
          <button 
            v-if="plugin.state === 'active'" 
            class="icon-button" 
            @click="deactivatePlugin(plugin.id)"
            :title="$t('config.deactivate') || '停用'"
          >
            <span class="material-symbols-rounded">pause</span>
          </button>
          <button 
            v-if="!plugin.id.startsWith('builtin-')"
            class="icon-button danger" 
            @click="uninstallPlugin(plugin.id)"
            :title="$t('config.uninstall') || '卸载'"
          >
            <span class="material-symbols-rounded">delete</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { pluginManager, PluginState } from '../../plugins'
import logger from '../../utils/logger'
import { useErrorNotification } from '../../composables/useErrorNotification'

const { showError } = useErrorNotification()

const plugins = computed(() => pluginManager.getAllPlugins())

const getStateText = (state) => {
  const texts = {
    [PluginState.ACTIVE]: '运行中',
    [PluginState.INACTIVE]: '未激活',
    [PluginState.ERROR]: '错误',
    [PluginState.DISABLED]: '已禁用',
  }
  return texts[state] || state
}

const activatePlugin = async (pluginId) => {
  try {
    await pluginManager.activate(pluginId)
    showError('插件已激活', 'info')
  } catch (error) {
    logger.error('激活插件失败:', error)
    showError(`激活失败: ${error.message}`, 'error')
  }
}

const deactivatePlugin = async (pluginId) => {
  try {
    await pluginManager.deactivate(pluginId)
    showError('插件已停用', 'info')
  } catch (error) {
    logger.error('停用插件失败:', error)
    showError(`停用失败: ${error.message}`, 'error')
  }
}

const uninstallPlugin = async (pluginId) => {
  try {
    await pluginManager.uninstall(pluginId)
    await invoke('uninstall_plugin', { pluginId })
    showError('插件已卸载', 'info')
  } catch (error) {
    logger.error('卸载插件失败:', error)
    showError(`卸载失败: ${error.message}`, 'error')
  }
}

const openPluginsFolder = async () => {
  try {
    await invoke('open_plugins_directory')
  } catch (error) {
    logger.error('打开插件目录失败:', error)
    showError('无法打开插件目录', 'error')
  }
}

const refreshPlugins = async () => {
  try {
    const { loadAllPlugins } = await import('../../plugins')
    await loadAllPlugins()
    showError('插件列表已刷新', 'info')
  } catch (error) {
    logger.error('刷新插件失败:', error)
  }
}

onMounted(async () => {
  // 内置插件已在 main.js 中加载
})
</script>

<style scoped>
.tab-content {
  max-width: 800px;
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

.header-actions {
  display: flex;
  gap: 8px;
}

.plugin-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.plugin-item {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
  border-left: 4px solid var(--md-sys-color-outline);
  transition: all 0.2s ease;
}

.plugin-item--active {
  border-left-color: var(--md-sys-color-primary);
}

.plugin-item--error {
  border-left-color: var(--md-sys-color-error);
}

.plugin-info {
  flex: 1;
}

.plugin-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}

.plugin-name {
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.plugin-builtin {
  font-size: 10px;
  color: var(--md-sys-color-on-tertiary-container);
  background-color: var(--md-sys-color-tertiary-container);
  padding: 2px 6px;
  border-radius: 4px;
  text-transform: uppercase;
  font-weight: 600;
}

.plugin-version {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.plugin-state {
  font-size: 10px;
  padding: 2px 6px;
  border-radius: 4px;
  text-transform: uppercase;
  font-weight: 600;
}

.state-active {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.state-inactive {
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-on-surface-variant);
}

.state-error {
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

.plugin-description {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  margin: 8px 0;
}

.plugin-meta {
  display: flex;
  gap: 16px;
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.plugin-meta span {
  display: flex;
  align-items: center;
  gap: 4px;
}

.plugin-meta .material-symbols-rounded {
  font-size: 16px;
}

.plugin-error {
  font-size: 12px;
  color: var(--md-sys-color-error);
  margin-top: 8px;
}

.plugin-actions {
  display: flex;
  gap: 4px;
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

.empty-state .hint {
  margin-top: 8px;
  font-size: 12px;
  opacity: 0.7;
}

/* 按钮样式 */
.filled-tonal-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
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
  border-radius: 20px;
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
