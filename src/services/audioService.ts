import { invoke } from '@tauri-apps/api/core'

// 音频设备信息(由后端 get_audio_devices / get_current_audio_device 返回)
export interface AudioDevice {
  name: string
  isDefault: boolean
  supportsExclusiveMode: boolean
  audioModeStatus: string
}

/** 获取系统音频设备列表 */
export function getAudioDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>('get_audio_devices')
}

/** 获取当前正在使用的音频设备 */
export function getCurrentAudioDevice(): Promise<AudioDevice> {
  return invoke<AudioDevice>('get_current_audio_device')
}

/** 切换音频设备（携带当前播放进度，便于后端无缝续播） */
export function setAudioDevice(deviceName: string, currentTime: number): Promise<void> {
  return invoke<void>('set_audio_device', { deviceName, currentTime })
}

/** 获取独占模式是否已生效 */
export function getExclusiveMode(): Promise<boolean> {
  return invoke<boolean>('get_exclusive_mode')
}

/** 切换独占模式（携带当前播放进度） */
export function toggleExclusiveMode(enabled: boolean, currentTime: number): Promise<void> {
  return invoke<void>('toggle_exclusive_mode', { enabled, currentTime })
}

/** 设置淡入淡出开关 */
export function setFadeEnabled(enabled: boolean): Promise<void> {
  return invoke<void>('set_fade_enabled', { enabled })
}

/** 获取淡入淡出开关状态 */
export function getFadeEnabled(): Promise<boolean> {
  return invoke<boolean>('get_fade_enabled')
}
