import { invoke } from '@tauri-apps/api/core'
import { TitleExtractor } from './titleExtractor.js'
import logger from './logger'

/**
 * 播放列表管理器
 * 支持子目录扫描和结构化展示
 */
export class PlaylistManager {

  /**
   * 获取音频文件树结构
   * @param {string} basePath - 基础路径
   * @param {Object} config - 配置对象
   * @returns {Promise<Object>} 文件树结构
   */
  static async getAudioFileTree(basePath, config = {}) {
    try {
      const {
        enableSubdirectoryScan = true,
        maxDepth = 3,
        ignoreHiddenFolders = true,
        folderBlacklist = []
      } = config.directoryScan || {}

      // 获取目录结构
      const directoryTree = await this.scanDirectoryTree(basePath, {
        enableSubdirectoryScan,
        maxDepth,
        ignoreHiddenFolders,
        folderBlacklist
      })

      // 从目录树中提取音频文件并生成播放列表
      const playlists = await this.generatePlaylistsFromTree(directoryTree, config)

      return {
        basePath,
        directoryTree,
        playlists,
        totalFiles: this.countAudioFiles(directoryTree)
      }

    } catch (error) {
      logger.error('Error getting audio file tree:', error)
      throw error
    }
  }

  /**
   * 扫描目录树结构 - 使用异步分块避免阻塞
   */
  static async scanDirectoryTree(basePath, config) {
    const { enableSubdirectoryScan, maxDepth, ignoreHiddenFolders, folderBlacklist } = config
    let scannedCount = 0
    const YIELD_INTERVAL = 10 // 每扫描 10 个目录让出一次主线程

    const scanDirectory = async (path, currentDepth = 0) => {
      if (currentDepth > maxDepth) {
        return null
      }

      // 定期让出主线程
      scannedCount++
      if (scannedCount % YIELD_INTERVAL === 0) {
        await new Promise(resolve => setTimeout(resolve, 0))
      }

      try {
        // 获取目录下的子目录
        const subdirectories = await invoke('read_directory', { path })

        const node = {
          path,
          name: this.getFolderName(path),
          depth: currentDepth,
          subdirectories: [],
          audioFiles: []
        }

        // 扫描当前目录的音频文件
        if (enableSubdirectoryScan || currentDepth === 0) {
          try {
            const playlist = await invoke('get_audio_files', { path })
            node.audioFiles = playlist.files
          } catch (error) {
            logger.warn(`Failed to get audio files from ${path}:`, error)
          }
        }

        // 递归扫描子目录
        if (enableSubdirectoryScan && currentDepth < maxDepth) {
          for (const subdir of subdirectories) {
            // 检查是否应该忽略这个文件夹
            if (this.shouldIgnoreFolder(subdir, folderBlacklist, ignoreHiddenFolders)) {
              continue
            }

            const subNode = await scanDirectory(subdir, currentDepth + 1)
            if (subNode) {
              node.subdirectories.push(subNode)
            }
          }
        }

        return node

      } catch (error) {
        logger.error(`Error scanning directory ${path}:`, error)
        return null
      }
    }

    return await scanDirectory(basePath)
  }

  /**
   * 从目录树生成播放列表
   */
  static async generatePlaylistsFromTree(directoryTree, config) {
    const playlists = []

    const generatePlaylists = async (node) => {
      if (!node) return

      // 检查是否是最终音频目录（不包含有音频文件的子目录）
      const isFinalAudioDirectory = this.isFinalAudioDirectory(node)

      // 只有最终音频目录才创建播放列表
      if (node.audioFiles.length > 0 && isFinalAudioDirectory) {
        const playlist = {
          name: TitleExtractor.formatPlaylistName(node.path, config.playlist?.playlistNameFormat || '{folderName}'),
          path: node.path,
          files: await this.processAudioFiles(node.audioFiles, config),
          subdirectoryCount: node.subdirectories.length,
          totalFiles: node.audioFiles.length
        }
        playlists.push(playlist)
      }

      // 递归处理子目录
      for (const subdir of node.subdirectories) {
        await generatePlaylists(subdir)
      }
    }

    await generatePlaylists(directoryTree)

    // 如果启用了"全部歌曲"播放列表，生成一个包含所有文件的播放列表
    if (config.playlist?.generateAllSongsPlaylist) {
      const allFiles = this.collectAllAudioFiles(directoryTree)
      if (allFiles.length > 0) {
        playlists.unshift({
          name: '全部歌曲',
          path: directoryTree.path,
          files: await this.processAudioFiles(allFiles, config),
          subdirectoryCount: this.countSubdirectories(directoryTree),
          totalFiles: allFiles.length,
          isAllSongsPlaylist: true
        })
      }
    }

    return playlists
  }

  /**
   * 处理音频文件，应用标题提取规则（使用批量API）
   */
  static async processAudioFiles(files, config) {
    if (!files || files.length === 0) {
      return []
    }

    // 收集所有文件路径
    const filePaths = files.map(file => file.path)

    // 使用批量 API 获取所有标题信息
    const titleInfoMap = await TitleExtractor.extractTitlesBatch(filePaths, config.titleExtraction)

    // 处理每个文件
    const processedFiles = []
    for (const file of files) {
      const titleInfo = titleInfoMap.get(file.path)

      if (titleInfo) {
        processedFiles.push({
          ...file,
          displayTitle: titleInfo.title,
          displayArtist: titleInfo.artist,
          fileName: titleInfo.fileName,
          isFromMetadata: titleInfo.isFromMetadata,
          // 包含音频元数据信息
          duration: titleInfo.duration || file.duration || 0,
          bitrate: titleInfo.bitrate || file.bitrate || null,
          sampleRate: titleInfo.sampleRate || file.sampleRate || null,
          channels: titleInfo.channels || file.channels || null,
          bitDepth: titleInfo.bitDepth || file.bitDepth || null,
          format: titleInfo.format || file.format || null
        })
      } else {
        // 回退方案：如果批量获取失败
        processedFiles.push({
          ...file,
          displayTitle: file.name,
          displayArtist: '',
          fileName: TitleExtractor.getFileName(file.path, config.titleExtraction?.hideFileExtension),
          isFromMetadata: false
        })
      }
    }

    return processedFiles
  }

  /**
   * 获取文件夹名称
   */
  static getFolderName(path) {
    const parts = path.split(/[/\\]/)
    return parts[parts.length - 1] || path
  }

  /**
   * 检查是否应该忽略文件夹
   */
  static shouldIgnoreFolder(folderPath, blacklist, ignoreHiddenFolders) {
    const folderName = this.getFolderName(folderPath)

    // 检查隐藏文件夹
    if (ignoreHiddenFolders && folderName.startsWith('.')) {
      return true
    }

    // 检查黑名单
    if (blacklist.includes(folderName)) {
      return true
    }

    return false
  }

  /**
   * 统计音频文件总数
   */
  static countAudioFiles(node) {
    if (!node) return 0

    let count = node.audioFiles.length

    for (const subdir of node.subdirectories) {
      count += this.countAudioFiles(subdir)
    }

    return count
  }

  /**
   * 统计子目录数量
   */
  static countSubdirectories(node) {
    if (!node) return 0

    let count = node.subdirectories.length

    for (const subdir of node.subdirectories) {
      count += this.countSubdirectories(subdir)
    }

    return count
  }

  /**
   * 收集所有音频文件
   */
  static collectAllAudioFiles(node) {
    if (!node) return []

    let files = [...node.audioFiles]

    for (const subdir of node.subdirectories) {
      files = files.concat(this.collectAllAudioFiles(subdir))
    }

    return files
  }

  /**
   * 搜索音频文件
   */
  static async searchAudioFiles(basePath, searchTerm, config) {
    try {
      const fileTree = await this.getAudioFileTree(basePath, config)
      const results = []

      const searchInNode = (node) => {
        if (!node) return

        // 搜索当前节点的文件
        for (const file of node.audioFiles) {
          const searchFields = [
            file.title || '',
            file.artist || '',
            file.album || '',
            file.name || ''
          ]

          if (searchFields.some(field =>
            field.toLowerCase().includes(searchTerm.toLowerCase())
          )) {
            results.push({
              ...file,
              folderPath: node.path,
              folderName: node.name
            })
          }
        }

        // 递归搜索子目录
        for (const subdir of node.subdirectories) {
          searchInNode(subdir)
        }
      }

      searchInNode(fileTree.directoryTree)

      return results

    } catch (error) {
      logger.error('Error searching audio files:', error)
      throw error
    }
  }

  /**
   * 根据路径获取播放列表
   */
  static async getPlaylistByPath(basePath, targetPath, config) {
    try {
      const fileTree = await this.getAudioFileTree(basePath, config)

      const findPlaylist = (node) => {
        if (!node) return null

        if (node.path === targetPath) {
          return {
            name: TitleExtractor.formatPlaylistName(node.path, config.playlist?.playlistNameFormat || '{folderName}'),
            path: node.path,
            files: node.audioFiles,
            subdirectoryCount: node.subdirectories.length,
            totalFiles: node.audioFiles.length
          }
        }

        for (const subdir of node.subdirectories) {
          const result = findPlaylist(subdir)
          if (result) return result
        }

        return null
      }

      return findPlaylist(fileTree.directoryTree)

    } catch (error) {
      logger.error('Error getting playlist by path:', error)
      throw error
    }
  }

  /**
   * 获取目录统计信息
   */
  static async getDirectoryStats(basePath, config) {
    try {
      const fileTree = await this.getAudioFileTree(basePath, config)

      const stats = {
        totalDirectories: this.countDirectories(fileTree.directoryTree),
        totalAudioFiles: fileTree.totalFiles,
        totalPlaylists: fileTree.playlists.length,
        maxDepth: this.getMaxDepth(fileTree.directoryTree)
      }

      return stats

    } catch (error) {
      logger.error('Error getting directory stats:', error)
      throw error
    }
  }

  /**
   * 统计目录总数
   */
  static countDirectories(node) {
    if (!node) return 0

    let count = 1 // 当前节点

    for (const subdir of node.subdirectories) {
      count += this.countDirectories(subdir)
    }

    return count
  }

  /**
   * 获取最大深度
   */
  static getMaxDepth(node) {
    if (!node || node.subdirectories.length === 0) {
      return node ? node.depth : 0
    }

    let maxDepth = node.depth

    for (const subdir of node.subdirectories) {
      maxDepth = Math.max(maxDepth, this.getMaxDepth(subdir))
    }

    return maxDepth
  }

  /**
   * 检查是否是最终音频目录（不包含有音频文件的子目录）
   */
  static isFinalAudioDirectory(node) {
    if (!node) return true

    // 如果一个目录没有任何子目录，那么它肯定是最终音频目录
    if (node.subdirectories.length === 0) {
      return true
    }

    // 检查子目录中是否有包含音频文件的目录
    for (const subdir of node.subdirectories) {
      // 如果子目录本身有音频文件，那么当前目录不是最终音频目录
      if (subdir.audioFiles.length > 0) {
        return false
      }

      // 递归检查子目录的子目录是否有音频文件
      if (this.hasAudioFilesInSubtree(subdir)) {
        return false
      }
    }

    return true
  }

  /**
   * 检查目录子树中是否有音频文件
   */
  static hasAudioFilesInSubtree(node) {
    if (!node) return false

    // 检查当前目录是否有音频文件
    if (node.audioFiles.length > 0) {
      return true
    }

    // 递归检查子目录
    for (const subdir of node.subdirectories) {
      if (this.hasAudioFilesInSubtree(subdir)) {
        return true
      }
    }

    return false
  }
}