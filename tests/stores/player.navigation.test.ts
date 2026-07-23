// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: vi.fn(),
  unregisterAll: vi.fn(),
  isRegistered: vi.fn(async () => false),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: {
    fileExists: vi.fn(async () => true),
    findLyricsFile: vi.fn(async () => null),
    readFile: vi.fn(async () => ''),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  default: {
    parseAsync: vi.fn(async () => []),
  },
}))

// Mock config store to avoid pulling plugin-store / theme dependencies into player tests.
vi.mock('@/stores/config', () => ({
  useConfigStore: vi.fn(() => ({
    audio: { volume: 0.5, exclusiveMode: false, fadeEnabled: true },
    saveConfigNow: vi.fn().mockResolvedValue(undefined),
  })),
}))

vi.mock('@/stores/musicLibrary', () => ({
  useMusicLibraryStore: vi.fn(() => ({
    currentPlaylist: null,
    playlists: [],
  })),
}))

import { setActivePinia, createPinia } from 'pinia'
import { usePlayerStore } from '@/stores/player'
import { invoke } from '@tauri-apps/api/core'
import FileUtils from '@/utils/fileUtils'
import type { Track } from '@/types'

const invokeMock = vi.mocked(invoke)
const fileExistsMock = vi.mocked(FileUtils.fileExists)

function makeTrack(path: string, name?: string): Track {
  return { path, name: name ?? path, title: name, duration: 100 }
}

function makePlaylist(n: number): Track[] {
  return Array.from({ length: n }, (_, i) => makeTrack(`/music/track${i}.mp3`, `Track${i}`))
}

/** Configure invoke to allow play_track / pause_track / cover lookup to succeed. */
function setupPlayInvokeMock() {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'pause_track') return Promise.resolve(undefined)
    if (cmd === 'play_track') return Promise.resolve(undefined)
    if (cmd === 'get_track_cover_path') return Promise.resolve(null)
    if (cmd === 'set_volume') return Promise.resolve(undefined)
    if (cmd === 'update_taskbar_state') return Promise.resolve(undefined)
    if (cmd === 'save_last_session') return Promise.resolve(undefined)
    return Promise.resolve(undefined)
  })
}

describe('usePlayerStore navigation & state', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined)
    fileExistsMock.mockResolvedValue(true)
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  // ---------- repeat mode ----------

  describe('toggleRepeat', () => {
    it('cycles none -> list -> track -> none', () => {
      const store = usePlayerStore()
      expect(store.repeatMode).toBe('none')
      store.toggleRepeat()
      expect(store.repeatMode).toBe('list')
      store.toggleRepeat()
      expect(store.repeatMode).toBe('track')
      store.toggleRepeat()
      expect(store.repeatMode).toBe('none')
    })

    it('disables shuffle when entering list mode', () => {
      const store = usePlayerStore()
      store.isShuffle = true
      store.toggleRepeat() // none -> list
      expect(store.isShuffle).toBe(false)
    })

    it('does not change shuffle when entering track mode', () => {
      const store = usePlayerStore()
      store.isShuffle = true
      store.toggleRepeat() // none -> list (clears shuffle)
      store.isShuffle = true
      store.toggleRepeat() // list -> track (should keep shuffle)
      expect(store.repeatMode).toBe('track')
      expect(store.isShuffle).toBe(true)
    })
  })

  // ---------- nextTrack / previousTrack ----------

  describe('nextTrack / previousTrack (non-shuffle)', () => {
    it('nextTrack advances to next index', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[1]
      await store.nextTrack()
      expect(store.currentTrackIndex).toBe(2)
      expect(store.currentTrack?.path).toBe(playlist[2].path)
    })

    it('nextTrack wraps at end even when repeatMode is none (manual next always wraps)', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      store.repeatMode = 'none'
      await store.nextTrack()
      expect(store.currentTrackIndex).toBe(0)
    })

    it('nextTrack wraps at end when repeatMode is list', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      store.repeatMode = 'list'
      await store.nextTrack()
      expect(store.currentTrackIndex).toBe(0)
    })

    it('nextTrack in track repeat mode still advances (manual next overrides track loop)', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[1]
      store.repeatMode = 'track'
      await store.nextTrack()
      expect(store.currentTrackIndex).toBe(2)
    })

    it('previousTrack goes to previous index', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      await store.previousTrack()
      expect(store.currentTrackIndex).toBe(1)
    })

    it('previousTrack wraps at start', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[0]
      await store.previousTrack()
      expect(store.currentTrackIndex).toBe(2)
    })

    it('nextTrack is a no-op without currentTrack', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      store.playlist = makePlaylist(3)
      await store.nextTrack()
      expect(store.currentTrack).toBeNull()
      expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
    })

    it('nextTrack is a no-op while loading', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[0]
      store._isLoading = true
      await store.nextTrack()
      expect(store.currentTrackIndex).toBe(0)
      expect(invokeMock).not.toHaveBeenCalledWith('play_track', expect.anything())
    })
  })

  // ---------- _onEnded branches by repeatMode ----------

  describe('_onEnded branches by repeatMode', () => {
    it('track repeat replays the current track', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[1]
      store.repeatMode = 'track'
      await store._onEnded()
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: playlist[1].path })
    })

    it('list repeat wraps to next track at end', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      store.repeatMode = 'list'
      await store._onEnded()
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: playlist[0].path })
    })

    it('none mode advances when not at end', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[1]
      store.repeatMode = 'none'
      await store._onEnded()
      expect(invokeMock).toHaveBeenCalledWith('play_track', { path: playlist[2].path })
    })

    it('none mode stops at end without playing next', async () => {
      setupPlayInvokeMock()
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      store.duration = 100
      store.currentTime = 50
      store.isPlaying = true
      store.repeatMode = 'none'
      await store._onEnded()
      expect(store.isPlaying).toBe(false)
      expect(store.currentTime).toBe(100)
      const playCalls = invokeMock.mock.calls.filter((c) => c[0] === 'play_track')
      expect(playCalls).toHaveLength(0)
    })
  })

  // ---------- shuffle ----------

  describe('toggleShuffle', () => {
    it('toggles isShuffle and resets repeatMode to none when enabling', () => {
      const store = usePlayerStore()
      store.repeatMode = 'list'
      expect(store.isShuffle).toBe(false)
      store.toggleShuffle()
      expect(store.isShuffle).toBe(true)
      expect(store.repeatMode).toBe('none')
    })

    it('generates shuffle order on enable with current track at position 0', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(3)
      store.currentTrack = store.playlist[1]
      store.toggleShuffle()
      expect(store._shuffleOrder).toHaveLength(3)
      expect(store._shufflePosition).toBe(0)
      // 当前曲目应被放在洗牌序列起点
      expect(store._shuffleOrder[0]).toBe(1)
    })

    it('clears shuffle order on disable', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(3)
      store.currentTrack = store.playlist[1]
      store.toggleShuffle() // on
      store.toggleShuffle() // off
      expect(store.isShuffle).toBe(false)
      expect(store._shuffleOrder).toEqual([])
      expect(store._shufflePosition).toBe(-1)
    })

    it('clears shuffle history when enabling', () => {
      const store = usePlayerStore()
      store._shuffleHistory = [1, 2]
      store.playlist = makePlaylist(3)
      store.currentTrack = store.playlist[0]
      store.toggleShuffle()
      expect(store._shuffleHistory).toEqual([])
    })

    it('handles empty playlist when enabling shuffle', () => {
      const store = usePlayerStore()
      store.toggleShuffle()
      expect(store.isShuffle).toBe(true)
      expect(store._shuffleOrder).toEqual([])
      expect(store._shufflePosition).toBe(-1)
    })

    it('disabling shuffle keeps history intact', () => {
      const store = usePlayerStore()
      store._shuffleHistory = [1, 2]
      store.isShuffle = true
      store.toggleShuffle() // off
      expect(store._shuffleHistory).toEqual([1, 2])
    })
  })

  // ---------- volume ----------

  describe('setVolume', () => {
    it('clamps values above 1 to 1', () => {
      const store = usePlayerStore()
      store.setVolume(1.5)
      expect(store.volume).toBe(1)
    })

    it('clamps negative values to 0', () => {
      const store = usePlayerStore()
      store.setVolume(-0.5)
      expect(store.volume).toBe(0)
    })

    it('sets volume to 0', () => {
      const store = usePlayerStore()
      store.setVolume(0)
      expect(store.volume).toBe(0)
    })

    it('sets volume to 1', () => {
      const store = usePlayerStore()
      store.setVolume(1)
      expect(store.volume).toBe(1)
    })

    it('updates previousVolume when volume > 0', () => {
      const store = usePlayerStore()
      store.previousVolume = 0.3
      store.setVolume(0.8)
      expect(store.previousVolume).toBe(0.8)
    })

    it('does not update previousVolume when volume is 0', () => {
      const store = usePlayerStore()
      store.previousVolume = 0.5
      store.setVolume(0)
      expect(store.previousVolume).toBe(0.5)
    })

    it('unmutes when setting volume > 0 while muted', () => {
      const store = usePlayerStore()
      store.isMuted = true
      store.setVolume(0.5)
      expect(store.isMuted).toBe(false)
    })

    it('stays muted when setting volume to 0', () => {
      const store = usePlayerStore()
      store.isMuted = true
      store.setVolume(0)
      expect(store.isMuted).toBe(true)
    })

    it('calls invoke set_volume with clamped value', () => {
      const store = usePlayerStore()
      store.setVolume(2)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 1 })
    })

    it('calls set_volume with 0 when muted and setting volume to 0', () => {
      const store = usePlayerStore()
      store.isMuted = true
      store.setVolume(0)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0 })
    })

    it('calls set_volume with newVolume when unmuting via setVolume', () => {
      const store = usePlayerStore()
      store.isMuted = true
      store.setVolume(0.7)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0.7 })
    })
  })

  // ---------- mute ----------

  describe('toggleMute', () => {
    it('mutes: saves current volume to previousVolume and sets isMuted', () => {
      const store = usePlayerStore()
      store.volume = 0.7
      store.isMuted = false
      store.toggleMute()
      expect(store.isMuted).toBe(true)
      expect(store.previousVolume).toBe(0.7)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0 })
    })

    it('mute preserves previousVolume when current volume is 0', () => {
      const store = usePlayerStore()
      store.volume = 0
      store.previousVolume = 0.4
      store.isMuted = false
      store.toggleMute()
      expect(store.previousVolume).toBe(0.4)
    })

    it('unmute: restores volume from previousVolume', () => {
      const store = usePlayerStore()
      store.previousVolume = 0.6
      store.volume = 0
      store.isMuted = true
      store.toggleMute()
      expect(store.isMuted).toBe(false)
      expect(store.volume).toBe(0.6)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0.6 })
    })

    it('unmute falls back to 0.5 when previousVolume is 0', () => {
      const store = usePlayerStore()
      store.previousVolume = 0
      store.isMuted = true
      store.toggleMute()
      expect(store.volume).toBe(0.5)
      expect(invokeMock).toHaveBeenCalledWith('set_volume', { volume: 0.5 })
    })
  })

  // ---------- loadPlaylist ----------

  describe('loadPlaylist', () => {
    it('sets playlist and currentTrack to first track', () => {
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.loadPlaylist(playlist)
      expect(store.playlist).toHaveLength(3)
      expect(store.currentTrack?.path).toBe(playlist[0].path)
      expect(store.currentTrackIndex).toBe(0)
      expect(store.duration).toBe(100)
    })

    it('resets state when playlist is empty', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(2)
      store.currentTrack = store.playlist[0]
      store.isPlaying = true
      store.duration = 200
      store.loadPlaylist([])
      expect(store.playlist).toEqual([])
      expect(store.currentTrack).toBeNull()
      expect(store.isPlaying).toBe(false)
      expect(store.duration).toBe(0)
    })

    it('invalidates shuffle order and history', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(3)
      store.currentTrack = store.playlist[0]
      store.toggleShuffle()
      expect(store._shuffleOrder).toHaveLength(3)
      store.loadPlaylist(makePlaylist(3))
      expect(store._shuffleOrder).toEqual([])
      expect(store._shufflePosition).toBe(-1)
      expect(store._shuffleHistory).toEqual([])
    })

    it('sets audioInfo from first track metadata', () => {
      const store = usePlayerStore()
      const playlist: Track[] = [
        {
          path: '/music/test.mp3',
          name: 'Test',
          title: 'Test',
          duration: 200,
          bitrate: 320,
          sampleRate: 44100,
          channels: 2,
          bitDepth: 16,
          format: 'mp3',
        },
      ]
      store.loadPlaylist(playlist)
      expect(store.audioInfo.bitrate).toBe(320)
      expect(store.audioInfo.sampleRate).toBe(44100)
      expect(store.audioInfo.channels).toBe(2)
      expect(store.audioInfo.bitDepth).toBe(16)
      expect(store.audioInfo.format).toBe('mp3')
    })
  })

  // ---------- LRU cache (via _checkFileExists) ----------

  describe('LRU cache (via _checkFileExists)', () => {
    it('caches file existence results so subsequent calls skip FileUtils', async () => {
      const store = usePlayerStore()
      await store._checkFileExists('/music/a.mp3')
      expect(fileExistsMock).toHaveBeenCalledTimes(1)
      await store._checkFileExists('/music/a.mp3')
      expect(fileExistsMock).toHaveBeenCalledTimes(1)
    })

    it('caches negative results too', async () => {
      fileExistsMock.mockResolvedValueOnce(false)
      const store = usePlayerStore()
      const r1 = await store._checkFileExists('/music/missing.mp3')
      expect(r1).toBe(false)
      const r2 = await store._checkFileExists('/music/missing.mp3')
      expect(r2).toBe(false)
      expect(fileExistsMock).toHaveBeenCalledTimes(1)
    })

    it('returns false for empty path without calling FileUtils', async () => {
      const store = usePlayerStore()
      const r = await store._checkFileExists('')
      expect(r).toBe(false)
      expect(fileExistsMock).not.toHaveBeenCalled()
    })

    it('different paths each trigger a FileUtils call', async () => {
      const store = usePlayerStore()
      await store._checkFileExists('/music/a.mp3')
      await store._checkFileExists('/music/b.mp3')
      expect(fileExistsMock).toHaveBeenCalledTimes(2)
    })

    it('resetPlayerState clears the file-exists cache', async () => {
      const store = usePlayerStore()
      await store._checkFileExists('/music/a.mp3')
      expect(fileExistsMock).toHaveBeenCalledTimes(1)
      await store.resetPlayerState()
      await store._checkFileExists('/music/a.mp3')
      expect(fileExistsMock).toHaveBeenCalledTimes(2)
    })
  })

  // ---------- lyrics offset ----------

  describe('lyrics offset', () => {
    it('setLyricsOffset sets the offset', () => {
      const store = usePlayerStore()
      store.setLyricsOffset(100)
      expect(store.lyricsOffset).toBe(100)
    })

    it('adjustLyricsOffset adds delta rounded to one decimal', () => {
      const store = usePlayerStore()
      store.lyricsOffset = 10
      store.adjustLyricsOffset(0.25)
      expect(store.lyricsOffset).toBe(10.3)
    })

    it('resetLyricsOffset resets to 0', () => {
      const store = usePlayerStore()
      store.lyricsOffset = 50
      store.resetLyricsOffset()
      expect(store.lyricsOffset).toBe(0)
    })
  })

  // ---------- addTrackNext ----------

  describe('addTrackNext', () => {
    it('inserts a new track right after the current track', () => {
      const store = usePlayerStore()
      store.playlist = [...makePlaylist(3)]
      store.currentTrack = store.playlist[1]
      const newTrack = makeTrack('/music/new.mp3', 'New')
      store.addTrackNext(newTrack)
      expect(store.playlist).toHaveLength(4)
      expect(store.playlist[2].path).toBe('/music/new.mp3')
    })

    it('unshifts when there is no current track', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(2)
      const newTrack = makeTrack('/music/new.mp3', 'New')
      store.addTrackNext(newTrack)
      expect(store.playlist).toHaveLength(3)
      expect(store.playlist[0].path).toBe('/music/new.mp3')
    })

    it('moves an existing track to the next position', () => {
      const store = usePlayerStore()
      const playlist = makePlaylist(4)
      store.playlist = [...playlist]
      store.currentTrack = playlist[1]
      store.addTrackNext(playlist[3])
      expect(store.playlist).toHaveLength(4)
      expect(store.playlist[2].path).toBe(playlist[3].path)
    })

    it('does nothing when track is null', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(2)
      store.addTrackNext(null as unknown as Track)
      expect(store.playlist).toHaveLength(2)
    })
  })

  // ---------- getters ----------

  describe('getters', () => {
    it('currentTrackIndex returns -1 when no current track', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(3)
      expect(store.currentTrackIndex).toBe(-1)
    })

    it('currentTrackIndex returns -1 when playlist is empty', () => {
      const store = usePlayerStore()
      store.currentTrack = makeTrack('/x.mp3')
      expect(store.currentTrackIndex).toBe(-1)
    })

    it('currentTrackIndex finds the track by path', () => {
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[2]
      expect(store.currentTrackIndex).toBe(2)
    })

    it('hasNextTrack is false when playlist has <= 1 track', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(1)
      store.currentTrack = store.playlist[0]
      expect(store.hasNextTrack).toBe(false)
    })

    it('hasNextTrack is true when playlist has > 1 track with current', () => {
      const store = usePlayerStore()
      const playlist = makePlaylist(3)
      store.playlist = playlist
      store.currentTrack = playlist[0]
      expect(store.hasNextTrack).toBe(true)
    })

    it('hasPreviousTrack is false when playlist has <= 1 track', () => {
      const store = usePlayerStore()
      store.playlist = makePlaylist(1)
      store.currentTrack = store.playlist[0]
      expect(store.hasPreviousTrack).toBe(false)
    })

    it('currentLyric returns null when no lyrics loaded', () => {
      const store = usePlayerStore()
      expect(store.currentLyric).toBeNull()
    })
  })
})
