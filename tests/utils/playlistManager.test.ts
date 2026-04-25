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

vi.mock('@/utils/titleExtractor', () => ({
  TitleExtractor: {
    formatPlaylistName: vi.fn((path: string, format?: string) => {
      const parts = path.split(/[/\\]/)
      const name = parts[parts.length - 1] || path
      if (format) {
        return format.replace('{folderName}', name)
      }
      return name
    }),
    extractTitlesBatch: vi.fn(async (paths: string[]) => {
      const map = new Map()
      for (const path of paths) {
        const parts = path.split(/[/\\]/)
        const fileName = parts[parts.length - 1] || path
        const name = fileName.replace(/\.[^.]+$/, '')
        map.set(path, {
          title: name,
          artist: '',
          fileName,
          isFromMetadata: false,
        })
      }
      return map
    }),
  },
}))

import { PlaylistManager } from '@/utils/playlistManager'
import { invoke } from '@tauri-apps/api/core'

const invokeMock = vi.mocked(invoke)

describe('PlaylistManager', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  describe('getFolderName', () => {
    it('should extract folder name from Unix path', () => {
      expect(PlaylistManager.getFolderName('/music/rock')).toBe('rock')
    })

    it('should extract folder name from Windows path', () => {
      expect(PlaylistManager.getFolderName('C:\\Music\\Jazz')).toBe('Jazz')
    })

    it('should return path if no separator', () => {
      expect(PlaylistManager.getFolderName('music')).toBe('music')
    })
  })

  describe('shouldIgnoreFolder', () => {
    it('should ignore hidden folders', () => {
      expect(PlaylistManager.shouldIgnoreFolder('/music/.git', [], true)).toBe(true)
      expect(PlaylistManager.shouldIgnoreFolder('/music/normal', [], true)).toBe(false)
    })

    it('should not ignore hidden folders when disabled', () => {
      expect(PlaylistManager.shouldIgnoreFolder('/music/.git', [], false)).toBe(false)
    })

    it('should ignore blacklisted folders', () => {
      expect(PlaylistManager.shouldIgnoreFolder('/music/node_modules', ['node_modules'], true)).toBe(true)
    })

    it('should not ignore non-blacklisted folders', () => {
      expect(PlaylistManager.shouldIgnoreFolder('/music/rock', ['pop'], true)).toBe(false)
    })
  })

  describe('countAudioFiles', () => {
    it('should return 0 for null', () => {
      expect(PlaylistManager.countAudioFiles(null)).toBe(0)
    })

    it('should count audio files in node', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [],
        audioFiles: [{ path: '/music/song1.mp3' }, { path: '/music/song2.mp3' }] as any,
      }
      expect(PlaylistManager.countAudioFiles(node)).toBe(2)
    })

    it('should count recursively', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [{ path: '/music/rock/song.mp3' }] as any,
          },
        ],
        audioFiles: [{ path: '/music/song.mp3' }] as any,
      }
      expect(PlaylistManager.countAudioFiles(node)).toBe(2)
    })
  })

  describe('countSubdirectories', () => {
    it('should return 0 for null', () => {
      expect(PlaylistManager.countSubdirectories(null)).toBe(0)
    })

    it('should count subdirectories recursively', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [
              {
                path: '/music/rock/heavy',
                name: 'heavy',
                depth: 2,
                subdirectories: [],
                audioFiles: [],
              },
            ],
            audioFiles: [],
          },
        ],
        audioFiles: [],
      }
      expect(PlaylistManager.countSubdirectories(node)).toBe(2)
    })
  })

  describe('collectAllAudioFiles', () => {
    it('should return empty array for null', () => {
      expect(PlaylistManager.collectAllAudioFiles(null)).toEqual([])
    })

    it('should collect all audio files recursively', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [{ path: '/music/rock/song1.mp3' }] as any,
          },
        ],
        audioFiles: [{ path: '/music/song2.mp3' }] as any,
      }
      const result = PlaylistManager.collectAllAudioFiles(node)
      expect(result).toHaveLength(2)
      expect(result.map((f: any) => f.path)).toContain('/music/song2.mp3')
      expect(result.map((f: any) => f.path)).toContain('/music/rock/song1.mp3')
    })
  })

  describe('countDirectories', () => {
    it('should count directories recursively', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [],
          },
        ],
        audioFiles: [],
      }
      expect(PlaylistManager.countDirectories(node)).toBe(2)
    })
  })

  describe('getMaxDepth', () => {
    it('should return 0 for null', () => {
      expect(PlaylistManager.getMaxDepth(null)).toBe(0)
    })

    it('should get max depth recursively', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [
              {
                path: '/music/rock/heavy',
                name: 'heavy',
                depth: 2,
                subdirectories: [],
                audioFiles: [],
              },
            ],
            audioFiles: [],
          },
        ],
        audioFiles: [],
      }
      expect(PlaylistManager.getMaxDepth(node)).toBe(2)
    })

    it('should return node depth when no children', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [],
        audioFiles: [],
      }
      expect(PlaylistManager.getMaxDepth(node)).toBe(0)
    })
  })

  describe('isFinalAudioDirectory', () => {
    it('should return true for null', () => {
      expect(PlaylistManager.isFinalAudioDirectory(null)).toBe(true)
    })

    it('should return true when no subdirectories', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [],
        audioFiles: [{ path: '/music/song.mp3' }] as any,
      }
      expect(PlaylistManager.isFinalAudioDirectory(node)).toBe(true)
    })

    it('should return false when subdirectories have audio files', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [{ path: '/music/rock/song.mp3' }] as any,
          },
        ],
        audioFiles: [{ path: '/music/song.mp3' }] as any,
      }
      expect(PlaylistManager.isFinalAudioDirectory(node)).toBe(false)
    })

    it('should return true when subdirectories have no audio files', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/empty',
            name: 'empty',
            depth: 1,
            subdirectories: [],
            audioFiles: [],
          },
        ],
        audioFiles: [{ path: '/music/song.mp3' }] as any,
      }
      expect(PlaylistManager.isFinalAudioDirectory(node)).toBe(true)
    })
  })

  describe('hasAudioFilesInSubtree', () => {
    it('should return false for null', () => {
      expect(PlaylistManager.hasAudioFilesInSubtree(null)).toBe(false)
    })

    it('should return true when node has audio files', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [],
        audioFiles: [{ path: '/music/song.mp3' }] as any,
      }
      expect(PlaylistManager.hasAudioFilesInSubtree(node)).toBe(true)
    })

    it('should return true when subdirectory has audio files', () => {
      const node = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [{ path: '/music/rock/song.mp3' }] as any,
          },
        ],
        audioFiles: [],
      }
      expect(PlaylistManager.hasAudioFilesInSubtree(node)).toBe(true)
    })
  })

  describe('scanDirectoryTree', () => {
    it('should return null when exceeding max depth', async () => {
      invokeMock.mockImplementation((cmd: string) => {
        if (cmd === 'read_directory') return Promise.resolve(['/music/sub'])
        if (cmd === 'get_audio_files') return Promise.resolve({ files: [] })
        return Promise.resolve([])
      })

      const result = await PlaylistManager.scanDirectoryTree('/music', {
        enableSubdirectoryScan: true,
        maxDepth: 0,
        ignoreHiddenFolders: true,
        folderBlacklist: [],
      })

      // depth 0 is allowed, depth 1 should be skipped
      expect(result).not.toBeNull()
      expect(result!.subdirectories).toHaveLength(0)
    })

    it('should scan directory and return tree', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') {
          if (args.path === '/music') return Promise.resolve(['/music/rock'])
          return Promise.resolve([])
        }
        if (cmd === 'get_audio_files') {
          return Promise.resolve({ files: [{ path: '/music/song.mp3', name: 'song' }] })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.scanDirectoryTree('/music', {
        enableSubdirectoryScan: true,
        maxDepth: 3,
        ignoreHiddenFolders: true,
        folderBlacklist: [],
      })

      expect(result).not.toBeNull()
      expect(result!.name).toBe('music')
      expect(result!.audioFiles).toHaveLength(1)
      expect(result!.subdirectories).toHaveLength(1)
      expect(result!.subdirectories[0].name).toBe('rock')
    })

    it('should ignore hidden folders', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') {
          return Promise.resolve(['/music/.git', '/music/rock'])
        }
        if (cmd === 'get_audio_files') {
          return Promise.resolve({ files: [] })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.scanDirectoryTree('/music', {
        enableSubdirectoryScan: true,
        maxDepth: 3,
        ignoreHiddenFolders: true,
        folderBlacklist: [],
      })

      expect(result!.subdirectories).toHaveLength(1)
      expect(result!.subdirectories[0].name).toBe('rock')
    })

    it('should handle read_directory errors gracefully', async () => {
      invokeMock.mockRejectedValue(new Error('Permission denied'))

      const result = await PlaylistManager.scanDirectoryTree('/music', {
        enableSubdirectoryScan: true,
        maxDepth: 3,
        ignoreHiddenFolders: true,
        folderBlacklist: [],
      })

      expect(result).toBeNull()
    })
  })

  describe('generatePlaylistsFromTree', () => {
    it('should return empty array for null', async () => {
      const result = await PlaylistManager.generatePlaylistsFromTree(null, {})
      expect(result).toEqual([])
    })

    it('should generate playlists for final audio directories', async () => {
      const tree = {
        path: '/music/rock',
        name: 'rock',
        depth: 1,
        subdirectories: [],
        audioFiles: [{ path: '/music/rock/song.mp3', name: 'song' }] as any,
      }

      const result = await PlaylistManager.generatePlaylistsFromTree(tree, {})

      expect(result).toHaveLength(1)
      expect(result[0].name).toBe('rock')
      expect(result[0].files).toHaveLength(1)
    })

    it('should generate all songs playlist when enabled', async () => {
      const tree = {
        path: '/music',
        name: 'music',
        depth: 0,
        subdirectories: [
          {
            path: '/music/rock',
            name: 'rock',
            depth: 1,
            subdirectories: [],
            audioFiles: [{ path: '/music/rock/song1.mp3', name: 'song1' }] as any,
          },
        ],
        audioFiles: [{ path: '/music/song2.mp3', name: 'song2' }] as any,
      }

      const result = await PlaylistManager.generatePlaylistsFromTree(tree, {
        playlist: { generateAllSongsPlaylist: true },
      })

      const allSongsPlaylist = result.find(p => p.isAllSongsPlaylist)
      expect(allSongsPlaylist).toBeDefined()
      expect(allSongsPlaylist!.files).toHaveLength(2)
    })
  })

  describe('searchAudioFiles', () => {
    it('should search by title', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') return Promise.resolve([])
        if (cmd === 'get_audio_files') {
          return Promise.resolve({
            files: [
              { path: '/music/song1.mp3', title: 'Hello World', artist: 'Artist' },
              { path: '/music/song2.mp3', title: 'Goodbye', artist: 'Artist' },
            ],
          })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.searchAudioFiles('/music', 'hello', {})

      expect(result).toHaveLength(1)
      expect(result[0].title).toBe('Hello World')
    })

    it('should search by artist', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') return Promise.resolve([])
        if (cmd === 'get_audio_files') {
          return Promise.resolve({
            files: [
              { path: '/music/song1.mp3', title: 'Song', artist: 'Rock Band' },
              { path: '/music/song2.mp3', title: 'Song', artist: 'Jazz Band' },
            ],
          })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.searchAudioFiles('/music', 'jazz', {})

      expect(result).toHaveLength(1)
      expect(result[0].artist).toBe('Jazz Band')
    })
  })

  describe('getPlaylistByPath', () => {
    it('should return playlist for matching path', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') {
          if (args.path === '/music') return Promise.resolve(['/music/rock'])
          return Promise.resolve([])
        }
        if (cmd === 'get_audio_files') {
          return Promise.resolve({
            files: [{ path: '/music/rock/song.mp3', name: 'song' }],
          })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.getPlaylistByPath('/music', '/music/rock', {})

      expect(result).not.toBeNull()
      expect(result!.name).toBe('rock')
    })

    it('should return null for non-matching path', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') return Promise.resolve([])
        if (cmd === 'get_audio_files') return Promise.resolve({ files: [] })
        return Promise.resolve([])
      })

      const result = await PlaylistManager.getPlaylistByPath('/music', '/music/nonexistent', {})

      expect(result).toBeNull()
    })
  })

  describe('getDirectoryStats', () => {
    it('should return directory statistics', async () => {
      invokeMock.mockImplementation((cmd: string, args?: any) => {
        if (cmd === 'read_directory') {
          if (args.path === '/music') return Promise.resolve(['/music/rock'])
          return Promise.resolve([])
        }
        if (cmd === 'get_audio_files') {
          return Promise.resolve({
            files: [{ path: '/music/song.mp3', name: 'song' }],
          })
        }
        return Promise.resolve([])
      })

      const result = await PlaylistManager.getDirectoryStats('/music', {})

      expect(result.totalDirectories).toBeGreaterThan(0)
      expect(result.totalAudioFiles).toBeGreaterThan(0)
      expect(result.totalPlaylists).toBeGreaterThanOrEqual(0)
      expect(result.maxDepth).toBeGreaterThanOrEqual(0)
    })
  })
})
