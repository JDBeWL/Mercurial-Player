import { beforeEach, describe, expect, it, vi } from 'vitest'

// setup.ts 的全局 mock 只提供 invoke;mediaService 还会 re-export convertFileSrc,
// 这里文件级覆盖以补齐导出。
// vi.mock 工厂会被提升到文件顶部,mockInvoke 必须经 vi.hoisted 同步提升。
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
  convertFileSrc: (path: string) => `asset://localhost/${encodeURIComponent(path)}`,
}))

import {
  getAudioDevices,
  getCurrentAudioDevice,
  getExclusiveMode,
  getFadeEnabled,
  setAudioDevice,
  setFadeEnabled,
  toggleExclusiveMode,
} from '@/services/audioService'
import {
  applyEqPreset,
  getEqBands,
  getEqPresets,
  getEqSettings,
  resetEq,
  setEqBandGain,
  setEqEnabled,
  setEqPreamp,
} from '@/services/eqService'
import {
  cleanCoverCache,
  clearMetadataCache,
  getMetadataCacheStats,
  getTempDir,
  getTrackCoverPath,
  setCoverCachePath,
  convertFileSrc,
} from '@/services/mediaService'
import {
  clearFontCaches,
  getFontCacheStats,
  getPlatform,
  getScreenRefreshRate,
  getSystemFonts,
  openExternalUrl,
  setTargetFps,
  setVerticalSync,
} from '@/services/appService'

beforeEach(() => {
  invokeMock.mockClear()
  invokeMock.mockResolvedValue(undefined)
})

/** 断言服务封装转发了正确的命令与参数(无参数命令只断言命令名) */
async function expectInvoke(fn: () => Promise<unknown>, command: string, args?: Record<string, unknown>) {
  await fn()
  if (args === undefined) {
    expect(invokeMock).toHaveBeenCalledWith(command)
  } else {
    expect(invokeMock).toHaveBeenCalledWith(command, args)
  }
}

describe('audioService', () => {
  it('wraps audio commands with correct payloads', async () => {
    await expectInvoke(() => getAudioDevices(), 'get_audio_devices')
    await expectInvoke(() => getCurrentAudioDevice(), 'get_current_audio_device')
    await expectInvoke(
      () => setAudioDevice('Speakers', 12.5),
      'set_audio_device',
      { deviceName: 'Speakers', currentTime: 12.5 },
    )
    await expectInvoke(() => getExclusiveMode(), 'get_exclusive_mode')
    await expectInvoke(
      () => toggleExclusiveMode(true, 3),
      'toggle_exclusive_mode',
      { enabled: true, currentTime: 3 },
    )
    await expectInvoke(() => setFadeEnabled(true), 'set_fade_enabled', { enabled: true })
    await expectInvoke(() => getFadeEnabled(), 'get_fade_enabled')
  })
})

describe('eqService', () => {
  it('wraps EQ commands with correct payloads', async () => {
    await expectInvoke(() => getEqBands(), 'get_eq_bands')
    await expectInvoke(() => getEqSettings(), 'get_eq_settings')
    await expectInvoke(() => getEqPresets(), 'get_eq_presets')
    await expectInvoke(() => setEqEnabled(true), 'set_eq_enabled', { enabled: true })
    await expectInvoke(
      () => setEqBandGain(3, -4.5),
      'set_eq_band_gain',
      { band: 3, gain: -4.5 },
    )
    await expectInvoke(() => setEqPreamp(-6), 'set_eq_preamp', { preamp: -6 })
    await expectInvoke(() => applyEqPreset('Rock'), 'apply_eq_preset', { presetName: 'Rock' })
    await expectInvoke(() => resetEq(), 'reset_eq')
  })
})

describe('mediaService', () => {
  it('wraps media commands with correct payloads', async () => {
    await expectInvoke(
      () => getTrackCoverPath('C:/music/a.mp3'),
      'get_track_cover_path',
      { path: 'C:/music/a.mp3' },
    )
    await expectInvoke(() => getTempDir(), 'get_temp_dir_command')
    await expectInvoke(() => getMetadataCacheStats(), 'get_metadata_cache_stats_command')
    await expectInvoke(() => clearMetadataCache(), 'clear_metadata_cache_command')
    await expectInvoke(
      () => cleanCoverCache(512),
      'clean_cover_cache_command',
      { maxCacheSizeMb: 512 },
    )
    await expectInvoke(
      () => setCoverCachePath(null),
      'set_cover_cache_path_command',
      { path: null },
    )
  })

  it('re-exports convertFileSrc for path conversion', () => {
    expect(convertFileSrc('C:/music/cover.jpg')).toContain('cover.jpg')
  })
})

describe('appService', () => {
  it('wraps app commands with correct payloads', async () => {
    await expectInvoke(() => getPlatform(), 'get_platform')
    await expectInvoke(() => getSystemFonts(), 'get_system_fonts')
    await expectInvoke(() => getFontCacheStats(), 'get_font_cache_stats')
    await expectInvoke(() => clearFontCaches(), 'clear_font_caches')
    await expectInvoke(() => getScreenRefreshRate(), 'get_screen_refresh_rate')
    await expectInvoke(() => setTargetFps(120), 'set_target_fps', { fps: 120 })
    await expectInvoke(() => setVerticalSync(true), 'set_vertical_sync', { enabled: true })
    await expectInvoke(
      () => openExternalUrl('https://example.com'),
      'open_external_url',
      { url: 'https://example.com' },
    )
  })
})
