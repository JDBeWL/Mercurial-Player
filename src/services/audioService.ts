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

/** 切换音频设备（携带当前播放进度，便于后端无缝续播）。@param remember 是否记住该设备选择(落盘
 *  preferredDeviceId)。设置页主动选择默认 true;设备拔出自动回退 / 跟随系统默认切换应传 false,
 *  避免覆盖用户选择。 */
export function setAudioDevice(
  deviceName: string,
  currentTime: number,
  remember = true,
): Promise<void> {
  return invoke<void>('set_audio_device', { deviceName, currentTime, remember })
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

// ============================================================================
// Android：USB DAC 独占（位完美）
// ============================================================================

/** 单个输出设备的摘要（Android，来自 AudioManager.getDevices） */
export interface OutputDevice {
  id: number
  name: string
  typeName: string
  isUsb: boolean
  sampleRates: number[]
  channelCounts: number[]
  encodings: number[]
}

/** 当前输出路由快照（Android） */
export interface AudioRouteInfo {
  usbConnected: boolean
  usbDeviceName: string
  exclusiveEnabled: boolean
  exclusiveActive: boolean
  sampleRate: number
  channels: number
  devices: OutputDevice[]
}

/** 读取当前输出路由；非 Android 平台会返回错误，调用方需 catch */
export function getAudioRoute(): Promise<AudioRouteInfo> {
  return invoke<AudioRouteInfo>('get_audio_route')
}

/** 开关 USB DAC 独占（位完美）输出；currentTime 用于切换后从原位置续播 */
export function setUsbDacExclusive(enabled: boolean, currentTime: number): Promise<void> {
  return invoke<void>('set_usb_dac_exclusive', { enabled, currentTime })
}
