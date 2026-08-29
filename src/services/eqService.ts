import { invoke } from '@tauri-apps/api/core'

// EQ 频段信息(由后端 get_eq_bands 返回)
export interface EqBandInfo {
  index: number
  frequency: number
  label: string
}

// EQ 设置(由后端 get_eq_settings 返回)
export interface EqSettings {
  enabled: boolean
  gains: number[]
  preamp: number
}

// EQ 预设(由后端 get_eq_presets 返回)
export interface EqPreset {
  name: string
  gains: number[]
}

/** 获取 EQ 频段列表 */
export function getEqBands(): Promise<EqBandInfo[]> {
  return invoke<EqBandInfo[]>('get_eq_bands')
}

/** 获取 EQ 设置（开关、各频段增益、前置增益） */
export function getEqSettings(): Promise<EqSettings> {
  return invoke<EqSettings>('get_eq_settings')
}

/** 获取 EQ 预设列表 */
export function getEqPresets(): Promise<EqPreset[]> {
  return invoke<EqPreset[]>('get_eq_presets')
}

/** 设置 EQ 启用状态 */
export function setEqEnabled(enabled: boolean): Promise<void> {
  return invoke<void>('set_eq_enabled', { enabled })
}

/** 设置指定频段增益 */
export function setEqBandGain(band: number, gain: number): Promise<void> {
  return invoke<void>('set_eq_band_gain', { band, gain })
}

/** 设置前置增益 */
export function setEqPreamp(preamp: number): Promise<void> {
  return invoke<void>('set_eq_preamp', { preamp })
}

/** 应用指定名称的 EQ 预设 */
export function applyEqPreset(presetName: string): Promise<void> {
  return invoke<void>('apply_eq_preset', { presetName })
}

/** 重置 EQ（频段增益、前置增益归零，关闭 EQ） */
export function resetEq(): Promise<void> {
  return invoke<void>('reset_eq')
}
