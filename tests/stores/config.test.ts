// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

// Mock theme store to avoid pulling @material/material-color-utilities into the test.
const themeSetPreferenceMock = vi.fn()
vi.mock('@/stores/theme', () => ({
  useThemeStore: vi.fn(() => ({
    themePreference: 'auto',
    setThemePreference: themeSetPreferenceMock,
  })),
}))

// Mock musicLibrary store to avoid recursive store initialization.
vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: vi.fn(() => ({
    musicFolders: [],
    currentPlaylist: null,
    playlists: [],
  })),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

import { setActivePinia, createPinia } from 'pinia'
import { useConfigStore } from '@/stores/config'
import { invoke } from '@tauri-apps/api/core'

const invokeMock = vi.mocked(invoke)

describe('useConfigStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    vi.useFakeTimers()
    invokeMock.mockReset()
    invokeMock.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.clearAllTimers()
    vi.useRealTimers()
    vi.clearAllMocks()
  })

  // ---------- default state ----------

  describe('default state', () => {
    it('has empty musicDirectories by default', () => {
      const store = useConfigStore()
      expect(store.musicDirectories).toEqual([])
    })

    it('has default directoryScan config', () => {
      const store = useConfigStore()
      expect(store.directoryScan.enableSubdirectoryScan).toBe(true)
      expect(store.directoryScan.maxDepth).toBe(3)
      expect(store.directoryScan.ignoreHiddenFolders).toBe(true)
      expect(store.directoryScan.folderBlacklist).toContain('node_modules')
      expect(store.directoryScan.folderBlacklist).toContain('.git')
    })

    it('has default titleExtraction config', () => {
      const store = useConfigStore()
      expect(store.titleExtraction.preferMetadata).toBe(true)
      expect(store.titleExtraction.separator).toBe('-')
      expect(store.titleExtraction.hideFileExtension).toBe(true)
      expect(store.titleExtraction.parseArtistTitle).toBe(true)
      expect(store.titleExtraction.customSeparators).toEqual(['-', '_', '.'])
    })

    it('has default playlist config', () => {
      const store = useConfigStore()
      expect(store.playlist.generateAllSongsPlaylist).toBe(true)
      expect(store.playlist.folderBasedPlaylists).toBe(true)
      expect(store.playlist.playlistNameFormat).toBe('{folderName}')
      expect(store.playlist.sortOrder).toBe('asc')
    })

    it('has default general config', () => {
      const store = useConfigStore()
      expect(store.general.language).toBe('zh')
      expect(store.general.theme).toBe('auto')
      expect(store.general.startupLoadLastConfig).toBe(true)
      expect(store.general.autoSaveConfig).toBe(true)
      expect(store.general.showAudioInfo).toBe(true)
      expect(store.general.showQueueInfo).toBe(true)
      expect(store.general.enableAutoUpdate).toBe(false)
      expect(store.general.coverCacheSizeMb).toBe(1024)
    })

    it('has default lyrics config', () => {
      const store = useConfigStore()
      expect(store.lyrics.enableOnlineFetch).toBe(false)
      expect(store.lyrics.autoSaveOnlineLyrics).toBe(true)
      expect(store.lyrics.preferTranslation).toBe(true)
      expect(store.lyrics.onlineSource).toBe('netease')
      expect(store.lyrics.lyricsAlignment).toBe('center')
      expect(store.lyrics.lyricsFontFamily).toBe('Noto Sans SC')
      expect(store.lyrics.lyricsStyle).toBe('modern')
    })

    it('has default desktopLyrics config', () => {
      const store = useConfigStore()
      expect(store.lyrics.desktopLyrics).toBeDefined()
      expect(store.lyrics.desktopLyrics?.enabled).toBe(false)
      expect(store.lyrics.desktopLyrics?.locked).toBe(true)
      expect(store.lyrics.desktopLyrics?.fontSize).toBe(28)
      expect(store.lyrics.desktopLyrics?.colorPreset).toBe('auto')
    })

    it('has default audio config', () => {
      const store = useConfigStore()
      expect(store.audio.exclusiveMode).toBe(false)
      expect(store.audio.volume).toBe(0.5)
      expect(store.audio.fadeEnabled).toBe(true)
    })

    it('has default ui config', () => {
      const store = useConfigStore()
      expect(store.ui.showSettings).toBe(false)
      expect(store.ui.showConfigPanel).toBe(false)
      expect(store.ui.miniMode).toBe(false)
    })

    it('has default visualizer config', () => {
      const store = useConfigStore()
      expect(store.visualizer.targetFps).toBe(60)
      expect(store.visualizer.enableVerticalSync).toBe(false)
    })

    it('starts not dirty with no pending save', () => {
      const store = useConfigStore()
      expect(store._isDirty).toBe(false)
      expect(store.hasUnsavedChanges).toBe(false)
      expect(store._isInitializing).toBe(false)
      expect(store._savePromise).toBeNull()
      expect(store._lastSavedConfig).toBeNull()
    })
  })

  // ---------- getters ----------

  describe('getters', () => {
    it('availableSeparators merges separator and customSeparators uniquely', () => {
      const store = useConfigStore()
      // defaults: separator='-', customSeparators=['-','_','.']
      const seps = store.availableSeparators
      expect(seps).toContain('-')
      expect(seps).toContain('_')
      expect(seps).toContain('.')
      // unique
      expect(new Set(seps).size).toBe(seps.length)
    })

    it('validSeparators filters out empty / whitespace-only entries', () => {
      const store = useConfigStore()
      store.titleExtraction.customSeparators = ['-', '', '  ', '_']
      // availableSeparators = [...new Set(['-', '-', '', '  ', '_'])] = ['-', '', '  ', '_']
      // validSeparators filters falsy/whitespace -> ['-', '_']
      expect(store.validSeparators).toEqual(['-', '_'])
    })

    it('hasUnsavedChanges reflects _isDirty', () => {
      const store = useConfigStore()
      expect(store.hasUnsavedChanges).toBe(false)
      store._isDirty = true
      expect(store.hasUnsavedChanges).toBe(true)
    })
  })

  // ---------- setAudioConfig ----------

  describe('setAudioConfig', () => {
    it('merges partial audio config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setAudioConfig({ volume: 0.8 })
      expect(store.audio.volume).toBe(0.8)
      expect(store.audio.exclusiveMode).toBe(false)
      expect(store.audio.fadeEnabled).toBe(true)
    })

    it('marks store dirty', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      expect(store._isDirty).toBe(false)
      store.setAudioConfig({ exclusiveMode: true })
      expect(store._isDirty).toBe(true)
    })
  })

  // ---------- setLyricsConfig ----------

  describe('setLyricsConfig', () => {
    it('merges partial lyrics config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setLyricsConfig({ lyricsAlignment: 'left', onlineSource: 'qq' })
      expect(store.lyrics.lyricsAlignment).toBe('left')
      expect(store.lyrics.onlineSource).toBe('qq')
      expect(store.lyrics.enableOnlineFetch).toBe(false)
      expect(store.lyrics.lyricsFontFamily).toBe('Noto Sans SC')
    })
  })

  // ---------- setDesktopLyricsConfig ----------

  describe('setDesktopLyricsConfig', () => {
    it('merges partial desktop lyrics config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setDesktopLyricsConfig({ enabled: true, fontSize: 36 })
      expect(store.lyrics.desktopLyrics?.enabled).toBe(true)
      expect(store.lyrics.desktopLyrics?.fontSize).toBe(36)
      expect(store.lyrics.desktopLyrics?.locked).toBe(true)
    })

    it('initializes desktopLyrics when missing', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      // Force-clear the sub-config to simulate legacy state.
      store.lyrics.desktopLyrics = undefined as unknown as NonNullable<
        typeof store.lyrics.desktopLyrics
      >
      store.setDesktopLyricsConfig({ enabled: true })
      expect(store.lyrics.desktopLyrics).toBeDefined()
      expect(store.lyrics.desktopLyrics?.enabled).toBe(true)
      expect(store.lyrics.desktopLyrics?.colorPreset).toBe('auto')
    })
  })

  // ---------- setDirectoryScanConfig ----------

  describe('setDirectoryScanConfig', () => {
    it('merges partial directory scan config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setDirectoryScanConfig({ maxDepth: 5 })
      expect(store.directoryScan.maxDepth).toBe(5)
      expect(store.directoryScan.enableSubdirectoryScan).toBe(true)
      expect(store.directoryScan.ignoreHiddenFolders).toBe(true)
    })
  })

  // ---------- setTitleExtractionConfig ----------

  describe('setTitleExtractionConfig', () => {
    it('merges partial title extraction config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setTitleExtractionConfig({ separator: '_', preferMetadata: false })
      expect(store.titleExtraction.separator).toBe('_')
      expect(store.titleExtraction.preferMetadata).toBe(false)
      expect(store.titleExtraction.hideFileExtension).toBe(true)
    })
  })

  // ---------- setPlaylistConfig ----------

  describe('setPlaylistConfig', () => {
    it('merges partial playlist config', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setPlaylistConfig({ sortOrder: 'desc' })
      expect(store.playlist.sortOrder).toBe('desc')
      expect(store.playlist.generateAllSongsPlaylist).toBe(true)
    })
  })

  // ---------- setGeneralConfig (incl. theme) ----------

  describe('setGeneralConfig', () => {
    it('merges partial general config including theme', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setGeneralConfig({ theme: 'dark', language: 'en' })
      expect(store.general.theme).toBe('dark')
      expect(store.general.language).toBe('en')
    })

    it('preserves unspecified general fields', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setGeneralConfig({ theme: '#abcdef' })
      expect(store.general.theme).toBe('#abcdef')
      expect(store.general.language).toBe('zh')
      expect(store.general.showAudioInfo).toBe(true)
    })

    it('marks dirty', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setGeneralConfig({ theme: 'light' })
      expect(store._isDirty).toBe(true)
    })
  })

  // ---------- toggleSortOrder ----------

  describe('toggleSortOrder', () => {
    it('toggles asc -> desc -> asc', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      expect(store.playlist.sortOrder).toBe('asc')
      store.toggleSortOrder()
      expect(store.playlist.sortOrder).toBe('desc')
      store.toggleSortOrder()
      expect(store.playlist.sortOrder).toBe('asc')
    })
  })

  // ---------- resetToDefaults ----------

  describe('resetToDefaults', () => {
    it('resets all state to defaults', () => {
      const store = useConfigStore()
      store.general.autoSaveConfig = false
      store.setAudioConfig({ volume: 0.9, exclusiveMode: true })
      store.setGeneralConfig({ theme: 'dark', language: 'en' })
      store.resetToDefaults()
      expect(store.audio.volume).toBe(0.5)
      expect(store.audio.exclusiveMode).toBe(false)
      expect(store.general.theme).toBe('auto')
      expect(store.general.language).toBe('zh')
    })

    it('marks store dirty after reset', () => {
      const store = useConfigStore()
      store._isDirty = false
      store.resetToDefaults()
      expect(store._isDirty).toBe(true)
    })
  })

  // ---------- UI methods ----------

  describe('UI methods', () => {
    it('openSettings / closeSettings / toggleSettings', () => {
      const store = useConfigStore()
      expect(store.ui.showSettings).toBe(false)
      store.openSettings()
      expect(store.ui.showSettings).toBe(true)
      store.closeSettings()
      expect(store.ui.showSettings).toBe(false)
      store.toggleSettings()
      expect(store.ui.showSettings).toBe(true)
      store.toggleSettings()
      expect(store.ui.showSettings).toBe(false)
    })

    it('openConfigPanel / closeConfigPanel / toggleConfigPanel', () => {
      const store = useConfigStore()
      expect(store.ui.showConfigPanel).toBe(false)
      store.openConfigPanel()
      expect(store.ui.showConfigPanel).toBe(true)
      store.closeConfigPanel()
      expect(store.ui.showConfigPanel).toBe(false)
      store.toggleConfigPanel()
      expect(store.ui.showConfigPanel).toBe(true)
    })
  })

  // ---------- markInitializationComplete / _markDirty ----------

  describe('markInitializationComplete', () => {
    it('clears _isInitializing and _isDirty', () => {
      const store = useConfigStore()
      store._isInitializing = true
      store._isDirty = true
      store.markInitializationComplete()
      expect(store._isInitializing).toBe(false)
      expect(store._isDirty).toBe(false)
    })
  })

  describe('_markDirty', () => {
    it('does not mark dirty while initializing', () => {
      const store = useConfigStore()
      store._isInitializing = true
      store._markDirty()
      expect(store._isDirty).toBe(false)
    })

    it('marks dirty when not initializing', () => {
      const store = useConfigStore()
      store._isInitializing = false
      store._markDirty()
      expect(store._isDirty).toBe(true)
    })
  })

  // ---------- _getSaveableConfig ----------

  describe('_getSaveableConfig', () => {
    it('excludes internal state fields', () => {
      const store = useConfigStore()
      const config = store._getSaveableConfig() as unknown as Record<string, unknown>
      expect(config).not.toHaveProperty('_isInitializing')
      expect(config).not.toHaveProperty('_isDirty')
      expect(config).not.toHaveProperty('_lastSavedConfig')
      expect(config).not.toHaveProperty('_savePromise')
    })

    it('forces showSettings / showConfigPanel to false', () => {
      const store = useConfigStore()
      store.ui.showSettings = true
      store.ui.showConfigPanel = true
      const config = store._getSaveableConfig()
      expect(config.ui?.showSettings).toBe(false)
      expect(config.ui?.showConfigPanel).toBe(false)
    })

    it('keeps miniMode as-is', () => {
      const store = useConfigStore()
      store.ui.miniMode = true
      const config = store._getSaveableConfig()
      expect(config.ui?.miniMode).toBe(true)
    })
  })

  // ---------- _hasRealChanges ----------

  describe('_hasRealChanges', () => {
    it('returns true when _lastSavedConfig is null', () => {
      const store = useConfigStore()
      store._lastSavedConfig = null
      expect(store._hasRealChanges()).toBe(true)
    })

    it('returns false when config equals last saved', () => {
      const store = useConfigStore()
      // JSON.stringify drops `undefined` values (e.g. general.coverCachePath),
      // which would make the snapshot differ from the live state. Set it to a
      // defined value so the round-trip clone matches.
      store.general.coverCachePath = '/cache'
      const snapshot = store._getSaveableConfig()
      store._lastSavedConfig = JSON.parse(JSON.stringify(snapshot))
      expect(store._hasRealChanges()).toBe(false)
    })

    it('returns true when config differs from last saved', () => {
      const store = useConfigStore()
      const snapshot = store._getSaveableConfig()
      store._lastSavedConfig = JSON.parse(JSON.stringify(snapshot))
      store.audio.volume = 0.9
      expect(store._hasRealChanges()).toBe(true)
    })
  })

  // ---------- toggleMiniMode ----------

  describe('toggleMiniMode', () => {
    it('toggles mini mode on when invoke succeeds', async () => {
      invokeMock.mockResolvedValue(undefined)
      const store = useConfigStore()
      expect(store.ui.miniMode).toBe(false)
      await store.toggleMiniMode()
      expect(store.ui.miniMode).toBe(true)
      expect(invokeMock).toHaveBeenCalledWith('set_mini_mode', { enable: true })
    })

    it('toggles back off on second call', async () => {
      invokeMock.mockResolvedValue(undefined)
      const store = useConfigStore()
      await store.toggleMiniMode()
      await store.toggleMiniMode()
      expect(store.ui.miniMode).toBe(false)
      expect(invokeMock).toHaveBeenCalledWith('set_mini_mode', { enable: false })
    })

    it('does not toggle when invoke fails', async () => {
      invokeMock.mockRejectedValue(new Error('backend failed'))
      const store = useConfigStore()
      expect(store.ui.miniMode).toBe(false)
      await store.toggleMiniMode()
      expect(store.ui.miniMode).toBe(false)
    })
  })

  // ---------- saveConfigNow / flushPendingSave ----------

  describe('saveConfigNow', () => {
    it('skips saving when there are no real changes', async () => {
      const store = useConfigStore()
      // Set coverCachePath to a defined value so JSON-clone matches live state.
      store.general.coverCachePath = '/cache'
      const snapshot = store._getSaveableConfig()
      store._lastSavedConfig = JSON.parse(JSON.stringify(snapshot))
      await store.saveConfigNow()
      expect(invokeMock).not.toHaveBeenCalledWith('save_config', expect.anything())
    })

    it('invokes save_config when there are changes', async () => {
      const store = useConfigStore()
      store._lastSavedConfig = null // force real changes
      store.audio.volume = 0.9
      await store.saveConfigNow()
      expect(invokeMock).toHaveBeenCalledWith(
        'save_config',
        expect.objectContaining({ config: expect.any(Object) }),
      )
      // After save: _isDirty cleared, _lastSavedConfig set
      expect(store._isDirty).toBe(false)
      expect(store._lastSavedConfig).not.toBeNull()
    })
  })

  describe('flushPendingSave', () => {
    it('cancels pending debounced save and saves immediately', async () => {
      const store = useConfigStore()
      store._lastSavedConfig = null
      await store.flushPendingSave()
      expect(invokeMock).toHaveBeenCalledWith(
        'save_config',
        expect.objectContaining({ config: expect.any(Object) }),
      )
    })
  })

  // ---------- exportConfig / importConfig ----------

  describe('exportConfig', () => {
    it('invokes export_config with config and filePath', async () => {
      invokeMock.mockResolvedValue(undefined)
      const store = useConfigStore()
      await store.exportConfig('/path/to/config.json')
      expect(invokeMock).toHaveBeenCalledWith(
        'export_config',
        expect.objectContaining({
          filePath: '/path/to/config.json',
          config: expect.any(Object),
        }),
      )
    })

    it('throws when invoke fails', async () => {
      invokeMock.mockRejectedValue(new Error('export failed'))
      const store = useConfigStore()
      await expect(store.exportConfig('/path/to/config.json')).rejects.toThrow()
    })
  })

  describe('importConfig', () => {
    it('patches state with imported config and marks dirty', async () => {
      invokeMock.mockResolvedValue({
        audio: { exclusiveMode: true, volume: 0.3, fadeEnabled: false },
      })
      const store = useConfigStore()
      await store.importConfig('/path/to/config.json')
      expect(store.audio.exclusiveMode).toBe(true)
      expect(store.audio.volume).toBe(0.3)
      expect(store.audio.fadeEnabled).toBe(false)
      expect(store._isDirty).toBe(true)
    })

    it('throws when invoke fails', async () => {
      invokeMock.mockRejectedValue(new Error('import failed'))
      const store = useConfigStore()
      await expect(store.importConfig('/path/to/config.json')).rejects.toThrow()
    })
  })
})
