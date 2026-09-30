<template>
  <div class="audio-device-settings">
    <div class="content-header">
      <h3>{{ $t('config.audioDeviceSettings') }}</h3>
      <!-- Android 上没有可切换的输出设备，刷新按钮只会刷出一个「系统默认」，意义不大 -->
      <button v-if="!isAndroidPlatform" class="filled-tonal-button" @click="refreshDevices">
        <span class="material-symbols-rounded">refresh</span>
        {{ $t('config.refreshDevices') }}
      </button>
    </div>

    <!-- Android：输出路由由系统统一管理（扬声器/蓝牙/有线耳机自动切换），后端也只上报默认设备，
         选择没有意义，直接不展示列表 -->
    <div v-if="isAndroidPlatform" class="capability-notice platform-notice">
      <span class="material-symbols-rounded">speaker_phone</span>
      <p>{{ $t('config.audioDeviceManagedBySystem') }}</p>
    </div>

    <!-- Android：输出路由 + USB DAC 独占（对应 Windows 的 WASAPI 独占） -->
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
          <!-- 不绑定 update 事件: 点击冒泡给上方整行,走 toggleExclusiveMode 命令;
               设备不支持时仅视觉变暗 (class),平台不支持时才真正禁用 -->
          <SettingSwitch
            :model-value="useExclusiveMode"
            :disabled="!isWindowsPlatform"
            :class="{ disabled: currentDevice != null && !currentDevice.supportsExclusiveMode }"
          />
        </div>
      </div>

      <!-- Android：USB DAC 独占（位完美）。与 Windows 的 WASAPI 独占是同一类
           诉求，区别在于安卓上设备由系统路由，只在检测到 USB 音频设备时才有意义 -->
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

      <!-- 平台不支持独占模式提示（Android 有自己的 USB DAC 通道，不算不支持） -->
      <div
        v-if="!isWindowsPlatform && !isAndroidPlatform"
        class="capability-notice platform-notice"
      >
        <span class="material-symbols-rounded">desktop_windows</span>
        <p>{{ $t('config.exclusiveModePlatformNotSupported') }}</p>
      </div>

      <!-- Android：未插 DAC / 独占没能开成 -->
      <div v-if="isAndroidPlatform && !audioRoute?.usbConnected" class="capability-notice">
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

      <!-- 设备能力提示 -->
      <div
        v-else-if="currentDevice && !currentDevice.supportsExclusiveMode && useExclusiveMode"
        class="capability-notice"
      >
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeNotSupported') }}</p>
      </div>

      <!-- 低延迟模式说明 -->
      <div v-if="isWindowsPlatform && useExclusiveMode" class="capability-notice">
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeWarning') }}</p>
      </div>

      <!-- 淡入淡出开关 -->
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

// 状态管理
const audioDevices = ref<AudioDevice[]>([])
const currentDevice = ref<AudioDevice | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const restartRequired = ref(false)
const useExclusiveMode = ref(false)
const fadeEnabled = ref(true)
const currentPlatform = ref<string>('unknown')

/** Android 输出路由快照；非安卓平台保持 null（后端在非安卓上返回错误） */
const audioRoute = ref<AudioRouteInfo | null>(null)
const usbDacExclusiveEnabled = ref(false)

// 平台检测
const isWindowsPlatform = computed(() => {
  return currentPlatform.value === 'windows'
})

// Android 上输出设备由系统接管（见模板说明），不提供设备列表
const isAndroidPlatform = computed(() => {
  return currentPlatform.value === 'android'
})

/** 把设备类型名映射成一句人话（未接 USB 时按主要类型猜一个） */
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

/** 读取输出路由。非安卓平台后端会报错，静默忽略即可 */
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
    await setUsbDacExclusive(next, playerStore.currentTime)
    usbDacExclusiveEnabled.value = next
    configStore.setAudioConfig({ usbDacExclusive: next })
    await fetchAudioRoute()
  } catch (err) {
    logger.error('切换 USB DAC 独占失败:', err)
    error.value = getErrorMessage(err, 'Failed to toggle USB DAC exclusive mode')
  }
}

/** USB DAC 插拔由后端广播；在设置页打开时顺手刷新一次 */
let unlistenRoute: UnlistenFn | null = null
// onMounted 里有一串 await，listen() 完成时组件可能已卸载，需要事后自查
let mountedCleanupDone = false

// 获取音频设备列表
const fetchAudioDevices = async (): Promise<void> => {
  loading.value = true
  error.value = null

  try {
    const devices = await getAudioDevices()
    audioDevices.value = devices

    // 获取当前设备
    const current = await getCurrentAudioDevice()
    currentDevice.value = current
  } catch (err) {
    logger.error('Failed to fetch audio devices:', err)
    error.value = getErrorMessage(err, 'Unknown error')
  } finally {
    loading.value = false
  }
}

// 选择并切换音频设备
const selectDevice = async (device: AudioDevice): Promise<void> => {
  if (currentDevice.value?.name === device.name) {
    return // 已经是当前设备，无需切换
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

// 检查是否需要重启以应用独占模式设置
const checkRestartRequired = async (): Promise<void> => {
  try {
    const activeExclusiveMode = await getExclusiveMode()
    // 如果当前活跃状态与 store 中的意向状态不一致，则需要重启
    restartRequired.value = activeExclusiveMode !== useExclusiveMode.value
  } catch (err) {
    logger.error('Failed to check active exclusive mode:', err)
  }
}

// 切换独占模式
const toggleExclusiveMode = async (): Promise<void> => {
  // 在非 Windows 平台上阻止启用独占模式
  if (!isWindowsPlatform.value && !useExclusiveMode.value) {
    logger.warn('Exclusive mode is only supported on Windows')
    return
  }

  // 检查当前设备是否支持独占模式
  if (
    currentDevice.value &&
    !currentDevice.value.supportsExclusiveMode &&
    !useExclusiveMode.value
  ) {
    // 尝试启用但不支持的设备，显示警告但仍然执行
    logger.warn('Trying to enable exclusive mode on unsupported device')
  }

  try {
    await toggleExclusiveModeCommand(!useExclusiveMode.value, playerStore.currentTime)

    // 如果成功返回（说明切换到了当前已生效的状态），更新状态并清除提示
    useExclusiveMode.value = !useExclusiveMode.value
    restartRequired.value = false

    // 重新获取当前设备信息以更新状态
    try {
      const updatedDevice = await getCurrentAudioDevice()
      currentDevice.value = updatedDevice
    } catch (deviceErr) {
      logger.error('Failed to update current device info:', deviceErr)
    }
  } catch (err) {
    const errorMessage = getErrorMessage(err, String(err))

    // 检查是否是需要重启的提示
    if (errorMessage.includes('RESTART_REQUIRED')) {
      // 更新本地状态以反映配置已更改
      useExclusiveMode.value = !useExclusiveMode.value
      // 显示需要重启的提示
      restartRequired.value = true
    } else {
      logger.error('Failed to toggle exclusive mode:', err)
      error.value = errorMessage || 'Failed to toggle exclusive mode'
    }
  }
}

// 切换淡入淡出
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

// 刷新设备列表
const refreshDevices = (): void => {
  void fetchAudioDevices()
  void fetchAudioRoute()
}

// 组件挂载时获取设备列表
onMounted(async () => {
  // 获取平台信息
  try {
    currentPlatform.value = await getPlatform()
    logger.debug('Detected platform:', currentPlatform.value)
  } catch (err) {
    logger.error('Failed to detect platform:', err)
    currentPlatform.value = 'unknown'
  }

  // 优先从 store 获取独占模式设置
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

  // 加载淡入淡出设置
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

  // 检查是否需要重启提示
  await checkRestartRequired()

  // 获取音频设备时不重新加载配置，避免重置主题
  await fetchAudioDevices()

  // 获取当前设备信息
  try {
    currentDevice.value = await getCurrentAudioDevice()
  } catch (err) {
    logger.error('Failed to get current audio device:', err)
  }

  // Android：输出路由 + USB DAC 独占状态
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

// 监听当前设备变化
watch(currentDevice, (newDevice: AudioDevice | null) => {
  if (newDevice) {
    logger.debug('Audio device changed to:', newDevice.name, 'Mode:', newDevice.audioModeStatus)
  }
})

// 保存配置
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

.device-item:hover {
  background-color: var(--md-sys-color-surface-container-high);
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

/* Android 输出路由摘要 */
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
  /* 设备名可能很长（USB 产品名），优先截断而不是撑破面板 */
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

.option-item.disabled:hover {
  background-color: transparent;
}

.option-item:hover {
  background-color: var(--md-sys-color-surface-container);
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

/* 开关视觉由 SettingSwitch 组件提供;此处仅保留设备不支持时的变暗态 */
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

.filled-tonal-button:hover {
  background-color: color-mix(
    in srgb,
    var(--md-sys-color-on-surface) 8%,
    var(--md-sys-color-secondary-container)
  );
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

.retry-button:hover {
  box-shadow: var(--md-sys-elevation-level1);
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
