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

/** 标题提取: 优先音频元数据, 缺失时按文件名规则解析(规则见 parseFromFileName) */
export class TitleExtractor {
  /** 批量提取, 元数据只走一次 IPC, 缺 title 的条目回落文件名解析 */
  static async extractTitlesBatch(
    filePaths: string[],
    config: Partial<TitleExtractionConfig> = {},
  ): Promise<Map<string, TitleInfo>> {
    const { preferMetadata = true } = config
    const result = new Map<string, TitleInfo>()

    if (!filePaths || filePaths.length === 0) {
      return result
    }

    const metadataMap = new Map<string, TrackMetadata>()
    if (preferMetadata) {
      try {
        const metadataList = await invoke<TrackMetadata[]>('get_tracks_metadata_batch', {
          paths: filePaths,
        })
        for (const metadata of metadataList) {
          if (metadata && metadata.path) {
            metadataMap.set(metadata.path, metadata)
          }
        }
      } catch (error) {
        logger.warn('Failed to get batch metadata:', error)
        // 批量 IPC 失败不致命: metadataMap 留空, 后面整批都走文件名解析
      }
    }

    for (const filePath of filePaths) {
      const metadata = metadataMap.get(filePath)

      if (metadata && metadata.title) {
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
          isFromMetadata: true,
        })
      } else {
        const parsed = this.parseFromFileName(filePath, config)
        // 元数据缺 title 时仍合并其中的音频参数, 只是标题改用文件名解析结果
        result.set(filePath, {
          ...parsed,
          duration: metadata?.duration || 0,
          bitrate: metadata?.bitrate || null,
          sampleRate: metadata?.sampleRate || null,
          channels: metadata?.channels || null,
          bitDepth: metadata?.bitDepth || null,
          format: metadata?.format || null,
        })
      }
    }

    return result
  }

  /** 单文件提取, 优先级与批量版一致 */
  static async extractTitle(
    filePath: string,
    config: Partial<TitleExtractionConfig> = {},
  ): Promise<TitleInfo> {
    try {
      const { preferMetadata = true } = config

      if (preferMetadata) {
        try {
          const metadata = await invoke<TrackMetadata>('get_track_metadata', { path: filePath })
          if (metadata && metadata.title) {
            return {
              fileName: this.getFileName(filePath, config.hideFileExtension),
              title: this.cleanTitle(metadata.title),
              artist: this.cleanTitle(metadata.artist || ''),
              album: this.cleanTitle(metadata.album || ''),
              isFromMetadata: true,
            }
          }
        } catch (error) {
          logger.warn('Failed to get metadata for:', filePath, error)
          // 失败处理同 extractTitlesBatch: 落回文件名解析
        }
      }

      return this.parseFromFileName(filePath, config)
    } catch (error) {
      logger.error('Error extracting title:', error)
      // 最终兜底: 连文件名解析都抛错时, 用去掉扩展名的文件名当标题
      const fileName = this.getFileName(filePath, config.hideFileExtension)
      return {
        fileName,
        title: this.cleanTitle(fileName),
        artist: '',
        album: '',
        isFromMetadata: false,
      }
    }
  }

  /**
   * 按文件名解析标题与艺术家, 只用用户配置的 separator。
   *
   * 优先级: 先试带空格的变体(如 ' - '), 再试原样 separator; 取最后一次出现的位置切分,
   * 使 "艺术家 - 歌曲 - 专辑" 的末段成为标题; 切点不能在首尾, 且切出的两侧都非空才算命中。
   */
  static parseFromFileName(
    filePath: string,
    config: Partial<TitleExtractionConfig> = {},
  ): TitleInfo {
    const { separator = '-', hideFileExtension = true, parseArtistTitle = true } = config

    const fileName = this.getFileName(filePath, hideFileExtension)

    let title = fileName
    let artist = ''

    if (parseArtistTitle) {
      const trimmedSep = separator.trim()
      const withSpaces = trimmedSep ? ` ${trimmedSep} ` : ''
      // 分隔符优先级与切分规则见方法注释
      const prioritizedSeparators =
        withSpaces && withSpaces !== separator ? [withSpaces, separator] : [separator]

      for (const sep of prioritizedSeparators) {
        const lastIndex = fileName.lastIndexOf(sep)

        if (lastIndex > 0 && lastIndex < fileName.length - sep.length) {
          const potentialArtist = fileName.substring(0, lastIndex)
          const potentialTitle = fileName.substring(lastIndex + sep.length)

          if (potentialArtist && potentialTitle) {
            artist = this.cleanTitle(potentialArtist)
            title = this.cleanTitle(potentialTitle)
            break
          }
        }
      }
    }

    // 没切出艺术家时标题仍要去掉首尾杂字符
    if (artist === '' && title === fileName) {
      title = this.cleanTitle(fileName)
    }

    return {
      fileName,
      title,
      artist,
      album: '',
      isFromMetadata: false,
    }
  }

  /** 取路径末段, / 与 \ 都算分隔符; 去扩展名时要求 . 不在首位, 因此 .hidden 不会被截掉 */
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

  /** 连续空白压成单个空格, 并去掉首尾的空白/连字符/下划线 */
  static cleanTitle(title: string): string {
    if (!title) return ''

    return title
      .trim()
      .replace(/\s+/g, ' ')
      .replace(/^[\s\-_]+|[\s\-_]+$/g, '')
  }

  /** 用末级目录名替换 format 里的 {folderName} 占位符 */
  static formatPlaylistName(folderPath: string, format: string = '{folderName}'): string {
    const parts = folderPath.split(/[/\\]/)
    const folderName = parts[parts.length - 1] || folderPath

    return format.replace('{folderName}', folderName)
  }

  /** 供测试直接喂裸文件名: 拼成假路径后走与真实文件相同的解析分支(含去扩展名) */
  static testParse(fileName: string, config: Partial<TitleExtractionConfig> = {}): TitleInfo {
    const testPath = `/test/${fileName}.mp3`
    return this.parseFromFileName(testPath, config)
  }
}
