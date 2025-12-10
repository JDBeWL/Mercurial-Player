import { open } from '@tauri-apps/plugin-dialog';
import { readTextFile, exists } from '@tauri-apps/plugin-fs';
import { invoke } from '@tauri-apps/api/core';

/**
 * 文件工具类，处理文件和目录相关操作
 */
export class FileUtils {
  /**
   * 打开文件夹选择对话框
   * @param {Object} options - 对话框选项
   * @returns {Promise<string|null>} 选中的文件夹路径
   */
  static async selectFolder(options = {}) {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select a folder',
        ...options
      });
      
      return selected;
    } catch (error) {
      console.error('Error selecting folder:', error);
      return null;
    }
  }

  /**
   * 打开文件选择对话框
   * @param {Object} options - 对话框选项
   * @returns {Promise<string[]|null>} 选中的文件路径数组
   */
  static async selectFiles(options = {}) {
    try {
      const selected = await open({
        multiple: true,
        title: 'Select files',
        ...options
      });
      
      return selected;
    } catch (error) {
      console.error('Error selecting files:', error);
      return null;
    }
  }

  /**
   * 读取目录中的子文件夹
   * @param {string} path - 目录路径
   * @returns {Promise<string[]>} 子文件夹路径数组
   */
  static async readDirectory(path) {
    try {
      const folders = await invoke('read_directory', { path });
      return folders;
    } catch (error) {
      console.error('Error reading directory:', error);
      return [];
    }
  }

  /**
   * 获取目录中的音频文件
   * @param {string} path - 目录路径
   * @returns {Promise<Object>} 包含音频文件的播放列表对象
   */
  static async getAudioFiles(path) {
    try {
      const playlist = await invoke('get_audio_files', { path });
      return playlist;
    } catch (error) {
      console.error('Error getting audio files:', error);
      return { name: '', files: [] };
    }
  }
  /**
   * 检查文件是否存在
   * @param {string} path - 文件路径
   * @returns {Promise<boolean>} 文件是否存在
   */
  static async fileExists(path) {
    try {
      // 使用后端的check_file_exists命令，确保路径处理一致
      return await invoke('check_file_exists', { path });
    } catch (error) {
      console.error('Error checking file existence:', error);
      return false;
    }
  }
  /**
   * 读取文件内容
   * @param {string} path - 文件路径
   * @returns {Promise<string>} 文件内容
   */
  static async readFile(path) {
    try {
      // 使用后端命令读取文件，而不是直接使用 Tauri 的文件系统 API
      const content = await invoke('read_lyrics_file', { path });
      return content;
    } catch (error) {
      console.error('Error reading file:', error);
      return '';
    }
  }

  /**
   * 获取文件名（不含扩展名）
   * @param {string} filePath - 文件路径
   * @returns {string} 文件名（不含扩展名）
   */
  static getFileNameWithoutExtension(filePath) {
    const fileName = this.getFileName(filePath);
    const lastDotIndex = fileName.lastIndexOf('.');
    return lastDotIndex > 0 ? fileName.substring(0, lastDotIndex) : fileName;
  }

  /**
   * 获取文件名（含扩展名）
   * @param {string} filePath - 文件路径
   * @returns {string} 文件名
   */
  static getFileName(filePath) {
    const parts = filePath.split(/[/\\]/);
    return parts[parts.length - 1] || filePath;
  }

  /**
   * 获取文件扩展名
   * @param {string} filePath - 文件路径
   * @returns {string} 文件扩展名
   */
  static getFileExtension(filePath) {
    const fileName = this.getFileName(filePath);
    const lastDotIndex = fileName.lastIndexOf('.');
    return lastDotIndex > 0 ? fileName.substring(lastDotIndex + 1).toLowerCase() : '';
  }

  /**
   * 获取文件所在目录路径
   * @param {string} filePath - 文件路径
   * @returns {string} 目录路径
   */
  static getDirectoryPath(filePath) {
    const parts = filePath.split(/[/\\]/);
    return parts.slice(0, -1).join('/') || '';
  }

  /**
   * 检查文件是否为音频文件
   * @param {string} filePath - 文件路径
   * @returns {boolean} 是否为音频文件
   */
  static isAudioFile(filePath) {
    const extension = this.getFileExtension(filePath);
    const audioExtensions = ['mp3', 'flac', 'wav', 'ogg', 'm4a', 'aac', 'wma'];
    return audioExtensions.includes(extension);
  }

  /**
   * 检查文件是否为歌词文件
   * @param {string} filePath - 文件路径
   * @returns {boolean} 是否为歌词文件
   */
  static isLyricsFile(filePath) {
    const extension = this.getFileExtension(filePath);
    const lyricsExtensions = ['lrc', 'ass', 'ssa', 'srt'];
    return lyricsExtensions.includes(extension);
  }

  /**
   * 根据音频文件路径查找对应的歌词文件
   * @param {string} audioPath - 音频文件路径
   * @returns {Promise<string|null>} 歌词文件路径
   */
  static async findLyricsFile(audioPath) {
    const baseName = this.getFileNameWithoutExtension(audioPath);
    const directory = this.getDirectoryPath(audioPath);
    
    // 尝试常见的歌词文件扩展名
    const lyricsExtensions = ['lrc', 'ass', 'ssa', 'srt'];
    
    for (const ext of lyricsExtensions) {
      const lyricsPath = `${directory}/${baseName}.${ext}`;
      try {
        // 先检查文件是否存在
        const exists = await this.fileExists(lyricsPath);
        if (exists) {
          return lyricsPath;
        }
      } catch (error) {
        // 文件不存在，继续尝试下一个扩展名
      }
    }
    
    return null;
  }

  /**
   * 格式化文件大小
   * @param {number} bytes - 字节数
   * @returns {string} 格式化后的文件大小
   */
  static formatFileSize(bytes) {
    if (bytes === 0) return '0 Bytes';
    
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  }

  /**
   * 格式化时间（秒转换为 mm:ss 或 hh:mm:ss）
   * @param {number} seconds - 秒数
   * @returns {string} 格式化后的时间
   */
  static formatTime(seconds) {
    if (isNaN(seconds) || !isFinite(seconds)) return '0:00';
    
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const remainingSeconds = Math.floor(seconds % 60);
    
    if (hours > 0) {
      return `${hours}:${minutes.toString().padStart(2, '0')}:${remainingSeconds.toString().padStart(2, '0')}`;
    } else {
      return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
    }
  }
  // 这些方法已经在文件前面定义过，不需要重复定义
  // 移除重复定义以提高性能
}

export default FileUtils;