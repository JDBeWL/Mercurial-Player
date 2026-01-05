import { invoke } from '@tauri-apps/api/core'
import logger from './logger'
import type { TitleExtractionConfig } from '@/types'

interface TitleInfo {
  fileName: string
  title: string
  artist: string
  album: string
  duration?: number
  bitrate?: number | null
  sampleRate?: number | null
  channels?: number | null
  bitDepth?: number | null
  format?: string | null
  isFromMetadata: boolean
}

interface TrackMetadata {
  path: string
  title?: string
  artist?: string
  album?: string
  duration?: number
  bitrate?: number | null
  sampleRate?: number | null
  channels?: number | null
  bitDepth?: number | null
  format?: string | null
}

/**
 * 歌曲标题提取工具类
 */
export class TitleExtractor {
  /**
   * 批量提取歌曲标题（减少 IPC 调用）
   */
  static async extractTitlesBatch(
    filePaths: string[],
    config: Partial<TitleExtractionConfig> = {}
  ): Promise<Map<string, TitleInfo>> {
    const { preferMetadata = true } = config
    const result = new Map<string, TitleInfo>()

    if (!filePaths || filePaths.length === 0) {
      return result
    }

    // 使用批量 API 获取所有元数据
    const metadataMap = new Map<string, TrackMetadata>()
    if (preferMetadata) {
      try {
        const metadataList = await invoke<TrackMetadata[]>('get_tracks_metadata_batch', { paths: filePaths })
        // 将结果转换为 Map 以便快速查找
        for (const metadata of metadataList) {
          if (metadata && metadata.path) {
            metadataMap.set(metadata.path, metadata)
          }
        }
      } catch (error) {
        logger.warn('Failed to get batch metadata:', error)
        // 批量获取失败，继续使用文件名解析
      }
    }

    // 处理每个文件
    for (const filePath of filePaths) {
      const metadata = metadataMap.get(filePath)

      if (metadata && metadata.title) {
        // 使用元数据
        result.set(filePath, {
          fileName: this.getFileName(filePath, config.hideFileExtension),
          title: this.cleanTitle(metadata.title),
          artist: this.cleanTitle(metadata.artist || ''),
          album: this.cleanTitle(metadata.album || ''),
          duration: metadata.duration || 0,
          bitrate: metadata.bitrate || null,
          sampleRate: metadata.sampleRate || null,
          channels: metadata.channels || null,
          bitDepth: metadata.bitDepth || null,
          format: metadata.format || null,
          isFromMetadata: true
        })
      } else {
        // 回退到文件名解析
        const parsed = this.parseFromFileName(filePath, config)
        // 如果有元数据但没有 title，仍然可以获取音频信息
        result.set(filePath, {
          ...parsed,
          duration: metadata?.duration || 0,
          bitrate: metadata?.bitrate || null,
          sampleRate: metadata?.sampleRate || null,
          channels: metadata?.channels || null,
          bitDepth: metadata?.bitDepth || null,
          format: metadata?.format || null
        })
      }
    }

    return result
  }

  /**
   * 提取歌曲标题（单个文件，保持向后兼容）
   */
  static async extractTitle(
    filePath: string,
    config: Partial<TitleExtractionConfig> = {}
  ): Promise<TitleInfo> {
    try {
      const { preferMetadata = true } = config

      // 优先从元数据获取信息
      if (preferMetadata) {
        try {
          const metadata = await invoke<TrackMetadata>('get_track_metadata', { path: filePath })
          if (metadata && metadata.title) {
            // 获取到元数据，清理后返回
            return {
              fileName: this.getFileName(filePath, config.hideFileExtension),
              title: this.cleanTitle(metadata.title),
              artist: this.cleanTitle(metadata.artist || ''),
              album: this.cleanTitle(metadata.album || ''),
              isFromMetadata: true
            }
          }
        } catch (error) {
          logger.warn('Failed to get metadata for:', filePath, error)
          // 获取元数据失败，继续执行，尝试从文件名解析
        }
      }

      // 回退到从文件名解析
      return this.parseFromFileName(filePath, config)

    } catch (error) {
      logger.error('Error extracting title:', error)
      // 出现意外错误时的最终回退方案
      const fileName = this.getFileName(filePath, config.hideFileExtension)
      return {
        fileName: fileName,
        title: this.cleanTitle(fileName),
        artist: '',
        album: '',
        isFromMetadata: false
      }
    }
  }

  /**
   * 从文件名解析标题信息
   */
  static parseFromFileName(
    filePath: string,
    config: Partial<TitleExtractionConfig> = {}
  ): TitleInfo {
    const {
      separator = '-',
      customSeparators = ['-', '_', '.', ' '],
      hideFileExtension = true,
      parseArtistTitle = true
    } = config

    const fileName = this.getFileName(filePath, hideFileExtension)

    let title = fileName
    let artist = ''

    if (parseArtistTitle) {
      // 定义一个带优先级的分隔符列表
      const prioritizedSeparators = [
        ' - ', // ' - ' 是最高优先级的
        ...new Set([separator, ...customSeparators])
      ].filter(s => s && s.length > 0)

      for (const sep of prioritizedSeparators) {
        // 使用 lastIndexOf 来处理 "艺术家 - 歌曲 - 专辑" 这类情况
        const lastIndex = fileName.lastIndexOf(sep)

        // 确保分隔符不在字符串的开头或结尾
        if (lastIndex > 0 && lastIndex < fileName.length - sep.length) {
          const potentialArtist = fileName.substring(0, lastIndex)
          const potentialTitle = fileName.substring(lastIndex + sep.length)

          // 如果分割后两部分都不为空，则认为解析成功
          if (potentialArtist && potentialTitle) {
            artist = this.cleanTitle(potentialArtist)
            title = this.cleanTitle(potentialTitle)
            // 解析成功，跳出循环
            break
          }
        }
      }
    }

    // 如果一轮循环后没有解析出艺术家，确保标题是干净的
    if (artist === '' && title === fileName) {
      title = this.cleanTitle(fileName)
    }

    return {
      fileName: fileName,
      title: title,
      artist: artist,
      album: '',
      isFromMetadata: false
    }
  }

  /**
   * 获取文件名（可选择是否包含扩展名）
   */
  static getFileName(filePath: string, hideExtension: boolean = true): string {
    const parts = filePath.split(/[/\\]/)
    let fileName = parts[parts.length - 1] || filePath

    if (hideExtension) {
      const lastDotIndex = fileName.lastIndexOf('.')
      if (lastDotIndex > 0) {
        fileName = fileName.substring(0, lastDotIndex)
      }
    }

    return fileName
  }

  /**
   * 清理标题中的多余空格和特殊字符
   */
  static cleanTitle(title: string): string {
    if (!title) return ''

    return title
      .trim()
      .replace(/\s+/g, ' ') // 替换多个空格为单个空格
      .replace(/^[\s\-_]+|[\s\-_]+$/g, '') // 去除开头和结尾的特殊字符
  }

  /**
   * 格式化播放列表名称
   */
  static formatPlaylistName(folderPath: string, format: string = '{folderName}'): string {
    const parts = folderPath.split(/[/\\]/)
    const folderName = parts[parts.length - 1] || folderPath

    return format.replace('{folderName}', folderName)
  }

  /**
   * 判断字符串是否为有效的分隔符
   */
  static isValidSeparator(separator: string): boolean {
    return typeof separator === 'string' && separator.trim() !== ''
  }

  /**
   * 获取所有有效的分隔符
   */
  static getValidSeparators(separators: string[]): string[] {
    return separators.filter(sep => this.isValidSeparator(sep))
  }

  /**
   * 测试文件名解析效果
   */
  static testParse(fileName: string, config: Partial<TitleExtractionConfig> = {}): TitleInfo {
    const testPath = `/test/${fileName}.mp3`
    return this.parseFromFileName(testPath, config)
  }
}
