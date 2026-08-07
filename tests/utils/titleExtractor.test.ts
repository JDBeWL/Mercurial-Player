import { describe, it, expect, beforeEach } from 'vitest'
import { TitleExtractor } from '@/utils/titleExtractor'
import { mockInvoke, resetTauriMocks, setupInvokeMocks } from '../mocks/tauri'

describe('TitleExtractor', () => {
  beforeEach(() => {
    resetTauriMocks()
  })

  describe('getFileName', () => {
    it('should extract filename without extension', () => {
      expect(TitleExtractor.getFileName('/path/to/song.mp3', true)).toBe('song')
      expect(TitleExtractor.getFileName('C:\\Music\\track.flac', true)).toBe('track')
    })

    it('should keep extension when hideExtension is false', () => {
      expect(TitleExtractor.getFileName('/path/to/song.mp3', false)).toBe('song.mp3')
    })

    it('should handle files without extension', () => {
      expect(TitleExtractor.getFileName('/path/to/song', true)).toBe('song')
    })
  })

  describe('cleanTitle', () => {
    it('should trim whitespace', () => {
      expect(TitleExtractor.cleanTitle('  hello  ')).toBe('hello')
    })

    it('should collapse multiple spaces', () => {
      expect(TitleExtractor.cleanTitle('hello   world')).toBe('hello world')
    })

    it('should remove leading/trailing special chars', () => {
      expect(TitleExtractor.cleanTitle('--title--')).toBe('title')
      expect(TitleExtractor.cleanTitle('__title__')).toBe('title')
    })

    it('should return empty string for empty input', () => {
      expect(TitleExtractor.cleanTitle('')).toBe('')
      expect(TitleExtractor.cleanTitle(null as unknown as string)).toBe('')
    })
  })

  describe('parseFromFileName', () => {
    it('should parse artist - title format', () => {
      const result = TitleExtractor.parseFromFileName('/music/Artist - Song Title.mp3')
      expect(result.artist).toBe('Artist')
      expect(result.title).toBe('Song Title')
    })

    it('should handle filename without separator', () => {
      const result = TitleExtractor.parseFromFileName('/music/JustASong.mp3')
      expect(result.artist).toBe('')
      expect(result.title).toBe('JustASong')
    })

    it('should use custom separator', () => {
      const result = TitleExtractor.parseFromFileName('/music/Artist_Song.mp3', {
        separator: '_',
        parseArtistTitle: true,
      })
      expect(result.artist).toBe('Artist')
      expect(result.title).toBe('Song')
    })

    it('should not parse when parseArtistTitle is false', () => {
      const result = TitleExtractor.parseFromFileName('/music/Artist - Song.mp3', {
        parseArtistTitle: false,
      })
      expect(result.artist).toBe('')
      expect(result.title).toBe('Artist - Song')
    })

    it('should not split on bare spaces (regression: space was a default separator)', () => {
      const result = TitleExtractor.parseFromFileName('/music/My Song.mp3')
      expect(result.artist).toBe('')
      expect(result.title).toBe('My Song')
    })

    it('should not split on dots when separator is dash (regression)', () => {
      const result = TitleExtractor.parseFromFileName('/music/01. Track Name.mp3')
      expect(result.artist).toBe('')
      expect(result.title).toBe('01. Track Name')
    })

    it('should only use the configured separator, not all separators', () => {
      // separator 为 '_' 时,不应尝试 '-' 分割
      const result = TitleExtractor.parseFromFileName('/music/Artist-Song.mp3', {
        separator: '_',
      })
      expect(result.artist).toBe('')
      expect(result.title).toBe('Artist-Song')
    })
  })

  describe('testParse', () => {
    it('should test parsing without full path', () => {
      const result = TitleExtractor.testParse('Artist - Title')
      expect(result.artist).toBe('Artist')
      expect(result.title).toBe('Title')
    })
  })

  describe('formatPlaylistName', () => {
    it('should extract folder name', () => {
      expect(TitleExtractor.formatPlaylistName('/music/Rock')).toBe('Rock')
      expect(TitleExtractor.formatPlaylistName('C:\\Music\\Jazz')).toBe('Jazz')
    })

    it('should use custom format', () => {
      expect(TitleExtractor.formatPlaylistName('/music/Rock', 'Playlist: {folderName}')).toBe(
        'Playlist: Rock',
      )
    })
  })

})

describe('extractTitle (with Tauri mock)', () => {
  it('should extract title from metadata', async () => {
    setupInvokeMocks({
      get_track_metadata: {
        path: '/music/song.mp3',
        title: 'Song Title',
        artist: 'Artist Name',
        album: 'Album Name',
      },
    })

    const result = await TitleExtractor.extractTitle('/music/song.mp3')

    expect(result.title).toBe('Song Title')
    expect(result.artist).toBe('Artist Name')
    expect(result.album).toBe('Album Name')
    expect(result.isFromMetadata).toBe(true)
  })

  it('should fall back to filename when metadata unavailable', async () => {
    setupInvokeMocks({
      get_track_metadata: { path: '/music/Artist - Song.mp3' },
    })

    const result = await TitleExtractor.extractTitle('/music/Artist - Song.mp3')

    expect(result.title).toBe('Song')
    expect(result.artist).toBe('Artist')
    expect(result.isFromMetadata).toBe(false)
  })

  it('should fall back to filename on metadata error', async () => {
    mockInvoke.mockRejectedValue(new Error('Failed to read metadata'))

    const result = await TitleExtractor.extractTitle('/music/Artist - Song.mp3')

    expect(result.title).toBe('Song')
    expect(result.artist).toBe('Artist')
    expect(result.isFromMetadata).toBe(false)
  })

  it('should skip metadata when preferMetadata is false', async () => {
    resetTauriMocks() // Ensure clean state

    const result = await TitleExtractor.extractTitle('/music/Artist - Song.mp3', {
      preferMetadata: false,
    })

    expect(result.isFromMetadata).toBe(false)
    expect(mockInvoke).not.toHaveBeenCalledWith('get_track_metadata', expect.anything())
  })

  it('should clean title from metadata', async () => {
    setupInvokeMocks({
      get_track_metadata: {
        path: '/music/song.mp3',
        title: '  Song Title  ',
        artist: '  Artist  ',
      },
    })

    const result = await TitleExtractor.extractTitle('/music/song.mp3')

    expect(result.title).toBe('Song Title')
    expect(result.artist).toBe('Artist')
  })
})

describe('extractTitlesBatch (with Tauri mock)', () => {
  it('should extract titles for multiple files', async () => {
    setupInvokeMocks({
      get_tracks_metadata_batch: [
        { path: '/music/song1.mp3', title: 'Song 1', artist: 'Artist 1' },
        { path: '/music/song2.mp3', title: 'Song 2', artist: 'Artist 2' },
      ],
    })

    const result = await TitleExtractor.extractTitlesBatch(['/music/song1.mp3', '/music/song2.mp3'])

    expect(result.size).toBe(2)
    expect(result.get('/music/song1.mp3')?.title).toBe('Song 1')
    expect(result.get('/music/song2.mp3')?.title).toBe('Song 2')
  })

  it('should return empty map for empty input', async () => {
    const result = await TitleExtractor.extractTitlesBatch([])

    expect(result.size).toBe(0)
  })

  it('should fall back to filename parsing on batch error', async () => {
    mockInvoke.mockRejectedValue(new Error('Batch failed'))

    const result = await TitleExtractor.extractTitlesBatch(['/music/Artist - Song.mp3'])

    expect(result.size).toBe(1)
    expect(result.get('/music/Artist - Song.mp3')?.title).toBe('Song')
    expect(result.get('/music/Artist - Song.mp3')?.isFromMetadata).toBe(false)
  })

  it('should include audio info from metadata', async () => {
    setupInvokeMocks({
      get_tracks_metadata_batch: [
        {
          path: '/music/song.mp3',
          title: 'Song',
          duration: 180,
          bitrate: 320,
          sampleRate: 44100,
          channels: 2,
          format: 'mp3',
        },
      ],
    })

    const result = await TitleExtractor.extractTitlesBatch(['/music/song.mp3'])
    const info = result.get('/music/song.mp3')

    expect(info?.duration).toBe(180)
    expect(info?.bitrate).toBe(320)
    expect(info?.sampleRate).toBe(44100)
    expect(info?.channels).toBe(2)
    expect(info?.format).toBe('mp3')
  })

  it('should handle files without metadata title', async () => {
    setupInvokeMocks({
      get_tracks_metadata_batch: [{ path: '/music/Artist - Song.mp3', duration: 180 }],
    })

    const result = await TitleExtractor.extractTitlesBatch(['/music/Artist - Song.mp3'])
    const info = result.get('/music/Artist - Song.mp3')

    expect(info?.title).toBe('Song')
    expect(info?.artist).toBe('Artist')
    expect(info?.duration).toBe(180)
    expect(info?.isFromMetadata).toBe(false)
  })
})
