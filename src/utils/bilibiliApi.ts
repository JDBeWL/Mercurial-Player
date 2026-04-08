/**
 * 哔哩哔哩搜索 API
 * 通过 Tauri 后端代理请求，避免 CORS 问题
 */

import { invoke } from '@tauri-apps/api/core'
import logger from './logger'
import errorHandler, { ErrorType, ErrorSeverity, handlePromise } from './errorHandler'

interface BilibiliVideo {
  bvid: string
  title: string
  author: string
  duration: string
  play_count: number
  url: string
}

/**
 * 哔哩哔哩 API 类
 */
export class BilibiliAPI {
  /**
   * 搜索视频
   */
  async searchVideos(keyword: string, limit: number = 10, page: number = 1): Promise<BilibiliVideo[]> {
    const result = await handlePromise(
      invoke<BilibiliVideo[]>('bilibili_search_videos', {
        keyword,
        page,
        limit
      }),
      {
        type: ErrorType.NETWORK,
        severity: ErrorSeverity.MEDIUM,
        context: { keyword, page, limit, action: 'searchVideos' },
        showToUser: false,
        throw: false
      }
    )

    return result.success ? result.data! : []
  }
}

export const bilibiliApi = new BilibiliAPI()
export default bilibiliApi
