import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

import { NeteaseAPI } from '@/utils/neteaseApi'
import { invoke } from '@tauri-apps/api/core'

const invokeMock = vi.mocked(invoke)

describe('NeteaseAPI', () => {
  let api: NeteaseAPI

  beforeEach(() => {
    api = new NeteaseAPI()
    vi.clearAllMocks()
  })

  describe('searchSongs', () => {
    it('should return songs on success', async () => {
      const songs = [
        { id: '1', name: 'Song 1', artist: 'Artist 1', duration: 180000 },
      ]
      invokeMock.mockResolvedValue(songs)

      const result = await api.searchSongs('test')

      expect(result).toEqual(songs)
      expect(invokeMock).toHaveBeenCalledWith('netease_search_songs', {
        keyword: 'test',
        limit: 10,
        offset: 0,
      })
    })

    it('should return empty array on error', async () => {
      invokeMock.mockRejectedValue(new Error('Network error'))

      const result = await api.searchSongs('test')

      expect(result).toEqual([])
    })

    it('should use custom limit and offset', async () => {
      invokeMock.mockResolvedValue([])

      await api.searchSongs('test', 20, 10)

      expect(invokeMock).toHaveBeenCalledWith('netease_search_songs', {
        keyword: 'test',
        limit: 20,
        offset: 10,
      })
    })
  })

  describe('getLyrics', () => {
    it('should return lyrics on success', async () => {
      const lyrics = { lrc: '[00:01.00]Hello', tlyric: '[00:01.00]你好' }
      invokeMock.mockResolvedValue(lyrics)

      const result = await api.getLyrics('123')

      expect(result).toEqual(lyrics)
    })

    it('should return null on error', async () => {
      invokeMock.mockRejectedValue(new Error('Not found'))

      const result = await api.getLyrics('123')

      expect(result).toBeNull()
    })
  })

  describe('searchAndGetLyrics', () => {
    it('should return lyrics for best match', async () => {
      const songs = [
        { id: '1', name: 'Test Song', artist: 'Test Artist', duration: 180000 },
      ]
      const lyrics = { lrc: '[00:01.00]Hello' }

      invokeMock.mockResolvedValueOnce(songs)
      invokeMock.mockResolvedValueOnce(lyrics)

      const result = await api.searchAndGetLyrics('Test Song', 'Test Artist', 180000)

      expect(result).toEqual(lyrics)
    })

    it('should fallback to title-only search', async () => {
      const songs = [
        { id: '1', name: 'Test Song', artist: 'Someone', duration: 180000 },
      ]
      const lyrics = { lrc: '[00:01.00]Hello' }

      invokeMock.mockResolvedValueOnce([])
      invokeMock.mockResolvedValueOnce(songs)
      invokeMock.mockResolvedValueOnce(lyrics)

      const result = await api.searchAndGetLyrics('Test Song', 'Test Artist')

      expect(result).toEqual(lyrics)
    })

    it('should return null when no songs found', async () => {
      invokeMock.mockResolvedValue([])

      const result = await api.searchAndGetLyrics('Unknown Song')

      expect(result).toBeNull()
    })
  })

  describe('findBestMatch', () => {
    it('should return null for empty songs', () => {
      expect(api.findBestMatch([], 'title', 'artist', 0)).toBeNull()
    })

    it('should match exact title', () => {
      const songs = [
        { id: '1', name: 'Exact Title', artist: 'Artist', duration: 180000 },
      ]
      expect(api.findBestMatch(songs, 'Exact Title', '', 0)).toEqual(songs[0])
    })

    it('should match title with different case', () => {
      const songs = [
        { id: '1', name: 'TITLE', artist: 'Artist', duration: 180000 },
      ]
      expect(api.findBestMatch(songs, 'title', '', 0)).toEqual(songs[0])
    })

    it('should match title with spaces and punctuation removed', () => {
      const songs = [
        { id: '1', name: 'My-Song_Title', artist: 'Artist', duration: 180000 },
      ]
      expect(api.findBestMatch(songs, 'My Song Title', '', 0)).toEqual(songs[0])
    })

    it('should match artist', () => {
      const songs = [
        { id: '1', name: 'Song', artist: 'Artist Name', duration: 180000 },
      ]
      expect(api.findBestMatch(songs, 'Song', 'Artist Name', 0)).toEqual(songs[0])
    })

    it('should use duration for scoring', () => {
      const songs = [
        { id: '1', name: 'Song', artist: 'Artist', duration: 180000 },
        { id: '2', name: 'Song', artist: 'Artist', duration: 185000 },
      ]
      // 180000 vs 180000 = exact match, should win
      expect(api.findBestMatch(songs, 'Song', 'Artist', 180000)).toEqual(songs[0])
    })

    it('should return null when score is too low', () => {
      const songs = [
        { id: '1', name: 'Completely Different', artist: 'Other', duration: 0 },
      ]
      expect(api.findBestMatch(songs, 'Title', 'Artist', 0)).toBeNull()
    })

    it('should ignore parentheses in title', () => {
      const songs = [
        { id: '1', name: 'Title (Remix)', artist: 'Artist', duration: 0 },
      ]
      expect(api.findBestMatch(songs, 'Title', '', 0)).toEqual(songs[0])
    })
  })

  describe('mergeLyrics', () => {
    it('should return original when no translation', () => {
      expect(api.mergeLyrics('[00:01.00]Hello', '')).toBe('[00:01.00]Hello')
    })

    it('should return empty when no original', () => {
      expect(api.mergeLyrics('', '[00:01.00]你好')).toBe('')
    })

    it('should merge original and translation', () => {
      const lrc = '[00:01.00]Hello\n[00:02.00]World'
      const tlyric = '[00:01.00]你好\n[00:02.00]世界'
      const result = api.mergeLyrics(lrc, tlyric)

      expect(result).toContain('[00:01.00]Hello')
      expect(result).toContain('[00:01.00]你好')
      expect(result).toContain('[00:02.00]World')
      expect(result).toContain('[00:02.00]世界')
    })

    it('should skip duplicate translation', () => {
      const lrc = '[00:01.00]Hello'
      const tlyric = '[00:01.00]Hello'

      const result = api.mergeLyrics(lrc, tlyric)

      expect(result).toBe('[00:01.00]Hello')
    })

    it('should handle missing translation lines', () => {
      const lrc = '[00:01.00]Hello\n[00:02.00]World'
      const tlyric = '[00:01.00]你好'

      const result = api.mergeLyrics(lrc, tlyric)

      expect(result).toContain('[00:01.00]Hello')
      expect(result).toContain('[00:01.00]你好')
      expect(result).toContain('[00:02.00]World')
    })
  })
})
