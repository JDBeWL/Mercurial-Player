import { describe, it, expect, beforeEach } from 'vitest'
import { FileUtils } from '@/utils/fileUtils'
import { mockInvoke, mockOpen, resetTauriMocks, setupInvokeMocks } from '../mocks/tauri'

describe('FileUtils', () => {
  beforeEach(() => {
    resetTauriMocks()
  })

  describe('getFileName', () => {
    it('should extract filename from Unix path', () => {
      expect(FileUtils.getFileName('/path/to/file.mp3')).toBe('file.mp3')
    })

    it('should extract filename from Windows path', () => {
      expect(FileUtils.getFileName('C:\\Music\\song.flac')).toBe('song.flac')
    })

    it('should return input if no path separator', () => {
      expect(FileUtils.getFileName('file.mp3')).toBe('file.mp3')
    })
  })

  describe('getFileNameWithoutExtension', () => {
    it('should remove extension', () => {
      expect(FileUtils.getFileNameWithoutExtension('/path/song.mp3')).toBe('song')
    })

    it('should handle multiple dots', () => {
      expect(FileUtils.getFileNameWithoutExtension('/path/song.name.mp3')).toBe('song.name')
    })

    it('should handle no extension', () => {
      expect(FileUtils.getFileNameWithoutExtension('/path/song')).toBe('song')
    })
  })

  describe('getFileExtension', () => {
    it('should extract extension', () => {
      expect(FileUtils.getFileExtension('/path/song.mp3')).toBe('mp3')
      expect(FileUtils.getFileExtension('/path/song.FLAC')).toBe('flac')
    })

    it('should return empty for no extension', () => {
      expect(FileUtils.getFileExtension('/path/song')).toBe('')
    })
  })

  describe('getDirectoryPath', () => {
    it('should extract directory from Unix path', () => {
      expect(FileUtils.getDirectoryPath('/path/to/file.mp3')).toBe('/path/to')
    })

    it('should extract directory from Windows path', () => {
      expect(FileUtils.getDirectoryPath('C:\\Music\\song.mp3')).toBe('C:/Music')
    })

    it('should return empty for filename only', () => {
      expect(FileUtils.getDirectoryPath('file.mp3')).toBe('')
    })
  })

  describe('isAudioFile', () => {
    it('should recognize audio extensions', () => {
      expect(FileUtils.isAudioFile('song.mp3')).toBe(true)
      expect(FileUtils.isAudioFile('song.flac')).toBe(true)
      expect(FileUtils.isAudioFile('song.wav')).toBe(true)
      expect(FileUtils.isAudioFile('song.ogg')).toBe(true)
      expect(FileUtils.isAudioFile('song.m4a')).toBe(true)
    })

    it('should reject non-audio files', () => {
      expect(FileUtils.isAudioFile('doc.txt')).toBe(false)
      expect(FileUtils.isAudioFile('image.png')).toBe(false)
    })
  })

  describe('isLyricsFile', () => {
    it('should recognize lyrics extensions', () => {
      expect(FileUtils.isLyricsFile('song.lrc')).toBe(true)
      expect(FileUtils.isLyricsFile('song.ass')).toBe(true)
      expect(FileUtils.isLyricsFile('song.srt')).toBe(true)
    })

    it('should reject non-lyrics files', () => {
      expect(FileUtils.isLyricsFile('song.mp3')).toBe(false)
      expect(FileUtils.isLyricsFile('doc.txt')).toBe(false)
    })
  })

  describe('formatFileSize', () => {
    it('should format bytes', () => {
      expect(FileUtils.formatFileSize(0)).toBe('0 Bytes')
      expect(FileUtils.formatFileSize(500)).toBe('500 Bytes')
    })

    it('should format KB', () => {
      expect(FileUtils.formatFileSize(1024)).toBe('1 KB')
      expect(FileUtils.formatFileSize(2048)).toBe('2 KB')
    })

    it('should format MB', () => {
      expect(FileUtils.formatFileSize(1024 * 1024)).toBe('1 MB')
      expect(FileUtils.formatFileSize(5.5 * 1024 * 1024)).toBe('5.5 MB')
    })

    it('should format GB', () => {
      expect(FileUtils.formatFileSize(1024 * 1024 * 1024)).toBe('1 GB')
    })
  })

  describe('formatTime', () => {
    it('should format seconds to mm:ss', () => {
      expect(FileUtils.formatTime(0)).toBe('0:00')
      expect(FileUtils.formatTime(65)).toBe('1:05')
      expect(FileUtils.formatTime(125)).toBe('2:05')
    })

    it('should format to hh:mm:ss for long durations', () => {
      expect(FileUtils.formatTime(3661)).toBe('1:01:01')
    })

    it('should handle invalid input', () => {
      expect(FileUtils.formatTime(NaN)).toBe('0:00')
      expect(FileUtils.formatTime(Infinity)).toBe('0:00')
    })
  })

  describe('selectFolder (with Tauri mock)', () => {
    it('should return selected folder path', async () => {
      mockOpen.mockResolvedValue('/selected/folder')

      const result = await FileUtils.selectFolder()

      expect(result).toBe('/selected/folder')
      expect(mockOpen).toHaveBeenCalledWith(
        expect.objectContaining({
          directory: true,
          multiple: false,
        }),
      )
    })

    it('should return null when cancelled', async () => {
      mockOpen.mockResolvedValue(null)

      const result = await FileUtils.selectFolder()

      expect(result).toBeNull()
    })

    it('should return null on error', async () => {
      mockOpen.mockRejectedValue(new Error('Permission denied'))

      const result = await FileUtils.selectFolder()

      expect(result).toBeNull()
    })

    it('should pass custom options', async () => {
      mockOpen.mockResolvedValue('/custom/path')

      await FileUtils.selectFolder({ title: 'Custom Title' })

      expect(mockOpen).toHaveBeenCalledWith(
        expect.objectContaining({
          title: 'Custom Title',
        }),
      )
    })
  })

  describe('selectFiles (with Tauri mock)', () => {
    it('should return selected files', async () => {
      mockOpen.mockResolvedValue(['/file1.mp3', '/file2.mp3'])

      const result = await FileUtils.selectFiles()

      expect(result).toEqual(['/file1.mp3', '/file2.mp3'])
      expect(mockOpen).toHaveBeenCalledWith(
        expect.objectContaining({
          multiple: true,
        }),
      )
    })

    it('should return null when cancelled', async () => {
      mockOpen.mockResolvedValue(null)

      const result = await FileUtils.selectFiles()

      expect(result).toBeNull()
    })

    it('should return null on error', async () => {
      mockOpen.mockRejectedValue(new Error('Error'))

      const result = await FileUtils.selectFiles()

      expect(result).toBeNull()
    })
  })

  describe('readDirectory (with Tauri mock)', () => {
    it('should return directory contents', async () => {
      setupInvokeMocks({
        read_directory: ['folder1', 'folder2'],
      })

      const result = await FileUtils.readDirectory('/music')

      expect(result).toEqual(['folder1', 'folder2'])
      expect(mockInvoke).toHaveBeenCalledWith('read_directory', { path: '/music' })
    })

    it('should return empty array on error', async () => {
      mockInvoke.mockRejectedValue(new Error('Not found'))

      const result = await FileUtils.readDirectory('/invalid')

      expect(result).toEqual([])
    })
  })

  describe('getAudioFiles (with Tauri mock)', () => {
    it('should return playlist with audio files', async () => {
      const mockPlaylist = {
        name: 'Rock',
        files: ['/music/song1.mp3', '/music/song2.mp3'],
      }
      setupInvokeMocks({
        get_audio_files: mockPlaylist,
      })

      const result = await FileUtils.getAudioFiles('/music/Rock')

      expect(result).toEqual(mockPlaylist)
    })

    it('should return empty playlist on error', async () => {
      mockInvoke.mockRejectedValue(new Error('Error'))

      const result = await FileUtils.getAudioFiles('/invalid')

      expect(result).toEqual({ name: '', files: [] })
    })
  })

  describe('fileExists (with Tauri mock)', () => {
    it('should return true when file exists', async () => {
      setupInvokeMocks({
        check_file_exists: true,
      })

      const result = await FileUtils.fileExists('/music/song.mp3')

      expect(result).toBe(true)
    })

    it('should return false when file does not exist', async () => {
      setupInvokeMocks({
        check_file_exists: false,
      })

      const result = await FileUtils.fileExists('/music/missing.mp3')

      expect(result).toBe(false)
    })

    it('should return false on error', async () => {
      mockInvoke.mockRejectedValue(new Error('Error'))

      const result = await FileUtils.fileExists('/invalid')

      expect(result).toBe(false)
    })
  })

  describe('readFile (with Tauri mock)', () => {
    it('should return file content', async () => {
      setupInvokeMocks({
        read_lyrics_file: '[00:01.00]Hello world',
      })

      const result = await FileUtils.readFile('/music/song.lrc')

      expect(result).toBe('[00:01.00]Hello world')
    })

    it('should return empty string on error', async () => {
      mockInvoke.mockRejectedValue(new Error('Error'))

      const result = await FileUtils.readFile('/invalid')

      expect(result).toBe('')
    })
  })

  describe('findLyricsFile (with Tauri mock)', () => {
    it('should find LRC file', async () => {
      setupInvokeMocks({
        check_file_exists: (args: { path: string }) => args.path.endsWith('.lrc'),
      })

      const result = await FileUtils.findLyricsFile('/music/song.mp3')

      expect(result).toBe('/music/song.lrc')
    })

    it('should find ASS file when LRC not found', async () => {
      setupInvokeMocks({
        check_file_exists: (args: { path: string }) => args.path.endsWith('.ass'),
      })

      const result = await FileUtils.findLyricsFile('/music/song.mp3')

      expect(result).toBe('/music/song.ass')
    })

    it('should find SRT file when others not found', async () => {
      setupInvokeMocks({
        check_file_exists: (args: { path: string }) => args.path.endsWith('.srt'),
      })

      const result = await FileUtils.findLyricsFile('/music/song.mp3')

      expect(result).toBe('/music/song.srt')
    })

    it('should return null when no lyrics file found', async () => {
      setupInvokeMocks({
        check_file_exists: false,
      })

      const result = await FileUtils.findLyricsFile('/music/song.mp3')

      expect(result).toBeNull()
    })
  })
})
