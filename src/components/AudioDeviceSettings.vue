<template>
  <div class="audio-device-settings">
    <div class="settings-header">
      <h3>{{ $t('config.audioDeviceSettings') }}</h3>
      <button @click="refreshDevices" class="refresh-button">
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
          <span v-if="device.isDefault" class="device-badge default">
            {{ $t('config.defaultDevice') }}
          </span>
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
      <div class="option-item" @click="toggleExclusiveMode">
        <div class="option-label">
          <span class="material-symbols-rounded">album</span>
          <div class="option-text">
            <h4>{{ $t('config.exclusiveMode') }}</h4>
            <p>{{ $t('config.exclusiveModeDesc') }}</p>
          </div>
        </div>
        <div class="option-control">
          <div class="switch" :class="{ 'active': useExclusiveMode }">
            <div class="switch-handle"></div>
          </div>
        </div>
      </div>
    </div>
    
    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>{{ $t('config.loadingDevices') }}</p>
    </div>
    
    <div v-if="error" class="error-state">
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
import { ref, onMounted, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from '../stores/player';
import { useConfigStore } from '../stores/config';

const playerStore = usePlayerStore();
const configStore = useConfigStore();

// 状态管理
const audioDevices = ref([]);
const currentDevice = ref(null);
const loading = ref(false);
const error = ref(null);
const useExclusiveMode = ref(false);

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
    console.error('Failed to fetch audio devices:', err);
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
    console.error('Failed to set audio device:', err);
    error.value = err.message || 'Unknown error';
  } finally {
    loading.value = false;
  }
};

// 切换独占模式
const toggleExclusiveMode = async () => {
  try {
    await invoke('toggle_exclusive_mode', { 
      enabled: !useExclusiveMode.value,
      currentTime: playerStore.currentTime,
    });
    useExclusiveMode.value = !useExclusiveMode.value;
  } catch (err) {
    console.error('Failed to toggle exclusive mode:', err);
    error.value = err.message || 'Failed to toggle exclusive mode';
  }
};

// 刷新设备列表
const refreshDevices = () => {
  fetchAudioDevices();
};

// 组件挂载时获取设备列表
onMounted(async () => {
  // 尝试从后端加载独占模式状态，如果失败则使用配置中的值
  try {
    useExclusiveMode.value = await invoke('get_exclusive_mode') ?? configStore.audio?.exclusiveMode ?? false;
  } catch (err) {
    console.warn('Failed to get exclusive mode from backend, using config value:', err);
    useExclusiveMode.value = configStore.audio?.exclusiveMode ?? false;
  }
  
  // 获取音频设备时不重新加载配置，避免重置主题
  fetchAudioDevices();
});

// 监听当前设备变化
watch(currentDevice, (newDevice) => {
  if (newDevice) {
    console.log('Audio device changed to:', newDevice.name);
  }
});

// 保存配置
watch(useExclusiveMode, (newValue) => {
  configStore.setAudioConfig({ exclusiveMode: newValue });
});
</script>

<style scoped>
.audio-device-settings {
  padding: 16px;
  background-color: var(--md-sys-color-surface);
  border-radius: var(--md-sys-shape-corner-medium);
  height: 100%;
  display: flex;
  flex-direction: column;
}

.settings-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.settings-header h3 {
  margin: 0;
  color: var(--md-sys-color-on-surface);
  font-size: 1.25rem;
  font-weight: 500;
}

.device-list {
  flex: 1;
  overflow-y: auto;
  margin-bottom: 16px;
}

.device-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  margin-bottom: 8px;
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-surface-variant);
  cursor: pointer;
  transition: all 0.2s ease;
}

.device-item:hover {
  background-color: var(--md-sys-color-secondary-container);
}

.device-item.active {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.device-info {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  overflow: hidden;
}

.device-name {
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  flex: 1;
}

.device-badge {
  font-size: 0.75rem;
  padding: 2px 8px;
  border-radius: 12px;
  font-weight: 500;
  white-space: nowrap;
  flex-shrink: 0;
  margin-right: 8px;
}

.device-badge.default {
  background-color: var(--md-sys-color-tertiary-container);
  color: var(--md-sys-color-on-tertiary-container);
}

.device-item.active .device-badge.default {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.device-icon {
  color: var(--md-sys-color-on-surface-variant);
}

.device-item.active .device-icon {
  color: var(--md-sys-color-primary);
  display: flex;
  align-items: center;
  justify-content: center;
}

.audio-options {
  margin-top: 16px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
  padding-top: 16px;
}

.option-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-radius: var(--md-sys-shape-corner-small);
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.option-item:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.option-label {
  display: flex;
  align-items: center;
  gap: 16px;
}

.option-text h4 {
  margin: 0;
  font-size: 1rem;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.option-text p {
  margin: 0;
  font-size: 0.875rem;
  color: var(--md-sys-color-on-surface-variant);
}

.option-control .switch {
  width: 36px;
  height: 20px;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: 10px;
  position: relative;
  transition: background-color 0.2s ease;
}

.option-control .switch.active {
  background-color: var(--md-sys-color-primary);
}

.option-control .switch-handle {
  width: 16px;
  height: 16px;
  background-color: var(--md-sys-color-on-surface);
  border-radius: 50%;
  position: absolute;
  top: 2px;
  left: 2px;
  transition: transform 0.2s ease;
}

.option-control .switch.active .switch-handle {
  transform: translateX(16px);
  background-color: var(--md-sys-color-on-primary);
}

.loading-state,
.error-state,
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 24px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--md-sys-color-surface-variant);
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



.refresh-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border: none;
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: background-color 0.2s ease;
}

.refresh-button:hover {
  background-color: var(--md-sys-color-outline);
  color: var(--md-sys-color-on-primary-container);
}

.retry-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  border: none;
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
  cursor: pointer;
  transition: all 0.2s ease;
  font-family: inherit;
  font-size: 0.875rem;
}

.retry-button:hover {
  background-color: var(--md-sys-color-error-container-hover);
}

.material-symbols-rounded {
  font-size: 1.2rem;
}

/* 响应式设计 */
@media (max-width: 768px) {
  .device-name {
    max-width: 250px;
  }
}
</style>