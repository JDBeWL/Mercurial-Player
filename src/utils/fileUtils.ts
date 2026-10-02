import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'
import logger from './logger'
import { ErrorType, ErrorSeverity, handlePromise } from './errorHandler'
import { formatTime } from './format'
import type { Playlist } from '@/types'

/**
 * 文件工具类，处理文件和目录相关操作
 */
export class FileUtils {
  /** 打开文件夹选择对话框。Android 上 plugin-dialog 的目录选择不可用（分区存储需 SAF），改为调系统
   *  目录选择器并轮询返回已保存的 content:// 树 URI；桌面保持原生对话框。 */
  static async selectFolder(options: Parameters<typeof open>[0] = {}): Promise<string | null> {
    const platform = await invoke<string>('get_platform')
    if (platform === 'android') {
      return this.selectFolderAndroid()
    }

    const result = await handlePromise(
      open({
        directory: true,
        multiple: false,
        title: 'Select a folder',
        ...options,
      }),
      {
        type: ErrorType.FILE_PERMISSION_DENIED,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'selectFolder' },
        showToUser: false,
        throw: false,
      },
    )

    return result.success ? (result.data as string | null) : null
  }

  /** Android：调起 SAF 系统目录选择器，轮询等待授权结果（树 URI 持久化在 Kotlin 侧） */
  private static async selectFolderAndroid(): Promise<string | null> {
    type PickState = { uri?: string | null; version: number; error?: string | null }
    const before = await invoke<PickState>('saf_get_pick_state')
    await invoke('saf_request_pick')

    // 轮询异步的选择器直到 version 变化，超时 3 分钟。
    // 不能比较 URI：重授权同一目录 URI 不变，会永远等不到。
    // 失败也递增 version 并带 error，据此提前收尾。
    const timeoutMs = 3 * 60 * 1000
    const startedAt = Date.now()
    let treeUri: string | null = before.uri ?? null
    let version = before.version
    let error: string | null = null
    while (Date.now() - startedAt < timeoutMs) {
      await new Promise((resolve) => setTimeout(resolve, 500))
      const state = await invoke<PickState>('saf_get_pick_state')
      treeUri = state.uri ?? null
      version = state.version
      error = state.error ?? null
      if (version !== before.version) break
    }

    if (error) {
      logger.warn(`SAF pick failed: ${error}`)
      return null
    }
    if (!treeUri || version === before.version) {
      logger.warn('SAF pick cancelled or timed out')
      return null
    }
    return treeUri
  }

  /**
   * 打开文件选择对话框
   */
  static async selectFiles(options: Parameters<typeof open>[0] = {}): Promise<string[] | null> {
    const result = await handlePromise(
      open({
        multiple: true,
        title: 'Select files',
        ...options,
      }),
      {
        type: ErrorType.FILE_PERMISSION_DENIED,
        severity: ErrorSeverity.MEDIUM,
        context: { action: 'selectFiles' },
        showToUser: false,
        throw: false,
      },
    )

    return result.success ? (result.data as string[] | null) : null
  }

  /**
   * 读取目录中的子文件夹
   */
  static async readDirectory(path: string): Promise<string[]> {
    const result = await handlePromise(invoke<string[]>('read_directory', { path }), {
      type: ErrorType.FILE_READ_ERROR,
      severity: ErrorSeverity.MEDIUM,
      context: { path, action: 'readDirectory' },
      showToUser: false,
      throw: false,
    })

    return result.success ? result.data! : []
  }

  /**
   * 获取目录中的音频文件
   */
  static async getAudioFiles(path: string): Promise<Playlist> {
    const result = await handlePromise(invoke<Playlist>('get_audio_files', { path }), {
      type: ErrorType.FILE_READ_ERROR,
      severity: ErrorSeverity.MEDIUM,
      context: { path, action: 'getAudioFiles' },
      showToUser: false,
      throw: false,
    })

    return result.success ? result.data! : { name: '', files: [] }
  }

  /**
   * 检查文件是否存在
   */
  static async fileExists(path: string): Promise<boolean> {
    const result = await handlePromise(invoke<boolean>('check_file_exists', { path }), {
      type: ErrorType.FILE_READ_ERROR,
      severity: ErrorSeverity.LOW,
      context: { path, action: 'fileExists' },
      showToUser: false,
      throw: false,
    })

    return result.success ? result.data! : false
  }

  /**
   * 读取文件内容
   */
  static async readFile(path: string): Promise<string> {
    const result = await handlePromise(invoke<string>('read_lyrics_file', { path }), {
      type: ErrorType.FILE_READ_ERROR,
      severity: ErrorSeverity.MEDIUM,
      context: { path, action: 'readFile' },
      showToUser: false,
      throw: false,
    })

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

  /** 获取音轨显示名称，优先级: displayTitle 大于 title 大于 文件名(根据 hideExtension 决定是否含扩展名)。
   *  hideExtension=true 时会对 displayTitle/title 也去除与文件扩展名匹配的后缀，防止元数据 title 恰好
   *  含扩展名时(常见于 wav 文件)绕过隐藏设置。 */
  static getTrackDisplayName(
    track: { displayTitle?: string; title?: string; name?: string; path: string },
    hideExtension: boolean = true,
  ): string {
    // 当需要隐藏扩展名时,去除标题末尾与文件扩展名匹配的部分
    // 例: track.path="01.wav", title="01.曲名.wav" -> "01.曲名"
    const stripTrailingExt = (s: string): string => {
      if (!hideExtension || !s) return s
      const ext = this.getFileExtension(track.path)
      if (!ext) return s
      const suffix = `.${ext}`
      return s.toLowerCase().endsWith(suffix) ? s.slice(0, -suffix.length) : s
    }

    if (track.displayTitle) return stripTrailingExt(track.displayTitle)
    if (track.title) return stripTrailingExt(track.title)
    return hideExtension
      ? this.getFileNameWithoutExtension(track.path)
      : track.name || this.getFileName(track.path)
  }

  /**
   * 获取文件名（含扩展名）
   */
  static getFileName(filePath: string): string {
    const normalizedPath = filePath.replace(/[\\/]+$/, '')
    if (!normalizedPath) return filePath

    // Android：content:// URI 的末段是 URL 编码的 document id
    // （如 .../document/primary%3AMusic%2FSong.mp3），必须先解码再取文件名，
    // 否则界面会直接显示 primary%3AMusic%2F…
    if (normalizedPath.startsWith('content://')) {
      let lastSegment = normalizedPath.split('/').pop() || normalizedPath
      try {
        lastSegment = decodeURIComponent(lastSegment)
      } catch {
        // 非法百分号编码时保留原样，不因解码失败影响整条列表
      }
      const decodedParts = lastSegment.split(/[/\\]/)
      return decodedParts[decodedParts.length - 1] || lastSegment
    }

    const parts = normalizedPath.split(/[/\\]/)
    return parts[parts.length - 1] || normalizedPath
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
    const normalizedPath = filePath.replace(/[\\/]+$/, '')
    const lastSeparatorIndex = Math.max(
      normalizedPath.lastIndexOf('/'),
      normalizedPath.lastIndexOf('\\'),
    )

    if (lastSeparatorIndex < 0) return ''
    if (lastSeparatorIndex === 0) return normalizedPath[0]!

    return normalizedPath.slice(0, lastSeparatorIndex).replace(/\\/g, '/')
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
   * 拼接路径片段，自动兼容分隔符
   */
  static joinPath(...segments: string[]): string {
    const validSegments = segments.filter((segment) => segment.length > 0)
    if (validSegments.length === 0) return ''

    const separator = validSegments[0]!.includes('\\') ? '\\' : '/'
    const [firstSegment, ...restSegments] = validSegments
    const normalizedFirst = firstSegment!.replace(/[\\/]+$/, '')
    const normalizedRest = restSegments.map((segment) => segment.replace(/^[\\/]+|[\\/]+$/g, ''))

    return [normalizedFirst, ...normalizedRest].filter(Boolean).join(separator)
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
      const lyricsPath = this.joinPath(directory, `${baseName}.${ext}`)
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
   * 格式化文件大小（非法输入：负数 / 非有限值返回 "--"）
   */
  static formatFileSize(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes < 0) return '--'
    if (bytes === 0) return '0 Bytes'

    const k = 1024
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB']
    const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1)

    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
  }

  /**
   * 格式化时间（秒转换为 mm:ss 或 hh:mm:ss）—— 统一实现见 utils/format.ts
   */
  static formatTime(seconds: number): string {
    return formatTime(seconds)
  }
}

export default FileUtils
