// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { fetch as tauriFetch } from '@tauri-apps/plugin-http'
import { PluginPermission, type PluginPermissionType } from '@/plugins/pluginTypes'

const stores = vi.hoisted(() => ({
  player: {} as Record<string, unknown>,
  musicLibrary: {} as Record<string, unknown>,
  theme: {} as Record<string, unknown>,
}))

vi.mock('@/utils/logger', () => ({
  default: { info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn() },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: { findLyricsFile: vi.fn() },
}))

vi.mock('@/utils/lyricsParser', () => ({
  findLyricIndex: vi.fn(() => 1),
}))

vi.mock('@/stores/player', () => ({ usePlayerStore: () => stores.player }))
vi.mock('@/stores/musicLibrary', () => ({ useMusicLibraryStore: () => stores.musicLibrary }))
vi.mock('@/stores/theme', () => ({ useThemeStore: () => stores.theme }))

const { createPluginAPI } = await import('@/plugins/pluginAPI')
const FileUtils = (await import('@/utils/fileUtils')).default
const { findLyricIndex } = await import('@/utils/lyricsParser')
const logger = (await import('@/utils/logger')).default
// showNotification 走真实 composable（模块级单例），断言其共享状态
const { useErrorNotification } = await import('@/composables/useErrorNotification')

const ALL_PERMISSIONS = Object.values(PluginPermission) as PluginPermissionType[]

type Extensions = {
  settingsPanels: unknown[]
  menuItems: unknown[]
  playerDecorators: unknown[]
  actionButtons: Record<string, unknown>[]
  lyricsProviders: unknown[]
  visualizers: unknown[]
  commands: Record<string, unknown>[]
  shortcuts: Record<string, unknown>[]
}

/** 构造一个记录所有调用的 PluginManager 替身 */
function createMockManager() {
  const extensions: Extensions = {
    settingsPanels: [],
    menuItems: [],
    playerDecorators: [],
    actionButtons: [],
    lyricsProviders: [],
    visualizers: [],
    commands: [],
    shortcuts: [],
  }
  const storage: Record<string, unknown> = {}
  const emitted: { event: string; data?: unknown }[] = []
  const listeners: Record<string, unknown[]> = {}

  return {
    extensions,
    storage,
    emitted,
    listeners,
    registerExtension: vi.fn((type: keyof Extensions, pluginId: string, item: unknown) => {
      ;(extensions[type] as Record<string, unknown>[]).push({
        ...(item as Record<string, unknown>),
        pluginId,
      })
    }),
    getExtensions: vi.fn((type: keyof Extensions) => extensions[type] as never),
    getStorage: vi.fn(() => storage),
    on: vi.fn((event: string, pluginId: string, callback: unknown) => {
      ;(listeners[event] ??= []).push({ pluginId, callback })
    }),
    off: vi.fn((event: string, _pluginId: string, callback: unknown) => {
      listeners[event] = (listeners[event] ?? []).filter(
        (l) => (l as { callback: unknown }).callback !== callback,
      )
    }),
    emit: vi.fn((event: string, data?: unknown) => {
      emitted.push({ event, data })
    }),
  }
}

type Manager = ReturnType<typeof createMockManager>

const api = (
  permissions: PluginPermissionType[] = ALL_PERMISSIONS,
  manager: Manager = createMockManager(),
) => createPluginAPI('demo-plugin', permissions, manager as never)

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(invoke).mockResolvedValue(null)
  Object.assign(stores.player, {
    currentTrack: null,
    isPlaying: false,
    currentTime: 0,
    duration: 0,
    volume: 0.5,
    repeatMode: 'none',
    isShuffle: false,
    lyrics: [],
    lyricsOffset: 0,
    playlist: [],
    play: vi.fn(),
    pause: vi.fn(),
    togglePlay: vi.fn(),
    nextTrack: vi.fn().mockResolvedValue(undefined),
    previousTrack: vi.fn().mockResolvedValue(undefined),
    seek: vi.fn(),
    setVolume: vi.fn(),
    loadLyrics: vi.fn().mockResolvedValue(undefined),
  })
  Object.assign(stores.musicLibrary, { playlists: [], currentPlaylist: null })
  Object.assign(stores.theme, {
    themePreference: 'dark',
    isDark: true,
    primaryColor: '#123456',
  })
})

describe('createPluginAPI - 基础结构', () => {
  it('exposes the plugin id and a readonly permission list', () => {
    const pluginApi = api([PluginPermission.PLAYER_READ])
    expect(pluginApi.pluginId).toBe('demo-plugin')
    expect(pluginApi.permissions).toEqual(['player:read'])
  })

  it('routes log helpers through the logger with a plugin prefix', () => {
    const pluginApi = api()
    pluginApi.log.info('a')
    pluginApi.log.warn('b')
    pluginApi.log.error('c')
    pluginApi.log.debug('d')

    expect(logger.info).toHaveBeenCalledWith('[Plugin:demo-plugin]', 'a')
    expect(logger.warn).toHaveBeenCalledWith('[Plugin:demo-plugin]', 'b')
    expect(logger.error).toHaveBeenCalledWith('[Plugin:demo-plugin]', 'c')
    expect(logger.debug).toHaveBeenCalledWith('[Plugin:demo-plugin]', 'd')
  })

  it('throws a permission error naming the plugin, permission and action', () => {
    const pluginApi = api([])
    expect(() => pluginApi.player.getState()).toThrow(
      '插件 demo-plugin 没有 player:read 权限，无法执行 player.getState',
    )
  })
})

describe('player API', () => {
  it('returns a deep copy of the current state', () => {
    stores.player.currentTrack = { path: '/a.mp3', title: 'A' }
    stores.player.isPlaying = true
    stores.player.currentTime = 12
    stores.player.duration = 200
    stores.player.volume = 0.8
    stores.player.repeatMode = 'list'
    stores.player.isShuffle = true

    const state = api().player.getState()

    expect(state).toEqual({
      currentTrack: { path: '/a.mp3', title: 'A' },
      isPlaying: true,
      currentTime: 12,
      duration: 200,
      volume: 0.8,
      repeatMode: 'list',
      isShuffle: true,
    })
    // 深拷贝:修改返回值不应影响 store
    state.currentTrack!.title = 'mutated'
    expect((stores.player.currentTrack as { title: string }).title).toBe('A')
  })

  it('returns a null currentTrack when nothing is playing', () => {
    expect(api().player.getState().currentTrack).toBeNull()
  })

  it('delegates playback controls to the store', async () => {
    const pluginApi = api()
    pluginApi.player.play()
    pluginApi.player.pause()
    pluginApi.player.togglePlay()
    pluginApi.player.seek(30)
    pluginApi.player.setVolume(0.2)
    await pluginApi.player.next()
    await pluginApi.player.previous()

    expect(stores.player.play).toHaveBeenCalled()
    expect(stores.player.pause).toHaveBeenCalled()
    expect(stores.player.togglePlay).toHaveBeenCalled()
    expect(stores.player.seek).toHaveBeenCalledWith(30)
    expect(stores.player.setVolume).toHaveBeenCalledWith(0.2)
    expect(stores.player.nextTrack).toHaveBeenCalled()
    expect(stores.player.previousTrack).toHaveBeenCalled()
  })

  it('guards every control behind PLAYER_CONTROL', async () => {
    const pluginApi = api([PluginPermission.PLAYER_READ])
    expect(() => pluginApi.player.play()).toThrow(/player:control/)
    expect(() => pluginApi.player.pause()).toThrow(/player:control/)
    expect(() => pluginApi.player.togglePlay()).toThrow(/player:control/)
    expect(() => pluginApi.player.seek(1)).toThrow(/player:control/)
    expect(() => pluginApi.player.setVolume(1)).toThrow(/player:control/)
    // next/previous 为 async,权限不足时以 rejected Promise 呈现
    await expect(pluginApi.player.next()).rejects.toThrow(/player:control/)
    await expect(pluginApi.player.previous()).rejects.toThrow(/player:control/)
  })

  describe('getLyrics', () => {
    it('returns the cached lyrics converted to the plugin shape', async () => {
      stores.player.lyrics = [{ time: 1, texts: ['one', 'two'] }]
      const lyrics = await api().player.getLyrics()
      expect(lyrics).toEqual([{ time: 1, texts: [{ text: 'one' }, { text: 'two' }] }])
    })

    it('falls back to line.text when texts is empty', async () => {
      stores.player.lyrics = [{ time: 2, text: 'plain', texts: [] }]
      const lyrics = await api().player.getLyrics()
      expect(lyrics?.[0]?.texts).toEqual([{ text: 'plain' }])
    })

    it('coerces non-string entries in texts', async () => {
      stores.player.lyrics = [{ time: 0, texts: [42] as never }]
      const lyrics = await api().player.getLyrics()
      expect(lyrics?.[0]?.texts).toEqual([{ text: '42' }])
    })

    it('ignores a non-array texts field', async () => {
      stores.player.lyrics = [{ time: 0, texts: 'nope' as never }]
      const lyrics = await api().player.getLyrics()
      expect(lyrics?.[0]?.texts).toEqual([])
    })

    it('returns null when there is no track', async () => {
      await expect(api().player.getLyrics()).resolves.toBeNull()
    })

    it('returns null when no lyrics file exists', async () => {
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(FileUtils.findLyricsFile).mockResolvedValue(null)

      await expect(api().player.getLyrics()).resolves.toBeNull()
    })

    it('returns null when the lyrics lookup throws', async () => {
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(FileUtils.findLyricsFile).mockRejectedValue(new Error('io'))

      await expect(api().player.getLyrics()).resolves.toBeNull()
      expect(logger.error).toHaveBeenCalled()
    })

    it('triggers a store load and polls for up to a second', async () => {
      vi.useFakeTimers()
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(FileUtils.findLyricsFile).mockResolvedValue('/a.lrc')
      vi.mocked(stores.player.loadLyrics as never as ReturnType<typeof vi.fn>).mockImplementation(
        async () => {
          stores.player.lyrics = [{ time: 1, text: 'loaded' }]
        },
      )

      const promise = api().player.getLyrics()
      const result = await vi.advanceTimersByTimeAsync(0).then(() => promise)
      vi.useRealTimers()

      expect(result).toEqual([{ time: 1, texts: [{ text: 'loaded' }], text: 'loaded' }])
    })

    it('gives up after ten polls when the store never fills the lyrics', async () => {
      vi.useFakeTimers()
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(FileUtils.findLyricsFile).mockResolvedValue('/a.lrc')

      const promise = api().player.getLyrics()
      const result = await vi.advanceTimersByTimeAsync(1_500).then(() => promise)
      vi.useRealTimers()

      expect(result).toBeNull()
    })

    it('returns null when the store load rejects', async () => {
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(FileUtils.findLyricsFile).mockResolvedValue('/a.lrc')
      vi.mocked(stores.player.loadLyrics as never as ReturnType<typeof vi.fn>).mockRejectedValue(
        new Error('boom'),
      )

      await expect(api().player.getLyrics()).resolves.toBeNull()
      expect(logger.error).toHaveBeenCalled()
    })
  })

  describe('getCurrentLyricIndex', () => {
    it('returns -1 without lyrics', () => {
      expect(api().player.getCurrentLyricIndex()).toBe(-1)
    })

    it('applies the lyrics offset and the 50ms lead', () => {
      stores.player.lyrics = [{ time: 1 }]
      stores.player.currentTime = 10
      stores.player.lyricsOffset = 2

      expect(api().player.getCurrentLyricIndex()).toBe(1)
      expect(findLyricIndex).toHaveBeenCalledWith(stores.player.lyrics, 8.05)
    })

    it('uses a zero offset by default', () => {
      stores.player.lyrics = [{ time: 1 }]
      stores.player.currentTime = 5
      ;(stores.player as { lyricsOffset?: number }).lyricsOffset = undefined

      api().player.getCurrentLyricIndex()
      expect(findLyricIndex).toHaveBeenCalledWith(stores.player.lyrics, 5.05)
    })
  })

  describe('setLyrics', () => {
    it('converts the plugin shape into the store shape', () => {
      const pluginApi = api()
      pluginApi.player.setLyrics([
        { time: 1, texts: [{ text: 'a' }, { text: 'b' }] },
        { time: 2, texts: [] },
      ] as never)

      expect(stores.player.lyrics).toEqual([
        { time: 1, texts: ['a', 'b'] },
        { time: 2, texts: [] },
      ])
    })

    it('tolerates a missing texts field', () => {
      api().player.setLyrics([{ time: 3 }] as never)
      expect(stores.player.lyrics).toEqual([{ time: 3, texts: [] }])
    })

    it('requires LYRICS_PROVIDER', () => {
      expect(() => api([]).player.setLyrics([])).toThrow(/lyrics:provider/)
    })
  })

  describe('getCoverPath', () => {
    it('returns the cached cover path when present', async () => {
      stores.player.currentTrack = { path: '/a.mp3', coverPath: '/cover/a.jpg' }
      await expect(api().player.getCoverPath()).resolves.toBe('/cover/a.jpg')
      expect(invoke).not.toHaveBeenCalled()
    })

    it('asks the backend when the store has no cover', async () => {
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(invoke).mockResolvedValue('/cover/b.jpg')

      await expect(api().player.getCoverPath()).resolves.toBe('/cover/b.jpg')
      expect(invoke).toHaveBeenCalledWith('get_track_cover_path', { path: '/a.mp3' })
    })

    it('returns null without a current track', async () => {
      await expect(api().player.getCoverPath()).resolves.toBeNull()
      expect(invoke).not.toHaveBeenCalled()
    })

    it('returns null and logs when the backend fails', async () => {
      stores.player.currentTrack = { path: '/a.mp3' }
      vi.mocked(invoke).mockRejectedValue(new Error('backend'))

      await expect(api().player.getCoverPath()).resolves.toBeNull()
      expect(logger.error).toHaveBeenCalled()
    })
  })
})

describe('library API', () => {
  it('maps playlists using the name as the id', () => {
    stores.musicLibrary.playlists = [
      { name: 'Fav', files: [{ path: '/a.mp3' }] },
      { name: 'Empty' },
    ]

    const playlists = api().library.getPlaylists()

    expect(playlists).toEqual([
      { id: 'Fav', name: 'Fav', tracks: [{ path: '/a.mp3' }] },
      { id: 'Empty', name: 'Empty', tracks: [] },
    ])
  })

  it('returns an empty list when the store has no playlists', () => {
    ;(stores.musicLibrary as { playlists?: unknown }).playlists = undefined
    expect(api().library.getPlaylists()).toEqual([])
  })

  it('returns a copy of the tracks so plugins cannot mutate the library', () => {
    stores.musicLibrary.playlists = [{ name: 'Fav', files: [{ path: '/a.mp3' }] }]
    const [playlist] = api().library.getPlaylists()
    playlist!.tracks![0]!.path = '/mutated.mp3'
    expect(
      (stores.musicLibrary.playlists as { files: { path: string }[] }[])[0]!.files[0]!.path,
    ).toBe('/a.mp3')
  })

  it('returns the current playlist or null', () => {
    expect(api().library.getCurrentPlaylist()).toBeNull()

    stores.musicLibrary.currentPlaylist = { name: 'Now', files: [{ path: '/b.mp3' }] }
    expect(api().library.getCurrentPlaylist()).toEqual({
      id: 'Now',
      name: 'Now',
      tracks: [{ path: '/b.mp3' }],
    })
  })

  it('returns the player playlist as tracks', () => {
    stores.player.playlist = [{ path: '/a.mp3' }, { path: '/b.mp3' }]
    expect(api().library.getTracks()).toEqual([{ path: '/a.mp3' }, { path: '/b.mp3' }])
  })

  it('returns an empty track list when the playlist is null', () => {
    ;(stores.player as { playlist?: unknown }).playlist = null
    expect(api().library.getTracks()).toEqual([])
  })

  it('guards every method behind LIBRARY_READ', () => {
    const pluginApi = api([])
    expect(() => pluginApi.library.getPlaylists()).toThrow(/library:read/)
    expect(() => pluginApi.library.getCurrentPlaylist()).toThrow(/library:read/)
    expect(() => pluginApi.library.getTracks()).toThrow(/library:read/)
  })
})

describe('theme API', () => {
  it('reports the current theme without requiring a permission', () => {
    expect(api([]).theme.getCurrent()).toEqual({
      preference: 'dark',
      isDark: true,
      primaryColor: '#123456',
    })
  })

  it('writes namespaced CSS variables', async () => {
    await api().theme.setColors({ accent: '#ff0000', bg: '#000000' })
    const root = document.documentElement
    expect(root.style.getPropertyValue('--plugin-demo-plugin-accent')).toBe('#ff0000')
    expect(root.style.getPropertyValue('--plugin-demo-plugin-bg')).toBe('#000000')
  })

  it('requires THEME for setColors', async () => {
    await expect(api([]).theme.setColors({ a: '#fff' })).rejects.toThrow(/theme/)
  })

  it('reads a CSS variable with or without the leading dashes', () => {
    document.documentElement.style.setProperty('--md-sys-color-primary', '#abcdef')
    const pluginApi = api()
    expect(pluginApi.theme.getCSSVariable('--md-sys-color-primary')).toBe('#abcdef')
    expect(pluginApi.theme.getCSSVariable('md-sys-color-primary')).toBe('#abcdef')
  })

  it('collects every defined MD3 colour into camelCase keys', () => {
    document.documentElement.style.setProperty('--md-sys-color-primary', '#111111')
    document.documentElement.style.setProperty('--md-sys-color-on-primary', '#222222')
    document.documentElement.style.setProperty('--md-sys-color-surface', '#333333')

    const colors = api().theme.getAllColors()

    expect(colors['mdSysColorPrimary']).toBe('#111111')
    expect(colors['mdSysColorOnPrimary']).toBe('#222222')
    expect(colors['mdSysColorSurface']).toBe('#333333')
  })

  it('returns an empty object when no MD3 colour is defined', () => {
    document.documentElement.removeAttribute('style')
    const colors = api().theme.getAllColors()
    expect(Object.values(colors).every((v) => typeof v === 'string')).toBe(true)
  })
})

describe('ui API', () => {
  it('registers a settings panel, menu item and player decorator', () => {
    const manager = createMockManager()
    const pluginApi = api(ALL_PERMISSIONS, manager)

    pluginApi.ui.registerSettingsPanel({ id: 'p' } as never)
    pluginApi.ui.registerMenuItem({ id: 'm' } as never)
    pluginApi.ui.registerPlayerDecorator({ id: 'd' } as never)

    expect(manager.registerExtension).toHaveBeenCalledWith('settingsPanels', 'demo-plugin', {
      id: 'p',
    })
    expect(manager.registerExtension).toHaveBeenCalledWith('menuItems', 'demo-plugin', { id: 'm' })
    expect(manager.registerExtension).toHaveBeenCalledWith('playerDecorators', 'demo-plugin', {
      id: 'd',
    })
  })

  it('rejects an incomplete action button', () => {
    const manager = createMockManager()
    expect(() =>
      api(ALL_PERMISSIONS, manager).ui.registerActionButton({ id: 'b' } as never),
    ).toThrow('按钮必须包含 id, name, icon 和 action')
    expect(manager.registerExtension).not.toHaveBeenCalled()
  })

  it('defaults an action button location to lyrics', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).ui.registerActionButton({
      id: 'b',
      name: 'B',
      icon: 'i',
      action: vi.fn(),
    } as never)

    expect(manager.extensions.actionButtons[0]).toMatchObject({ location: 'lyrics' })
    expect(logger.info).toHaveBeenCalledWith('操作按钮已注册: B')
  })

  it('keeps an explicit action button location', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).ui.registerActionButton({
      id: 'b',
      name: 'B',
      icon: 'i',
      action: vi.fn(),
      location: 'toolbar',
    } as never)

    expect(manager.extensions.actionButtons[0]).toMatchObject({ location: 'toolbar' })
  })

  it('unregisters only its own action buttons', () => {
    const manager = createMockManager()
    manager.extensions.actionButtons.push(
      { id: 'mine', pluginId: 'demo-plugin' },
      { id: 'theirs', pluginId: 'other-plugin' },
      { id: 'mine', pluginId: 'other-plugin' },
    )

    api(ALL_PERMISSIONS, manager).ui.unregisterActionButton('mine')

    expect(manager.extensions.actionButtons.map((b) => b.pluginId)).toEqual([
      'other-plugin',
      'other-plugin',
    ])
    expect(logger.info).toHaveBeenCalledWith('操作按钮已取消: mine')
  })

  it('is a no-op when unregistering an unknown button', () => {
    const manager = createMockManager()
    expect(() => api(ALL_PERMISSIONS, manager).ui.unregisterActionButton('nope')).not.toThrow()
  })

  it('shows a notification through the shared notification bus', () => {
    const { errorNotifications, clearErrors } = useErrorNotification()
    clearErrors()

    const pluginApi = api()
    pluginApi.ui.showNotification('hello')
    pluginApi.ui.showNotification('bad', 'error')

    expect(errorNotifications.value.map((n) => n.message)).toEqual(['hello', 'bad'])
    expect(errorNotifications.value.map((n) => n.severity)).toEqual(['info', 'error'])
    clearErrors()
  })

  it('guards UI extension registration behind UI_EXTEND', () => {
    const pluginApi = api([])
    expect(() => pluginApi.ui.registerSettingsPanel({ id: 'p' } as never)).toThrow(/ui:extend/)
    expect(() => pluginApi.ui.registerMenuItem({ id: 'm' } as never)).toThrow(/ui:extend/)
    expect(() => pluginApi.ui.registerPlayerDecorator({ id: 'd' } as never)).toThrow(/ui:extend/)
    expect(() =>
      pluginApi.ui.registerActionButton({ id: 'b', name: 'B', icon: 'i', action: vi.fn() } as never),
    ).toThrow(/ui:extend/)
    expect(() => pluginApi.ui.unregisterActionButton('b')).toThrow(/ui:extend/)
  })
})

describe('lyrics / visualizer API', () => {
  it('registers a valid lyrics provider', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).lyrics.registerProvider({
      id: 'lp',
      name: 'LP',
      search: vi.fn(),
    } as never)

    expect(manager.registerExtension).toHaveBeenCalledWith(
      'lyricsProviders',
      'demo-plugin',
      expect.objectContaining({ id: 'lp' }),
    )
    expect(logger.info).toHaveBeenCalledWith('歌词源已注册: LP')
  })

  it('rejects a provider without a search function', () => {
    expect(() => api().lyrics.registerProvider({ id: 'lp', name: 'LP' } as never)).toThrow(
      '歌词源必须包含 id, name 和 search 方法',
    )
  })

  it('requires LYRICS_PROVIDER', () => {
    expect(() =>
      api([]).lyrics.registerProvider({ id: 'lp', name: 'LP', search: vi.fn() } as never),
    ).toThrow(/lyrics:provider/)
  })

  it('registers a valid visualizer', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).visualizer.register({
      id: 'v',
      name: 'V',
      render: vi.fn(),
    } as never)

    expect(manager.registerExtension).toHaveBeenCalledWith(
      'visualizers',
      'demo-plugin',
      expect.objectContaining({ id: 'v' }),
    )
    expect(logger.info).toHaveBeenCalledWith('可视化效果已注册: V')
  })

  it('rejects a visualizer without a render function', () => {
    expect(() => api().visualizer.register({ id: 'v', name: 'V' } as never)).toThrow(
      '可视化效果必须包含 id, name 和 render 方法',
    )
  })

  it('requires VISUALIZER', () => {
    expect(() =>
      api([]).visualizer.register({ id: 'v', name: 'V', render: vi.fn() } as never),
    ).toThrow(/visualizer/)
  })
})

describe('commands API', () => {
  it('registers a command and executes it by id', async () => {
    const manager = createMockManager()
    const execute = vi.fn()
    const pluginApi = api(ALL_PERMISSIONS, manager)

    pluginApi.commands.register({ id: 'c1', name: 'C1', execute } as never)
    await pluginApi.commands.execute('c1')

    expect(execute).toHaveBeenCalled()
  })

  it('rejects an incomplete command', () => {
    expect(() => api().commands.register({ id: 'c1' } as never)).toThrow(
      '命令必须包含 id, name 和 execute 方法',
    )
  })

  it('is a no-op when the command id is unknown', async () => {
    const manager = createMockManager()
    await expect(api(ALL_PERMISSIONS, manager).commands.execute('ghost')).resolves.toBeUndefined()
  })

  it('awaits an async command', async () => {
    const manager = createMockManager()
    let done = false
    api(ALL_PERMISSIONS, manager).commands.register({
      id: 'async',
      name: 'Async',
      execute: async () => {
        await Promise.resolve()
        done = true
      },
    } as never)

    await api(ALL_PERMISSIONS, manager).commands.execute('async')
    expect(done).toBe(true)
  })

  it('refuses to execute a command registered by another plugin', async () => {
    const manager = createMockManager()
    const foreignExecute = vi.fn()
    manager.extensions.commands.push({
      id: 'foreign',
      name: 'Foreign',
      execute: foreignExecute,
      pluginId: 'other-plugin',
    })

    await expect(
      api(ALL_PERMISSIONS, manager).commands.execute('foreign'),
    ).resolves.toBeUndefined()
    expect(foreignExecute).not.toHaveBeenCalled()
  })

  it('guards command registration behind UI_EXTEND', () => {
    const manager = createMockManager()
    expect(() =>
      api([], manager).commands.register({
        id: 'c1',
        name: 'C1',
        execute: vi.fn(),
      } as never),
    ).toThrow(/ui:extend/)
    expect(manager.registerExtension).not.toHaveBeenCalled()
  })
})

describe('shortcuts API', () => {
  it('normalizes the key to a lowercase canonical order', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).shortcuts.register({
      id: 's',
      name: 'S',
      key: 'ShiFt + Ctrl + K',
      action: vi.fn(),
    } as never)

    expect(manager.extensions.shortcuts[0]?.key).toBe('ctrl+shift+k')
  })

  it('keeps unknown modifiers after the known ones', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).shortcuts.register({
      id: 's',
      name: 'S',
      key: 'Z+Ctrl',
      action: vi.fn(),
    } as never)

    expect(manager.extensions.shortcuts[0]?.key).toBe('ctrl+z')
  })

  it('logs the registration with the original key', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).shortcuts.register({
      id: 's',
      name: 'Save',
      key: 'Ctrl+S',
      action: vi.fn(),
    } as never)

    expect(logger.info).toHaveBeenCalledWith('快捷键已注册: Save (Ctrl+S)')
  })

  it('rejects an incomplete shortcut', () => {
    expect(() => api().shortcuts.register({ id: 's', name: 'S' } as never)).toThrow(
      '快捷键必须包含 id, name, key 和 action',
    )
  })

  it('unregisters only its own shortcuts', () => {
    const manager = createMockManager()
    manager.extensions.shortcuts.push(
      { id: 's1', pluginId: 'demo-plugin' },
      { id: 's1', pluginId: 'other' },
      { id: 's2', pluginId: 'demo-plugin' },
    )

    api(ALL_PERMISSIONS, manager).shortcuts.unregister('s1')

    expect(manager.extensions.shortcuts.map((s) => s.id)).toEqual(['s1', 's2'])
    expect(logger.info).toHaveBeenCalledWith('快捷键已取消: s1')
  })

  it('is a no-op when unregistering an unknown shortcut', () => {
    const manager = createMockManager()
    expect(() => api(ALL_PERMISSIONS, manager).shortcuts.unregister('ghost')).not.toThrow()
  })

  it('rejects a key already bound by another shortcut', () => {
    const manager = createMockManager()
    manager.extensions.shortcuts.push({
      id: 's1',
      name: 'S1',
      key: 'ctrl+shift+k',
      pluginId: 'other',
    })

    expect(() =>
      api(ALL_PERMISSIONS, manager).shortcuts.register({
        id: 's2',
        name: 'S2',
        key: 'CTRL+SHIFT+K', // 大小写差异归一化后仍冲突
        action: vi.fn(),
      } as never),
    ).toThrow(/已被 S1 \(other\) 注册/)

    // 冲突注册被拒绝,不追加新条目
    expect(manager.extensions.shortcuts).toHaveLength(1)
  })

  it('replaces its own shortcut when re-registering the same id', () => {
    const manager = createMockManager()
    const pluginApi = api(ALL_PERMISSIONS, manager)

    pluginApi.shortcuts.register({
      id: 's',
      name: 'First',
      key: 'ctrl+k',
      action: vi.fn(),
    } as never)
    pluginApi.shortcuts.register({
      id: 's',
      name: 'Second',
      key: 'ctrl+j',
      action: vi.fn(),
    } as never)

    expect(manager.extensions.shortcuts).toHaveLength(1)
    expect(manager.extensions.shortcuts[0]?.name).toBe('Second')
    expect(manager.extensions.shortcuts[0]?.key).toBe('ctrl+j')
  })

  it('guards shortcut registration behind UI_EXTEND', () => {
    const manager = createMockManager()
    expect(() =>
      api([], manager).shortcuts.register({
        id: 's',
        name: 'S',
        key: 'Ctrl+K',
        action: vi.fn(),
      } as never),
    ).toThrow(/ui:extend/)
    expect(() => api([], manager).shortcuts.unregister('s')).toThrow(/ui:extend/)
    expect(manager.extensions.shortcuts).toHaveLength(0)
  })
})

describe('storage API', () => {
  it('reads, writes, removes and snapshots the plugin storage', () => {
    const manager = createMockManager()
    const pluginApi = api(ALL_PERMISSIONS, manager)

    expect(pluginApi.storage.get('missing', 'fallback')).toBe('fallback')
    pluginApi.storage.set('count', 3)
    expect(pluginApi.storage.get('count', 0)).toBe(3)
    expect(pluginApi.storage.getAll()).toEqual({ count: 3 })

    pluginApi.storage.remove('count')
    expect(pluginApi.storage.get('count', -1)).toBe(-1)
    expect(manager.storage).toEqual({})
  })

  it('returns a copy from getAll so plugins cannot mutate the store', () => {
    const manager = createMockManager()
    const pluginApi = api(ALL_PERMISSIONS, manager)
    pluginApi.storage.set('k', 'v')

    const all = pluginApi.storage.getAll()
    all.k = 'mutated'

    expect(manager.storage.k).toBe('v')
  })

  it('falls back to null when no default is given', () => {
    expect(api().storage.get('nope')).toBeNull()
  })

  it('guards every method behind STORAGE', () => {
    const pluginApi = api([])
    expect(() => pluginApi.storage.get('k')).toThrow(/storage/)
    expect(() => pluginApi.storage.set('k', 1)).toThrow(/storage/)
    expect(() => pluginApi.storage.remove('k')).toThrow(/storage/)
    expect(() => pluginApi.storage.getAll()).toThrow(/storage/)
  })
})

describe('events API', () => {
  it('namespaces emitted events with the plugin id', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).events.emit('ready', { ok: true })

    expect(manager.emit).toHaveBeenCalledWith('plugin:demo-plugin:ready', { ok: true })
  })

  it('emits without a payload', () => {
    const manager = createMockManager()
    api(ALL_PERMISSIONS, manager).events.emit('ping')
    expect(manager.emit).toHaveBeenCalledWith('plugin:demo-plugin:ping', undefined)
  })

  it('forwards on/off to the manager', () => {
    const manager = createMockManager()
    const pluginApi = api(ALL_PERMISSIONS, manager)
    const callback = vi.fn()

    pluginApi.events.on('plugin:other:tick', callback)
    expect(manager.on).toHaveBeenCalledWith('plugin:other:tick', 'demo-plugin', callback)
    pluginApi.events.off('plugin:other:tick', callback)
    expect(manager.off).toHaveBeenCalledWith('plugin:other:tick', 'demo-plugin', callback)
  })

  it('allows player events only with PLAYER_READ', () => {
    const manager = createMockManager()
    const callback = vi.fn()

    expect(() => api([], manager).events.on('player:trackChanged', callback)).toThrow(
      /player:read 权限，无法订阅事件 player:trackChanged/,
    )
    expect(manager.on).not.toHaveBeenCalled()

    api([PluginPermission.PLAYER_READ], manager).events.on('player:trackChanged', callback)
    expect(manager.on).toHaveBeenCalledWith('player:trackChanged', 'demo-plugin', callback)
  })

  it('rejects events outside the whitelist even with full permissions', () => {
    const manager = createMockManager()
    expect(() => api(ALL_PERMISSIONS, manager).events.on('secret', vi.fn())).toThrow(
      /未知插件事件: secret/,
    )
    expect(manager.on).not.toHaveBeenCalled()
  })
})

describe('network API', () => {
  it('adds the plugin header and forbids redirects', async () => {
    const response = { url: 'https://api.example/ok', ok: true }
    vi.mocked(tauriFetch).mockResolvedValue(response as never)

    const result = await api().network.fetch('https://api.example/ok', {
      headers: { 'X-Custom': '1' },
    })

    expect(result).toBe(response)
    expect(tauriFetch).toHaveBeenCalledWith(
      'https://api.example/ok',
      expect.objectContaining({
        headers: { 'X-Custom': '1', 'X-Plugin-Request': 'true' },
        redirect: 'manual',
      }),
    )
  })

  it('rejects a non-HTTPS url before issuing a request', async () => {
    await expect(api().network.fetch('http://insecure.example')).rejects.toThrow(
      '只允许 HTTPS 请求',
    )
    expect(tauriFetch).not.toHaveBeenCalled()
  })

  it('rejects a response that was redirected to a non-HTTPS url', async () => {
    vi.mocked(tauriFetch).mockResolvedValue({ url: 'http://evil.example' } as never)

    await expect(api().network.fetch('https://api.example')).rejects.toThrow(
      '请求被重定向到非HTTPS地址',
    )
    expect(logger.error).toHaveBeenCalled()
  })

  it('accepts a response without a url', async () => {
    vi.mocked(tauriFetch).mockResolvedValue({ ok: true } as never)
    await expect(api().network.fetch('https://api.example')).resolves.toEqual({ ok: true })
  })

  it('logs and rethrows transport failures', async () => {
    vi.mocked(tauriFetch).mockRejectedValue(new Error('offline'))
    await expect(api().network.fetch('https://api.example')).rejects.toThrow('offline')
    expect(logger.error).toHaveBeenCalled()
  })

  it('requires NETWORK', async () => {
    await expect(api([]).network.fetch('https://api.example')).rejects.toThrow(/network/)
  })
})
