<template>
  <div class="audio-device-settings">
    <div class="content-header">
      <h3>{{ $t('config.audioDeviceSettings') }}</h3>
      <button @click="refreshDevices" class="filled-tonal-button">
        <span class="material-symbols-rounded">refresh</span>
        {{ $t('config.refreshDevices') }}
      </button>
    </div>
    
    <div class="device-list" v-if="audioDevices.length > 0">
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
          <span v-else class="material-symbols-rounded">
            radio_button_unchecked
          </span>
        </div>
      </div>
    </div>

    <div class="audio-options">
      <div class="option-item" :class="{ 'disabled': !isWindowsPlatform }" @click="isWindowsPlatform && toggleExclusiveMode()">
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
          <div class="switch" :class="{ 'active': useExclusiveMode, 'disabled': !isWindowsPlatform || (currentDevice && !currentDevice.supportsExclusiveMode) }">
            <div class="switch-handle"></div>
          </div>
        </div>
      </div>
      
      <!-- 平台不支持独占模式提示 -->
      <div v-if="!isWindowsPlatform" class="capability-notice platform-notice">
        <span class="material-symbols-rounded">desktop_windows</span>
        <p>{{ $t('config.exclusiveModePlatformNotSupported') }}</p>
      </div>

      <!-- 设备能力提示 -->
      <div v-else-if="currentDevice && !currentDevice.supportsExclusiveMode && useExclusiveMode" class="capability-notice">
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeNotSupported') }}</p>
      </div>

      <!-- 低延迟模式说明 -->
      <div v-if="isWindowsPlatform && useExclusiveMode" class="capability-notice">
        <span class="material-symbols-rounded">info</span>
        <p>{{ $t('config.exclusiveModeWarning') }}</p>
      </div>
    </div>
    
    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>{{ $t('config.loadingDevices') }}</p>
    </div>
    
    <div v-if="error === 'restart_required'" class="restart-notice">
      <span class="material-symbols-rounded">restart_alt</span>
      <div class="notice-content">
        <p>{{ $t('config.exclusiveModeRestartRequired') }}</p>
        <p class="notice-hint">{{ $t('config.exclusiveModeRestartHint') }}</p>
      </div>
    </div>
    
    <div v-else-if="error" class="error-state">
      <span class="material-symbols-rounded">error</span>
      <p>{{ $t('config.deviceLoadError') }}: {{ error }}</p>
      <button @click="refreshDevices" class="retry-button">
        {{ $t('config.retry') }}
      </button>
    </div>
    
    <div v-if="audioDevices.length === 0 && !loading && !error" class="empty-state">
      <span class="material-symbols-rounded">speaker</span>
      <p>{{ $t('config.noAudioDevices') }}</p>
      <button @click="refreshDevices" class="refresh-button">
        {{ $t('config.refresh') }}
      </button>
    </div>
    

  </div>
</template>

<script setup>
import { ref, onMounted, watch, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from '../stores/player';
import { useConfigStore } from '../stores/config';
import logger from '../utils/logger';

const playerStore = usePlayerStore();
const configStore = useConfigStore();

// 状态管理
const audioDevices = ref([]);
const currentDevice = ref(null);
const loading = ref(false);
const error = ref(null);
const useExclusiveMode = ref(false);
const currentPlatform = ref('unknown');

// 平台检测
const isWindowsPlatform = computed(() => {
  return currentPlatform.value === 'windows';
});

// 获取音频设备列表
const fetchAudioDevices = async () => {
  loading.value = true;
  error.value = null;
  
  try {
    const devices = await invoke('get_audio_devices');
    audioDevices.value = devices;
    
    // 获取当前设备
    const current = await invoke('get_current_audio_device');
    currentDevice.value = current;
  } catch (err) {
    logger.error('Failed to fetch audio devices:', err);
    error.value = err.message || 'Unknown error';
  } finally {
    loading.value = false;
  }
};

// 选择并切换音频设备
const selectDevice = async (device) => {
  if (currentDevice.value?.name === device.name) {
    return; // 已经是当前设备，无需切换
  }
  
  loading.value = true;
  error.value = null;
  
  try {
    await invoke('set_audio_device', { 
      deviceName: device.name,
      currentTime: playerStore.currentTime,
    });
    currentDevice.value = device;
  } catch (err) {
    logger.error('Failed to set audio device:', err);
    error.value = err.message || 'Unknown error';
  } finally {
    loading.value = false;
  }
};

// 切换独占模式
const toggleExclusiveMode = async () => {
  // 在非 Windows 平台上阻止启用独占模式
  if (!isWindowsPlatform.value && !useExclusiveMode.value) {
    logger.warn('Exclusive mode is only supported on Windows');
    return;
  }

  // 检查当前设备是否支持独占模式
  if (currentDevice.value && !currentDevice.value.supportsExclusiveMode && !useExclusiveMode.value) {
    // 尝试启用但不支持的设备，显示警告但仍然执行
    logger.warn('Trying to enable exclusive mode on unsupported device');
  }

  try {
    await invoke('toggle_exclusive_mode', {
      enabled: !useExclusiveMode.value,
      currentTime: playerStore.currentTime,
    });
    useExclusiveMode.value = !useExclusiveMode.value;

    // 重新获取当前设备信息以更新状态
    try {
      const updatedDevice = await invoke('get_current_audio_device');
      currentDevice.value = updatedDevice;
    } catch (deviceErr) {
      logger.error('Failed to update current device info:', deviceErr);
    }
  } catch (err) {
    const errorMessage = err.message || err.toString() || '';

    // 检查是否是需要重启的提示
    if (errorMessage.includes('RESTART_REQUIRED')) {
      // 更新本地状态以反映配置已更改
      useExclusiveMode.value = !useExclusiveMode.value;
      // 显示需要重启的提示
      error.value = 'restart_required';
    } else {
      logger.error('Failed to toggle exclusive mode:', err);
      error.value = errorMessage || 'Failed to toggle exclusive mode';
    }
  }
};

// 刷新设备列表
const refreshDevices = () => {
  fetchAudioDevices();
};

// 组件挂载时获取设备列表
onMounted(async () => {
  // 获取平台信息
  try {
    currentPlatform.value = await invoke('get_platform');
    logger.debug('Detected platform:', currentPlatform.value);
  } catch (err) {
    logger.error('Failed to detect platform:', err);
    currentPlatform.value = 'unknown';
  }

  // 尝试从后端加载独占模式状态，如果失败则使用配置中的值
  try {
    useExclusiveMode.value = await invoke('get_exclusive_mode') ?? configStore.audio?.exclusiveMode ?? false;
  } catch (err) {
    logger.warn('Failed to get exclusive mode from backend, using config value:', err);
    useExclusiveMode.value = configStore.audio?.exclusiveMode ?? false;
  }

  // 获取音频设备时不重新加载配置，避免重置主题
  await fetchAudioDevices();

  // 获取当前设备信息
  try {
    currentDevice.value = await invoke('get_current_audio_device');
  } catch (err) {
    logger.error('Failed to get current audio device:', err);
  }
});

// 监听当前设备变化
watch(currentDevice, (newDevice) => {
  if (newDevice) {
    logger.debug('Audio device changed to:', newDevice.name, 'Mode:', newDevice.audioModeStatus);
  }
});

// 保存配置
watch(useExclusiveMode, (newValue) => {
  configStore.setAudioConfig({ exclusiveMode: newValue });
});
</script>

<style scoped>
.audio-device-settings {
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
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
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

/* MD3 Switch */
.option-control .switch {
  position: relative;
  width: 52px;
  height: 32px;
  flex-shrink: 0;
}

.option-control .switch::before {
  content: '';
  position: absolute;
  width: 100%;
  height: 100%;
  background-color: var(--md-sys-color-surface-container-highest);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 16px;
  transition: all 0.2s ease;
  box-sizing: border-box;
}

.option-control .switch.active::before {
  background-color: var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.option-control .switch.disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.option-control .switch-handle {
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

.option-control .switch.active .switch-handle {
  left: 22px;
  width: 24px;
  height: 24px;
  background-color: var(--md-sys-color-on-primary);
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
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
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
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-secondary-container));
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