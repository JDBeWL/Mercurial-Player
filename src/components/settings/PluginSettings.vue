<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.plugins') }}</h3>
      <div class="header-actions">
        <button class="filled-tonal-button" @click="openPluginsFolder">
          <span class="material-symbols-rounded">folder_open</span>
          {{ $t('config.openPluginsFolder') }}
        </button>
        <button class="filled-tonal-button" @click="refreshPlugins">
          <span class="material-symbols-rounded">refresh</span>
          {{ $t('config.refresh') }}
        </button>
      </div>
    </div>

    <div v-if="plugins.length === 0" class="empty-state">
      <span class="material-symbols-rounded">extension</span>
      <p>{{ $t('config.noPlugins') }}</p>
      <p class="hint">{{ $t('config.pluginHint') }}</p>
    </div>

    <div v-else class="plugin-list">
      <div
        v-for="plugin in plugins"
        :key="plugin.id"
        class="plugin-item"
        :class="{
          'plugin-item--active': plugin.state === 'active',
          'plugin-item--error': plugin.state === 'error',
        }"
      >
        <div class="plugin-info">
          <div class="plugin-header">
            <span class="plugin-name">{{ plugin.name }}</span>
            <span v-if="plugin.id.startsWith('builtin-')" class="plugin-builtin">{{
              $t('plugin.builtin')
            }}</span>
            <span class="plugin-state" :class="`state-${plugin.state}`">
              {{ getStateText(plugin.state) }}
            </span>
          </div>
          <p class="plugin-description">{{ plugin.description || $t('plugin.noDescription') }}</p>
          <div class="plugin-meta">
            <span class="plugin-author">
              <span class="material-symbols-rounded">person</span>
              {{ plugin.author }}
            </span>
            <span class="plugin-version">v{{ plugin.version }}</span>
            <span v-if="plugin.permissions?.length" class="plugin-permissions">
              <span class="material-symbols-rounded">security</span>
              {{ $t('plugin.permissionsCount', { count: plugin.permissions.length }) }}
            </span>
          </div>
          <p v-if="plugin.error" class="plugin-error">{{ plugin.error }}</p>
        </div>
        <div class="plugin-actions">
          <button
            v-if="plugin.state === 'inactive'"
            class="icon-button"
            :title="$t('config.activate')"
            @click="activatePlugin(plugin.id)"
          >
            <span class="material-symbols-rounded">play_arrow</span>
          </button>
          <button
            v-if="plugin.state === 'active'"
            class="icon-button"
            :title="$t('config.deactivate')"
            @click="deactivatePlugin(plugin.id)"
          >
            <span class="material-symbols-rounded">pause</span>
          </button>
          <button
            v-if="!plugin.id.startsWith('builtin-')"
            class="icon-button danger"
            :title="$t('config.uninstall')"
            @click="uninstallPlugin(plugin.id)"
          >
            <span class="material-symbols-rounded">delete</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { pluginManager, PluginState, loadAllPlugins } from '../../plugins'
import type { Plugin, PluginStateType } from '../../plugins/pluginManager'
import logger from '../../utils/logger'
import { useErrorNotification } from '../../composables/useErrorNotification'

const { t } = useI18n()
const { showError } = useErrorNotification()

const plugins = computed<Plugin[]>(() => pluginManager.getAllPlugins())

const getStateText = (state: PluginStateType): string => {
  const texts: Partial<Record<PluginStateType, string>> = {
    [PluginState.ACTIVE]: t('plugin.state.active'),
    [PluginState.INACTIVE]: t('plugin.state.inactive'),
    [PluginState.ERROR]: t('plugin.state.error'),
    [PluginState.DISABLED]: t('plugin.state.disabled'),
  }
  return texts[state] || state
}

const activatePlugin = async (pluginId: string): Promise<void> => {
  try {
    await pluginManager.activate(pluginId)
    showError(t('plugin.activated'), 'info')
  } catch (error) {
    logger.error('激活插件失败:', error)
    const msg = error instanceof Error ? error.message : String(error)
    showError(t('plugin.activateFailed', { message: msg }), 'error')
  }
}

const deactivatePlugin = async (pluginId: string): Promise<void> => {
  try {
    await pluginManager.deactivate(pluginId)
    showError(t('plugin.deactivated'), 'info')
  } catch (error) {
    logger.error('停用插件失败:', error)
    const msg = error instanceof Error ? error.message : String(error)
    showError(t('plugin.deactivateFailed', { message: msg }), 'error')
  }
}

const uninstallPlugin = async (pluginId: string): Promise<void> => {
  try {
    await pluginManager.uninstall(pluginId)
    await invoke('uninstall_plugin', { pluginId })
    showError(t('plugin.uninstalled'), 'info')
  } catch (error) {
    logger.error('卸载插件失败:', error)
    const msg = error instanceof Error ? error.message : String(error)
    showError(t('plugin.uninstallFailed', { message: msg }), 'error')
  }
}

const openPluginsFolder = async (): Promise<void> => {
  try {
    await invoke('open_plugins_directory')
  } catch (error) {
    logger.error('打开插件目录失败:', error)
    showError(t('plugin.openFolderFailed'), 'error')
  }
}

const refreshPlugins = async (): Promise<void> => {
  try {
    // 先停用所有非内置插件
    const currentPlugins = pluginManager.getAllPlugins()
    for (const plugin of currentPlugins) {
      if (!plugin.id.startsWith('builtin-')) {
        try {
          await pluginManager.deactivate(plugin.id)
          await pluginManager.uninstall(plugin.id, false) // 不清除存储
        } catch (error) {
          logger.warn(`卸载插件失败: ${plugin.id}`, error)
        }
      }
    }

    // 重新加载所有插件
    await loadAllPlugins()
    showError(t('plugin.refreshed'), 'info')
  } catch (error) {
    logger.error('刷新插件失败:', error)
    const msg = error instanceof Error ? error.message : String(error)
    showError(t('plugin.refreshFailed', { message: msg }), 'error')
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
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
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
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
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
