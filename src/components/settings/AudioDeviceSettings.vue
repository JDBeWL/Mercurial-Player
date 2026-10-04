<template>
  <div class="audio-device-settings">
    <div class="content-header">
      <h3>{{ $t('config.audioDeviceSettings') }}</h3>
      <!-- Android 无可切换输出设备, 刷新只会得到系统默认 -->
      <button v-if="!isAndroidPlatform" class="filled-tonal-button" @click="refreshDevices">
        <span class="material-symbols-rounded">refresh</span>
        {{ $t('config.refreshDevices') }}
      </button>
    </div>

    <!-- Android: 输出路由由系统统一管理 (扬声器/蓝牙/有线自动切换), 后端只上报默认设备, 不展示列表 -->
    <div v-if="isAndroidPlatform" class="capability-notice platform-notice">
      <span class="material-symbols-rounded">speaker_phone</span>
      <p>{{ $t('config.audioDeviceManagedBySystem') }}</p>
    </div>

    <div v-if="isAndroidPlatform" class="route-panel">
      <div class="route-row">
        <span class="route-label">{{ $t('config.currentOutputRoute') }}</span>
        <span class="route-value">
          <span class="material-symbols-rounded route-icon">{{ routeIcon }}</span>
          {{ routeLabel }}
        </span>
      </div>
      <div class="route-row">
        <span class="route-label">{{ $t('config.outputSampleRate') }}</span>
        <span class="route-value">
          {{ audioRoute?.sampleRate ? `${audioRoute.sampleRate} Hz` : '—' }}
          <template v-if="audioRoute?.channels"> · {{ audioRoute.channels }}ch </template>
        </span>
      </div>
    </div>

    <div v-else-if="audioDevices.length > 0" class="device-list">
      <div
        v-for="device in audioDevices"
        :key="device.name"
        class="device-item"
        :class="{ active: currentDevice?.name === device.name }"
        @click="selectDevice(device)"
      >
        <div class="device-info">
          <span class="device-name">{{ device.name }}</span>
          <div class="device-badges">
            <span v-if="device.isDefault" class="device-badge default">
              {{ $t('config.defaultDevice') }}
            </span>
            <span v-if="device.supportsExclusiveMode" class="device-badge exclusive">
              {{ $t('config.exclusiveModeSupported') }}
            </span>
          </div>
        </div>
        <div class="device-icon">
          <span v-if="currentDevice?.name === device.name" class="material-symbols-rounded">
            check_circle
          </span>
          <span v-else class="material-symbols-rounded"> radio_button_unchecked </span>
        </div>
      </div>
    </div>

    <div class="audio-options">
      <div
        class="option-item"
        :class="{ disabled: !isWindowsPlatform }"
        @click="isWindowsPlatform && toggleExclusiveMode()"
      >
        <div class="option-label">
          <span class="material-symbols-rounded">album</span>
          <div class="option-text">
            <h4>{{ $t('config.exclusiveMode') }}</h4>
            <p>{{ $t('config.exclusiveModeDesc') }}</p>
            <div v-if="currentDevice" class="device-status">
              <span class="status-label">{{ $t('config.currentAudioMode') }}:</span>
              <span class="status-value" :class="`status-${currentDevice.audioModeStatus}`">
                {{ $t(`config.exclusiveModeStatus.${currentDevice.audioModeStatus}`) }}
              </span>
            </div>
          </div>
        </div>
        <div class="option-control">
          <!-- 不绑 update: 点击冒泡给整行走 toggleExclusiveMode; 设备不支持仅变暗, 平台不支持才禁用 -->
          <SettingSwitch
            :model-value="useExclusiveMode"
            :disabled="!isWindowsPlatform"
            :class="{ disabled: currentDevice != null && !currentDevice.supportsExclusiveMode }"
          />
        </div>
      </div>

      <!-- Android USB DAC 独占 (位完美) 对应 Windows WASAPI 独占; 安卓设备由系统路由, 只在插入 USB 音频设备时有意义 -->
      <div
        v-if="isAndroidPlatform"
        class="option-item"
        :class="{ disabled: !audioRoute?.usbConnected }"
        @click="toggleUsbDacExclusive"
      >
        <div class="option-label">
          <span class="material-symbols-rounded">usb</span>
          <div class="option-text">
            <h4>{{ $t('config.usbDacExclusive') }}</h4>
            <p>{{ $t('config.usbDacExclusiveDesc') }}</p>
            <div v-if="usbDacExclusiveEnabled" class="device-status">
              <span class="status-label">{{ $t('config.currentAudioMode') }}:</span>
              <span
                class="status-value"
                :class="audioRoute?.exclusiveActive ? 'status-exclusive' : 'status-standard'"
              >
                {{
                  audioRoute?.exclusiveActive
                    ? $t('config.usbDacActive')
                    : $t('config.exclusiveModeStatus.standard')
                }}
              </span>
            </div>
          </div>
        </div>
        <div class="option-control">
          <SettingSwitch
            :model-value="usbDacExclusiveEnabled"
            :disabled="!audioRoute?.usbConnected"
          />
        </div>
      </div>

      <!-- Android 有自己的 USB DAC 通道, 不算不支持独占 -->
      <div
        v-if="!isWindowsPlatform && !isAndroidPlatform"
        class="capability-notice platform-notice"
      >
        <span class="material-symbols-rounded">desktop_windows</span>
        <p>{{ $t('config.exclusiveModePlatformNotSupported') }}</p>
      </div>

      <!-- 三种降级态: 等下一首生效 / 未插 DAC / 独占未生效已回退标准输出 -->
      <div v-if="isAndroidPlatform && pendingNextTrack" class="capability-notice">
        <span class="material-symbols-rounded">playlist_play</span>
        <p>{{ $t('config.usbDacTakesEffectNextTrack') }}</p>
      </div>
      <div v-else-if="isAndroidPlatform && !audioRoute?.usbConnected" class="capability-notice">
        <span class="material-symbols-rounded">usb_off</span>
        <p>{{ $t('config.usbDacNotConnected') }}</p>
      </div>
      <div
        v-else-if="
          isAndroidPlatform && usbDacExclusiveEnabled && audioRoute && !audioRoute.exclusiveActive
        "
        class="capability-notice"
      >
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.usbDacFallback') }}</p>
      </div>

      <div
        v-else-if="currentDevice && !currentDevice.supportsExclusiveMode && useExclusiveMode"
        class="capability-notice"
      >
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeNotSupported') }}</p>
      </div>

      <div v-if="isWindowsPlatform && useExclusiveMode" class="capability-notice">
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeWarning') }}</p>
      </div>

      <div class="option-item" @click="toggleFadeEnabled()">
        <div class="option-label">
          <span class="material-symbols-rounded">graphic_eq</span>
          <div class="option-text">
            <h4>{{ $t('config.fadeEnabled') }}</h4>
            <p>{{ $t('config.fadeEnabledDesc') }}</p>
          </div>
        </div>
        <div class="option-control">
          <SettingSwitch :model-value="fadeEnabled" />
        </div>
      </div>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>{{ $t('config.loadingDevices') }}</p>
    </div>

    <div v-if="restartRequired" class="restart-notice">
      <span class="material-symbols-rounded">restart_alt</span>
      <div class="notice-content">
        <p>{{ $t('config.exclusiveModeRestartRequired') }}</p>
        <p class="notice-hint">{{ $t('config.exclusiveModeRestartHint') }}</p>
      </div>
    </div>

    <div v-else-if="error" class="error-state">
      <span class="material-symbols-rounded">error</span>
      <p>{{ $t('config.deviceLoadError') }}: {{ error }}</p>
      <button class="retry-button" @click="refreshDevices">
        {{ $t('config.retry') }}
      </button>
    </div>

    <div v-if="audioDevices.length === 0 && !loading && !error" class="empty-state">
      <span class="material-symbols-rounded">speaker</span>
      <p>{{ $t('config.noAudioDevices') }}</p>
      <button class="refresh-button" @click="refreshDevices">
        {{ $t('config.refresh') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, watch, computed } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  getAudioDevices,
  getCurrentAudioDevice,
  getExclusiveMode,
  getFadeEnabled,
  setAudioDevice,
  setFadeEnabled,
  toggleExclusiveMode as toggleExclusiveModeCommand,
  getAudioRoute,
  setUsbDacExclusive,
  type AudioDevice,
  type AudioRouteInfo,
} from '../../services/audioService'
import { getPlatform } from '../../services/appService'
import { useI18n } from 'vue-i18n'
import { usePlayerStore } from '../../stores/player'
import { useConfigStore } from '../../stores/config'
import logger from '../../utils/logger'
import { getErrorMessage } from '../../utils/errorMessages'
import SettingSwitch from './SettingSwitch.vue'

const playerStore = usePlayerStore()
const configStore = useConfigStore()
const { t } = useI18n()

const audioDevices = ref<AudioDevice[]>([])
const currentDevice = ref<AudioDevice | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const restartRequired = ref(false)
const useExclusiveMode = ref(false)
const fadeEnabled = ref(true)
const currentPlatform = ref<string>('unknown')

/** Android 输出路由快照; 非安卓平台保持 null */
const audioRoute = ref<AudioRouteInfo | null>(null)
const usbDacExclusiveEnabled = ref(false)
// 切过开关但当前曲目仍在旧输出上: 提示"下一首生效"
const pendingNextTrack = ref(false)

const isWindowsPlatform = computed(() => {
  return currentPlatform.value === 'windows'
})

// 见模板的平台说明: Android 不展示设备列表
const isAndroidPlatform = computed(() => {
  return currentPlatform.value === 'android'
})

/** 未接 USB 时按主要设备类型给路由起名 */
const routeLabel = computed(() => {
  const route = audioRoute.value
  if (!route) return '—'
  if (route.usbConnected && route.usbDeviceName) return route.usbDeviceName
  const usb = route.devices?.find((d) => d.isUsb)
  if (usb) return usb.name
  const type = route.devices?.find((d) => !d.isUsb)?.typeName ?? ''
  if (type.includes('BLUETOOTH')) return t('config.bluetoothAudio')
  if (type.includes('WIRED')) return t('config.wiredHeadphones')
  if (type.includes('SPEAKER') || type.includes('EARPIECE')) {
    return t('config.builtinSpeaker')
  }
  return t('config.unknownOutput')
})

const routeIcon = computed(() => {
  const route = audioRoute.value
  if (!route) return 'speaker'
  const type =
    route.devices?.find((d) => d.isUsb)?.typeName ??
    route.devices?.find((d) => !d.isUsb)?.typeName ??
    ''
  if (type.includes('USB')) return 'usb'
  if (type.includes('BLUETOOTH')) return 'bluetooth'
  if (type.includes('WIRED')) return 'headphones'
  return 'speaker'
})

/** 非安卓平台后端会报错, 静默忽略即可 */
const fetchAudioRoute = async (): Promise<void> => {
  if (!isAndroidPlatform.value) return
  try {
    audioRoute.value = await getAudioRoute()
    usbDacExclusiveEnabled.value = audioRoute.value.exclusiveEnabled
  } catch (err) {
    logger.warn('读取输出路由失败（非 Android 平台预期行为）:', err)
    audioRoute.value = null
  }
}

const toggleUsbDacExclusive = async (): Promise<void> => {
  if (!audioRoute.value?.usbConnected) {
    logger.warn('未检测到 USB 音频设备，忽略独占开关操作')
    return
  }
  const next = !usbDacExclusiveEnabled.value
  try {
    await setUsbDacExclusive(next)
    usbDacExclusiveEnabled.value = next
    configStore.setAudioConfig({ usbDacExclusive: next })
    await fetchAudioRoute()
    // 移动端下一首生效: 期望值与实际输出不一致时提示, 切歌后自然消失
    pendingNextTrack.value = audioRoute.value?.exclusiveActive !== next
  } catch (err) {
    logger.error('切换 USB DAC 独占失败:', err)
    error.value = getErrorMessage(err, 'Failed to toggle USB DAC exclusive mode')
  }
}

/** USB DAC 插拔由后端 audio-route-changed 事件广播; 打开设置页时顺手刷新一次 */
let unlistenRoute: UnlistenFn | null = null
// onMounted 里有一串 await, listen() 完成时组件可能已卸载, 需要事后自查
let mountedCleanupDone = false

const fetchAudioDevices = async (): Promise<void> => {
  loading.value = true
  error.value = null

  try {
    const devices = await getAudioDevices()
    audioDevices.value = devices

    const current = await getCurrentAudioDevice()
    currentDevice.value = current
  } catch (err) {
    logger.error('Failed to fetch audio devices:', err)
    error.value = getErrorMessage(err, 'Unknown error')
  } finally {
    loading.value = false
  }
}

const selectDevice = async (device: AudioDevice): Promise<void> => {
  if (currentDevice.value?.name === device.name) {
    return
  }

  loading.value = true
  error.value = null

  try {
    await setAudioDevice(device.name, playerStore.currentTime)
    currentDevice.value = device
  } catch (err) {
    logger.error('Failed to set audio device:', err)
    error.value = getErrorMessage(err, 'Unknown error')
  } finally {
    loading.value = false
  }
}

const checkRestartRequired = async (): Promise<void> => {
  // 只有 Windows 同时存在"已生效的独占模式"和"配置意向"两套状态;
  // 安卓的 exclusive_mode 跟随 USB DAC 偏好且按下一首生效, 比较会在切歌后误报需要重启
  if (!isWindowsPlatform.value) {
    restartRequired.value = false
    return
  }
  try {
    const activeExclusiveMode = await getExclusiveMode()
    restartRequired.value = activeExclusiveMode !== useExclusiveMode.value
  } catch (err) {
    logger.error('Failed to check active exclusive mode:', err)
  }
}

const toggleExclusiveMode = async (): Promise<void> => {
  if (!isWindowsPlatform.value && !useExclusiveMode.value) {
    logger.warn('Exclusive mode is only supported on Windows')
    return
  }

  if (
    currentDevice.value &&
    !currentDevice.value.supportsExclusiveMode &&
    !useExclusiveMode.value
  ) {
    // 设备不支持时只告警, 仍继续切换
    logger.warn('Trying to enable exclusive mode on unsupported device')
  }

  try {
    await toggleExclusiveModeCommand(!useExclusiveMode.value, playerStore.currentTime)

    // 后端成功返回即已生效, 清掉重启提示
    useExclusiveMode.value = !useExclusiveMode.value
    restartRequired.value = false

    try {
      const updatedDevice = await getCurrentAudioDevice()
      currentDevice.value = updatedDevice
    } catch (deviceErr) {
      logger.error('Failed to update current device info:', deviceErr)
    }
  } catch (err) {
    const errorMessage = getErrorMessage(err, String(err))

    // 后端用 RESTART_REQUIRED 标记"意向已存但未生效"
    if (errorMessage.includes('RESTART_REQUIRED')) {
      useExclusiveMode.value = !useExclusiveMode.value
      restartRequired.value = true
    } else {
      logger.error('Failed to toggle exclusive mode:', err)
      error.value = errorMessage || 'Failed to toggle exclusive mode'
    }
  }
}

const toggleFadeEnabled = async (): Promise<void> => {
  const newValue = !fadeEnabled.value
  try {
    await setFadeEnabled(newValue)
    fadeEnabled.value = newValue
    configStore.setAudioConfig({ fadeEnabled: newValue })
  } catch (err) {
    logger.error('Failed to toggle fade enabled:', err)
    error.value = getErrorMessage(err, 'Failed to toggle fade')
  }
}

const refreshDevices = (): void => {
  void fetchAudioDevices()
  void fetchAudioRoute()
}

onMounted(async () => {
  try {
    currentPlatform.value = await getPlatform()
    logger.debug('Detected platform:', currentPlatform.value)
  } catch (err) {
    logger.error('Failed to detect platform:', err)
    currentPlatform.value = 'unknown'
  }

  // 先用 store 的意向值, 缺省才发 IPC 查询
  if (configStore.audio?.exclusiveMode !== undefined) {
    useExclusiveMode.value = configStore.audio.exclusiveMode
  } else {
    try {
      useExclusiveMode.value = (await getExclusiveMode()) ?? false
    } catch (err) {
      logger.warn('Failed to get exclusive mode from backend:', err)
      useExclusiveMode.value = false
    }
  }

  if (configStore.audio?.fadeEnabled !== undefined) {
    fadeEnabled.value = configStore.audio.fadeEnabled
  } else {
    try {
      fadeEnabled.value = (await getFadeEnabled()) ?? true
    } catch (err) {
      logger.warn('Failed to get fade enabled from backend:', err)
      fadeEnabled.value = true
    }
  }

  await checkRestartRequired()

  // 取设备列表时不重新加载配置, 避免重置主题
  await fetchAudioDevices()

  try {
    currentDevice.value = await getCurrentAudioDevice()
  } catch (err) {
    logger.error('Failed to get current audio device:', err)
  }

  if (configStore.audio?.usbDacExclusive !== undefined) {
    usbDacExclusiveEnabled.value = configStore.audio.usbDacExclusive
  }
  await fetchAudioRoute()

  if (isAndroidPlatform.value) {
    try {
      const unlisten = await listen('audio-route-changed', () => {
        void fetchAudioRoute()
      })
      if (mountedCleanupDone) unlisten()
      else unlistenRoute = unlisten
    } catch (err) {
      logger.warn('订阅输出路由变化事件失败:', err)
    }
  }
})

onUnmounted(() => {
  mountedCleanupDone = true
  unlistenRoute?.()
  unlistenRoute = null
})

watch(currentDevice, (newDevice: AudioDevice | null) => {
  if (newDevice) {
    logger.debug('Audio device changed to:', newDevice.name, 'Mode:', newDevice.audioModeStatus)
  }
})

watch(useExclusiveMode, (newValue: boolean) => {
  configStore.setAudioConfig({ exclusiveMode: newValue })
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

.device-list {
  margin-bottom: 24px;
}

.device-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px;
  margin-bottom: 8px;
  border-radius: 12px;
  background-color: var(--md-sys-color-surface-container);
  cursor: pointer;
  transition: all 0.2s ease;
}

@media (hover: hover) {
  .device-item:hover {
    background-color: var(--md-sys-color-surface-container-high);
  }
}

.device-item.active {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.device-info {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
  overflow: hidden;
}

.device-name {
  font-size: 16px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.device-badges {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.device-badge {
  font-size: 12px;
  padding: 4px 12px;
  border-radius: 16px;
  font-weight: 500;
  white-space: nowrap;
}

.device-badge.default {
  background-color: var(--md-sys-color-tertiary-container);
  color: var(--md-sys-color-on-tertiary-container);
}

.device-badge.exclusive {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-surface);
}

.device-item.active .device-badge {
  background-color: var(--md-sys-color-surface);
  color: var(--md-sys-color-on-surface);
}

.device-icon {
  color: var(--md-sys-color-on-surface-variant);
  display: flex;
  align-items: center;
}

.device-item.active .device-icon {
  color: var(--md-sys-color-primary);
}

.route-panel {
  border-radius: 12px;
  padding: 4px 16px;
  margin-bottom: 24px;
  background-color: var(--md-sys-color-surface-container);
}

.route-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 0;
}

.route-row + .route-row {
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.route-label {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  flex-shrink: 0;
}

.route-value {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  /* USB 产品名可能很长, 优先截断而不是撑破面板 */
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.route-icon {
  font-size: 18px;
  color: var(--md-sys-color-primary);
  flex-shrink: 0;
}

.audio-options {
  border-top: 1px solid var(--md-sys-color-outline-variant);
  padding-top: 24px;
}

.option-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px;
  border-radius: 12px;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.option-item.disabled {
  cursor: not-allowed;
  opacity: 0.6;
}

@media (hover: hover) {
  .option-item.disabled:hover {
    background-color: transparent;
  }
}

@media (hover: hover) {
  .option-item:hover {
    background-color: var(--md-sys-color-surface-container);
  }
}

.option-label {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  flex: 1;
}

.option-label > .material-symbols-rounded {
  color: var(--md-sys-color-on-surface-variant);
  font-size: 24px;
  margin-top: 2px;
}

.option-text {
  flex: 1;
}

.option-text h4 {
  margin: 0 0 4px;
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.option-text p {
  margin: 0;
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
}

/* 开关视觉在 SettingSwitch; 这里只有设备不支持时的变暗态 */
.option-control .switch.disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.device-status {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  font-size: 14px;
}

.status-label {
  color: var(--md-sys-color-on-surface-variant);
}

.status-value {
  font-weight: 500;
}

.status-value.status-exclusive {
  color: var(--md-sys-color-primary);
}

.status-value.status-optimized {
  color: var(--md-sys-color-tertiary);
}

.status-value.status-standard {
  color: var(--md-sys-color-on-surface-variant);
}

.capability-notice {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 16px;
  border-radius: 12px;
  margin-top: 12px;
  font-size: 14px;
  background-color: var(--md-sys-color-tertiary-container);
  color: var(--md-sys-color-on-tertiary-container);
}

.capability-notice.platform-notice {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.capability-notice.platform-notice .material-symbols-rounded {
  font-size: 24px;
}

.capability-notice .material-symbols-rounded {
  font-size: 20px;
  flex-shrink: 0;
}

.capability-notice p {
  margin: 0;
  line-height: 1.5;
}

.restart-notice {
  display: flex;
  align-items: flex-start;
  gap: 16px;
  padding: 20px;
  border-radius: 12px;
  margin-top: 16px;
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.restart-notice .material-symbols-rounded {
  font-size: 24px;
  flex-shrink: 0;
}

.restart-notice .notice-content {
  flex: 1;
}

.restart-notice .notice-content p {
  margin: 0;
  font-size: 14px;
  line-height: 1.5;
}

.restart-notice .notice-content p:first-child {
  font-weight: 500;
  margin-bottom: 4px;
}

.restart-notice .notice-hint {
  opacity: 0.8;
  font-size: 13px;
}

.loading-state,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 48px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--md-sys-color-surface-container-highest);
  border-top: 3px solid var(--md-sys-color-primary);
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin-bottom: 16px;
}

@keyframes spin {
  0% {
    transform: rotate(0deg);
  }
  100% {
    transform: rotate(360deg);
  }
}

.error-state {
  color: var(--md-sys-color-error);
}

.error-state .material-symbols-rounded {
  font-size: 48px;
  margin-bottom: 16px;
}

.empty-state .material-symbols-rounded {
  font-size: 48px;
  margin-bottom: 16px;
  opacity: 0.6;
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

@media (hover: hover) {
  .filled-tonal-button:hover {
    background-color: color-mix(
      in srgb,
      var(--md-sys-color-on-surface) 8%,
      var(--md-sys-color-secondary-container)
    );
  }
}

.retry-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 24px;
  border: none;
  border-radius: 20px;
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

@media (hover: hover) {
  .retry-button:hover {
    box-shadow: var(--md-sys-elevation-level1);
  }
}

.material-symbols-rounded {
  font-size: 20px;
}

@media (max-width: 768px) {
  .device-name {
    max-width: 200px;
  }

  .content-header {
    flex-direction: column;
    align-items: flex-start;
    gap: 16px;
  }
}
</style>
