import { invoke } from '@tauri-apps/api/core'

/**
 * 歌曲标题提取工具类
 */
export class TitleExtractor {
  /**
   * 提取歌曲标题
   * @param {string} filePath - 音频文件路径
   * @param {Object} config - 标题提取配置
   * @returns {Promise<Object>} 包含标题信息的对象
   */
  static async extractTitle(filePath, config = {}) {
    try {
      const { preferMetadata = true } = config
      
      // 优先从元数据获取信息
      if (preferMetadata) {
        try {
          const metadata = await invoke('get_track_metadata', { path: filePath })
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
          console.warn('Failed to get metadata for:', filePath, error)
          // 获取元数据失败，继续执行，尝试从文件名解析
        }
      }
      
      // 回退到从文件名解析
      return this.parseFromFileName(filePath, config)
      
    } catch (error) {
      console.error('Error extracting title:', error)
      // 出现意外错误时的最终回退方案
      const fileName = this.getFileName(filePath, config.hideFileExtension)
      return {
        fileName: fileName,
        title: this.cleanTitle(fileName), // 清理回退的标题
        artist: '',
        album: '',
        isFromMetadata: false
      }
    }
  }

  /**
   * 从文件名解析标题信息
   * @param {string} filePath - 音频文件路径
   * @param {Object} config - 标题提取配置
   * @returns {Object} 包含标题信息的对象
   */
  static parseFromFileName(filePath, config = {}) {
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
      ].filter(s => s && s.length > 0);

      for (const sep of prioritizedSeparators) {
        // 使用 lastIndexOf 来处理 "艺术家 - 歌曲 - 专辑" 这类情况
        const lastIndex = fileName.lastIndexOf(sep);
        
        // 确保分隔符不在字符串的开头或结尾
        if (lastIndex > 0 && lastIndex < fileName.length - sep.length) {
          const potentialArtist = fileName.substring(0, lastIndex);
          const potentialTitle = fileName.substring(lastIndex + sep.length);

          // 如果分割后两部分都不为空，则认为解析成功
          if (potentialArtist && potentialTitle) {
            artist = this.cleanTitle(potentialArtist);
            title = this.cleanTitle(potentialTitle);
            // 解析成功，跳出循环
            break;
          }
        }
      }
    }
    
    // 如果一轮循环后没有解析出艺术家，确保标题是干净的
    if (artist === '' && title === fileName) {
      title = this.cleanTitle(fileName);
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
   * @param {string} filePath - 文件路径
   * @param {boolean} hideExtension - 是否隐藏扩展名
   * @returns {string} 文件名
   */
  static getFileName(filePath, hideExtension = true) {
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
   * @param {string} title - 原始标题
   * @returns {string} 清理后的标题
   */
  static cleanTitle(title) {
    if (!title) return ''
    
    return title
      .trim()
      .replace(/\s+/g, ' ') // 替换多个空格为单个空格
      .replace(/^[\s\-_]+|[\s\-_]+$/g, '') // 去除开头和结尾的特殊字符
  }

  /**
   * 格式化播放列表名称
   * @param {string} folderPath - 文件夹路径
   * @param {string} format - 格式化字符串，支持 {folderName}
   * @returns {string} 格式化后的播放列表名称
   */
  static formatPlaylistName(folderPath, format = '{folderName}') {
    const parts = folderPath.split(/[/\\]/)
    const folderName = parts[parts.length - 1] || folderPath
    
    return format.replace('{folderName}', folderName)
  }

  /**
   * 判断字符串是否为有效的分隔符
   * @param {string} separator - 分隔符
   * @returns {boolean} 是否为有效分隔符
   */
  static isValidSeparator(separator) {
    return typeof separator === 'string' && separator.trim() !== ''
  }

  /**
   * 获取所有有效的分隔符
   * @param {Array} separators - 分隔符数组
   * @returns {Array} 有效的分隔符数组
   */
  static getValidSeparators(separators) {
    return separators.filter(sep => this.isValidSeparator(sep))
  }

  /**
   * 测试文件名解析效果
   * @param {string} fileName - 测试文件名
   * @param {Object} config - 标题提取配置
   * @returns {Object} 解析结果
   */
  static testParse(fileName, config = {}) {
    const testPath = `/test/${fileName}.mp3`
    return this.parseFromFileName(testPath, config)
  }
}