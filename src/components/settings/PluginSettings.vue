<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.plugins') }}</h3>
      <div class="header-actions">
        <!-- 提示放动作区：竖屏时面板头部的 h3 会整条隐藏，问号跟着 h3 会一起没了 -->
        <span class="help-tooltip">
          <button
            class="help-tooltip__trigger"
            type="button"
            :aria-label="$t('plugin.sandboxNoticeTitle')"
          >
            <span class="material-symbols-rounded">help</span>
          </button>
          <span class="help-tooltip__bubble" role="tooltip">
            <strong>{{ $t('plugin.sandboxNoticeTitle') }}</strong>
            {{ $t('plugin.sandboxNotice') }}
          </span>
        </span>
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

    <div v-if="plugins.length === 0 && !beatComponent" class="empty-state">
      <span class="material-symbols-rounded">extension</span>
      <p>{{ $t('config.noPlugins') }}</p>
      <p class="hint">{{ $t('config.pluginHint') }}</p>
    </div>

    <div v-else class="plugin-list">
      <!-- 原生组件跑在 Rust 侧，但复用 JS 插件的卡片形态，避免分两处管理 -->
      <div
        v-if="beatComponent"
        class="plugin-item"
        :class="{
          'plugin-item--active': beatComponent.ready,
          'plugin-item--missing': !beatComponent.ready,
        }"
      >
        <div class="plugin-info">
          <div class="plugin-header">
            <span class="plugin-name">{{ $t('plugin.beatComponentName') }}</span>
            <span class="plugin-builtin">{{ $t('plugin.nativeComponent') }}</span>
            <span
              class="plugin-state"
              :class="beatComponent.ready ? 'state-active' : 'state-inactive'"
            >
              {{ beatStateText }}
            </span>
          </div>
          <p class="plugin-description">{{ $t('plugin.beatComponentDesc') }}</p>
          <!-- 安装说明只在真缺东西时出现：装好了还念叨"不随安装包分发"是噪音 -->
          <p v-if="!beatComponent.ready" class="plugin-description">
            {{ $t('plugin.beatComponentInstallHint') }}
          </p>
          <!-- 模型在、运行时不在：功能可用只是慢，别让下面的 missing 看成装坏了 -->
          <p v-else-if="!beatComponent.gpu" class="plugin-description">
            {{ $t('plugin.beatComponentNoGpu') }}
          </p>
          <div class="plugin-meta">
            <span class="plugin-author">
              <span class="material-symbols-rounded">memory</span>
              {{ $t('plugin.beatComponentFootprint') }}
            </span>
          </div>
          <!-- 缺什么就列什么，用户照着往目录里放。功能已可用时这里只剩运行时 -->
          <ul v-if="beatComponent.missing.length" class="plugin-files">
            <li v-for="file in beatComponent.missing" :key="file">{{ file }}</li>
          </ul>

          <!-- 功能入口只在装好后出现：没装模型时按钮点下去只会报错 -->
          <div v-if="beatComponent.ready" class="beat-toolbar">
            <button type="button" class="beat-run-button" @click="openBeatAnalysis">
              <span class="material-symbols-rounded">graphic_eq</span>
              {{ $t('plugin.beatRunAnalysis') }}
            </button>
            <span v-if="beatCacheDesc" class="beat-cache-hint">{{ beatCacheDesc }}</span>
          </div>
        </div>
        <div class="plugin-actions">
          <!-- 和其他插件保持同一套动作语义：play 表示启用、pause 表示停用 -->
          <button
            v-if="!configStore.general.enableBpmAnalysis"
            class="icon-button"
            :title="$t('config.activate')"
            :disabled="!beatComponent.ready"
            @click="toggleBpmAnalysis(true)"
          >
            <span class="material-symbols-rounded">play_arrow</span>
          </button>
          <button
            v-else
            class="icon-button"
            :title="$t('config.deactivate')"
            @click="toggleBpmAnalysis(false)"
          >
            <span class="material-symbols-rounded">pause</span>
          </button>
          <button
            class="icon-button"
            :title="$t('plugin.beatOpenFolder')"
            @click="openBeatComponentFolder"
          >
            <span class="material-symbols-rounded">folder_open</span>
          </button>
        </div>
      </div>

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

    <BeatAnalysisDialog
      :open="beatDialogOpen"
      :paths="beatDialogPaths"
      @close="beatDialogOpen = false"
      @settled="onBeatAnalysisSettled"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { pluginManager, PluginState, loadAllPlugins } from '../../plugins'
import type { Plugin, PluginStateType } from '../../plugins/pluginManager'
import {
  beatBackendActual,
  beatCacheStats,
  beatComponentStatus,
  openBeatComponentDir,
  type BeatComponentStatus,
} from '../../services/beatService'
import { useConfigStore } from '../../stores/config'
import { useMusicLibraryStore } from '../../stores/musicLibrary'
import { saveConfigSafely } from '../../utils/errorMessages'
import BeatAnalysisDialog from './BeatAnalysisDialog.vue'
import logger from '../../utils/logger'
import { useErrorNotification } from '../../composables/useErrorNotification'

const { t } = useI18n()
const { showError } = useErrorNotification()
const configStore = useConfigStore()
const libraryStore = useMusicLibraryStore()

const plugins = computed<Plugin[]>(() => pluginManager.getAllPlugins())

/** 节拍分析组件状态。`null` 表示还没查出来，此时不渲染卡片免得闪一下空壳 */
const beatComponent = ref<BeatComponentStatus | null>(null)

/** 装齐了显示"运行中"，只有模型显示"CPU 推理"，都没有显示"未安装" */
const beatStateText = computed(() => {
  if (!beatComponent.value?.ready) return t('plugin.beatComponentMissing')
  return beatComponent.value.gpu
    ? t('plugin.beatComponentReadyGpu')
    : t('plugin.beatComponentReadyCpu')
})

/** 批量分析弹窗 */
const beatDialogOpen = ref(false)
const beatDialogPaths = ref<string[]>([])
const beatAnalyzedCount = ref(0)
const beatCacheKb = ref(0)
/** 上一次实际用上的推理后端，空串表示还没分析过 */
const beatBackendName = ref('')

/** 全库曲目路径（去重）。
 * 必须用 `musicLibrary` 而不是 `playerStore.playlist`：后者可能为空或只含子集。 */
const allTrackPaths = computed(() => {
  const seen = new Set<string>()
  for (const playlist of libraryStore.playlists) {
    for (const track of playlist.files) {
      if (track.path) seen.add(track.path)
    }
  }
  return [...seen]
})

const beatCacheDesc = computed(() => {
  if (!beatComponent.value?.ready) return ''
  if (beatAnalyzedCount.value <= 0) return ''
  const total = allTrackPaths.value.length
  const size = beatCacheKb.value > 0 ? `（${beatCacheKb.value} KB）` : ''
  // 曲库懒加载，插件页可能先就绪；total 为 0 时只报已分析数量
  const base =
    total > 0
      ? t('config.beatAnalysisCached', { count: beatAnalyzedCount.value, total })
      : t('plugin.beatAnalyzedOnly', { count: beatAnalyzedCount.value })
  if (!beatBackendName.value) return base + size
  // 后端标签统一小写；未分析过时回退成探测结果，所以不是 directml 就当 CPU
  const backend =
    beatBackendName.value === 'directml'
      ? t('config.beatBackendGpu')
      : t('config.beatBackendCpu')
  return `${base}${size} · ${backend}`
})

const loadBeatComponent = async (): Promise<void> => {
  try {
    beatComponent.value = await beatComponentStatus()
  } catch (error) {
    logger.error('读取节拍分析组件状态失败:', error)
  }
}

const loadBeatCacheStats = async (): Promise<void> => {
  try {
    const [count, kb] = await beatCacheStats()
    beatAnalyzedCount.value = count
    beatCacheKb.value = kb
  } catch (error) {
    logger.error('读取节拍缓存统计失败:', error)
  }
  try {
    beatBackendName.value = await beatBackendActual()
  } catch (error) {
    logger.error('读取节拍推理后端失败:', error)
  }
}

/** 开关只改配置：它管的是"切歌时顺带识别"，不是"立刻分析整库" */
const toggleBpmAnalysis = async (next: boolean): Promise<void> => {
  configStore.general.enableBpmAnalysis = next
  await saveConfigSafely(configStore)
}

/** 打开批量分析弹窗。曲库是懒加载的，用户可能直接进插件页，这时 playlists 还是空的 */
const openBeatAnalysis = async (): Promise<void> => {
  if (libraryStore.playlists.length === 0) {
    try {
      await libraryStore.loadPlaylistsFromCache()
    } catch (error) {
      logger.error('加载曲库缓存失败:', error)
    }
  }
  // 即使为空也打开弹窗：静默 return 会让用户以为按钮坏了
  beatDialogPaths.value = allTrackPaths.value
  beatDialogOpen.value = true
}

/** 一轮分析结束：刷新缓存统计，并重新读一次实际后端 */
const onBeatAnalysisSettled = () => {
  void loadBeatCacheStats()
}

const openBeatComponentFolder = async (): Promise<void> => {
  try {
    await openBeatComponentDir()
  } catch (error) {
    logger.error('打开节拍分析组件目录失败:', error)
    showError(t('plugin.openFolderFailed'), 'error')
  }
}

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
    // 组件状态也一起刷：用户刚往目录里放完文件，点刷新就该看到"已安装"
    await loadBeatComponent()
    showError(t('plugin.refreshed'), 'info')
  } catch (error) {
    logger.error('刷新插件失败:', error)
    const msg = error instanceof Error ? error.message : String(error)
    showError(t('plugin.refreshFailed', { message: msg }), 'error')
  }
}

onMounted(async () => {
  // 内置插件已在 main.js 中加载
  await loadBeatComponent()
  // 曲库是懒加载的：不先拉一次，"已分析 N / total"里的 total 会是 0
  if (libraryStore.playlists.length === 0) {
    try {
      await libraryStore.loadPlaylistsFromCache()
    } catch (error) {
      logger.error('加载曲库缓存失败:', error)
    }
  }
  void loadBeatCacheStats()
  // 用户按引导去放文件，切回来时自动重测，否则要重启应用
  window.addEventListener('focus', loadBeatComponent)
})

onUnmounted(() => {
  window.removeEventListener('focus', loadBeatComponent)
})
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

/* 沙箱与来源安全提示 (动作区最左侧的问号按钮,hover 显示气泡) */
.help-tooltip {
  position: relative;
  display: inline-flex;
  align-items: center;
}

.help-tooltip__trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  transition: color 0.2s ease;
}

.help-tooltip__trigger:hover,
.help-tooltip__trigger:focus-visible {
  color: var(--md-sys-color-primary);
}

.help-tooltip__trigger .material-symbols-rounded {
  font-size: 18px;
}

.help-tooltip__bubble {
  position: absolute;
  /* 挂在按钮下方并右对齐：原来的"向右展开"会顶出窗口 */
  top: calc(100% + 6px);
  right: 0;
  width: max-content;
  max-width: 320px;
  padding: 10px 14px;
  border-radius: 8px;
  background-color: var(--md-sys-color-background);
  color: var(--md-sys-color-on-surface-variant);
  box-shadow: var(--md-sys-elevation-2, 0 2px 6px rgba(0, 0, 0, 0.25));
  font-size: 12px;
  font-weight: 400;
  line-height: 1.6;
  text-align: left;
  white-space: normal;
  z-index: 10;
  /* 未激活时隐藏且不可交互,保持过渡 */
  opacity: 0;
  visibility: hidden;
  transition:
    opacity 0.15s ease,
    visibility 0.15s ease;
}

.help-tooltip:hover .help-tooltip__bubble,
.help-tooltip:focus-within .help-tooltip__bubble {
  opacity: 1;
  visibility: visible;
}

.help-tooltip__bubble strong {
  display: block;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin-bottom: 4px;
}

/* 竖屏：320px 气泡左右都会溢出，改成贴视口 16px 的横条。
   top 不能沿用 100%——固定定位下那是视口高度，会甩到屏外 */
@media (orientation: portrait) {
  .help-tooltip__bubble {
    position: fixed;
    /* 128 = 头部 64 + 上内边距 16 + 按钮 40 + 余量 8 */
    top: calc(env(safe-area-inset-top, 0px) + 128px);
    right: 16px;
    left: 16px;
    width: auto;
    max-width: none;
  }
}

.header-actions {
  display: flex;
  align-items: center;
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

/* 未安装时信息区压暗，但动作区不动：「打开目录」是唯一的安装途径 */
.plugin-item--missing .plugin-info {
  opacity: 0.55;
}

.plugin-item--missing {
  border-left-color: var(--md-sys-color-outline-variant);
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

/* 缺失文件清单：等宽字体表明"这是路径"，用户照着往目录里放就行 */
.plugin-files {
  margin: 8px 0 0;
  padding-left: 18px;
  font-family: ui-monospace, monospace;
  font-size: 12px;
  line-height: 1.7;
  color: var(--md-sys-color-on-surface-variant);
}

/* 功能入口：装了才出现，和下面的缺失清单不会同时在场 */
.beat-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  margin-top: 12px;
}

.beat-run-button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border: none;
  border-radius: 20px;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.beat-run-button:hover {
  background-color: var(--md-sys-color-hover-overlay);
}

.beat-run-button .material-symbols-rounded {
  font-size: 18px;
}

.beat-cache-hint {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
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
  border-radius: 20px;
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

/* 组件没装时「启用」按钮不可点：置灰且不给悬停反馈 */
.icon-button:disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.icon-button:disabled:hover {
  background: none;
}

.icon-button.danger {
  color: var(--md-sys-color-error);}

@media (hover: hover) {
  .icon-button.danger:hover {
    background-color: var(--md-sys-color-error-container);
    color: var(--md-sys-color-on-error-container);
  }
}
</style>
