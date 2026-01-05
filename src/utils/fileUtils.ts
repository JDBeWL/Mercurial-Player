import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import logger from './logger'
import { ErrorType, ErrorSeverity, handlePromise } from './errorHandler'
import type { Playlist } from '@/types'

/**
 * 文件工具类，处理文件和目录相关操作
 */
export class FileUtils {
  /**
   * 打开文件夹选择对话框
   */
  static async selectFolder(options: Parameters<typeof open>[0] = {}): Promise<string | null> {
    const result = await handlePromise(
      open({
        directory: true,
        multiple: false,
        title: 'Select a folder',
        ...options
      }),
      {
        type: ErrorType.FILE_PERMISSION_DENIED,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'selectFolder' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? (result.data as string | null) : null
  }

  /**
   * 打开文件选择对话框
   */
  static async selectFiles(options: Parameters<typeof open>[0] = {}): Promise<string[] | null> {
    const result = await handlePromise(
      open({
        multiple: true,
        title: 'Select files',
        ...options
      }),
      {
        type: ErrorType.FILE_PERMISSION_DENIED,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'selectFiles' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? (result.data as string[] | null) : null
  }

  /**
   * 读取目录中的子文件夹
   */
  static async readDirectory(path: string): Promise<string[]> {
    const result = await handlePromise(
      invoke<string[]>('read_directory', { path }),
      {
        type: ErrorType.FILE_READ_ERROR,
        severity: ErrorSeverity.MEDIUM,
        context: { path, action: 'readDirectory' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? result.data! : []
  }

  /**
   * 获取目录中的音频文件
   */
  static async getAudioFiles(path: string): Promise<Playlist> {
    const result = await handlePromise(
      invoke<Playlist>('get_audio_files', { path }),
      {
        type: ErrorType.FILE_READ_ERROR,
        severity: ErrorSeverity.MEDIUM,
        context: { path, action: 'getAudioFiles' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? result.data! : { name: '', files: [] }
  }

  /**
   * 检查文件是否存在
   */
  static async fileExists(path: string): Promise<boolean> {
    const result = await handlePromise(
      invoke<boolean>('check_file_exists', { path }),
      {
        type: ErrorType.FILE_READ_ERROR,
        severity: ErrorSeverity.LOW,
        context: { path, action: 'fileExists' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? result.data! : false
  }

  /**
   * 读取文件内容
   */
  static async readFile(path: string): Promise<string> {
    const result = await handlePromise(
      invoke<string>('read_lyrics_file', { path }),
      {
        type: ErrorType.FILE_READ_ERROR,
        severity: ErrorSeverity.MEDIUM,
        context: { path, action: 'readFile' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? result.data! : ''
  }

  /**
   * 获取文件名（不含扩展名）
   */
  static getFileNameWithoutExtension(filePath: string): string {
    const fileName = this.getFileName(filePath)
    const lastDotIndex = fileName.lastIndexOf('.')
    return lastDotIndex > 0 ? fileName.substring(0, lastDotIndex) : fileName
  }

  /**
   * 获取文件名（含扩展名）
   */
  static getFileName(filePath: string): string {
    const parts = filePath.split(/[/\\]/)
    return parts[parts.length - 1] || filePath
  }

  /**
   * 获取文件扩展名
   */
  static getFileExtension(filePath: string): string {
    const fileName = this.getFileName(filePath)
    const lastDotIndex = fileName.lastIndexOf('.')
    return lastDotIndex > 0 ? fileName.substring(lastDotIndex + 1).toLowerCase() : ''
  }

  /**
   * 获取文件所在目录路径
   */
  static getDirectoryPath(filePath: string): string {
    const parts = filePath.split(/[/\\]/)
    return parts.slice(0, -1).join('/') || ''
  }

  /**
   * 检查文件是否为音频文件
   */
  static isAudioFile(filePath: string): boolean {
    const extension = this.getFileExtension(filePath)
    const audioExtensions = ['mp3', 'flac', 'wav', 'ogg', 'm4a', 'aac', 'wma']
    return audioExtensions.includes(extension)
  }

  /**
   * 检查文件是否为歌词文件
   */
  static isLyricsFile(filePath: string): boolean {
    const extension = this.getFileExtension(filePath)
    const lyricsExtensions = ['lrc', 'ass', 'srt']
    return lyricsExtensions.includes(extension)
  }

  /**
   * 根据音频文件路径查找对应的歌词文件
   */
  static async findLyricsFile(audioPath: string): Promise<string | null> {
    const baseName = this.getFileNameWithoutExtension(audioPath)
    const directory = this.getDirectoryPath(audioPath)
    
    // 尝试常见的歌词文件扩展名
    const lyricsExtensions = ['lrc', 'ass', 'srt']
    
    for (const ext of lyricsExtensions) {
      const lyricsPath = `${directory}/${baseName}.${ext}`
      try {
        // 先检查文件是否存在
        const exists = await this.fileExists(lyricsPath)
        if (exists) {
          return lyricsPath
        }
      } catch {
        // 文件不存在，继续尝试下一个扩展名
      }
    }
    
    return null
  }

  /**
   * 格式化文件大小
   */
  static formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 Bytes'
    
    const k = 1024
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
  }

  /**
   * 格式化时间（秒转换为 mm:ss 或 hh:mm:ss）
   */
  static formatTime(seconds: number): string {
    if (isNaN(seconds) || !isFinite(seconds)) return '0:00'
    
    const hours = Math.floor(seconds / 3600)
    const minutes = Math.floor((seconds % 3600) / 60)
    const remainingSeconds = Math.floor(seconds % 60)
    
    if (hours > 0) {
      return `${hours}:${minutes.toString().padStart(2, '0')}:${remainingSeconds.toString().padStart(2, '0')}`
    } else {
      return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`
    }
  }
}

export default FileUtils
