/**
 * 播放统计插件
 * 记录每首歌曲的播放次数和播放历史
 */

import { PluginPermission, type PluginAPI, type BuiltinPluginDefinition, type Track } from '../pluginManager'

interface PlayCountData {
  playCounts: Record<string, number>
  playHistory: HistoryEntry[]
  totalPlayTime: number
}

interface HistoryEntry {
  path: string
  title: string
  artist: string
  timestamp: number
}

interface PlayCountStats {
  totalTracks: number
  totalPlays: number
  totalPlayTime: number
  totalPlayTimeFormatted: string
}

function formatDuration(seconds: number): string {
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  
  if (hours > 0) {
    return `${hours}小时${minutes}分钟`
  }
  return `${minutes}分钟`
}

export const playCountPlugin: BuiltinPluginDefinition = {
  id: 'builtin-play-count',
  name: '播放统计',
  version: '1.1.0',
  author: 'Mercurial Player',
  description: '记录每首歌曲的播放次数和播放历史',
  permissions: [
    PluginPermission.PLAYER_READ,
    PluginPermission.STORAGE,
  ],

  main: (api: PluginAPI) => {
    let lastTrack: Track | null = null
    let playStartTime: number | null = null
    let accumulatedPlayTime: number = 0  // 累计播放时长
    let hasRecordedCurrentTrack = false
    let pollingInterval: ReturnType<typeof setInterval> | null = null
    
    // 保存事件回调引用以便正确清理
    let trackChangedCallback: (data: unknown) => void
    let stateChangedCallback: (data: unknown) => void

    const loadData = (): PlayCountData => {
      return {
        playCounts: api.storage.get<Record<string, number>>('playCounts', {}),
        playHistory: api.storage.get<HistoryEntry[]>('playHistory', []),
        totalPlayTime: api.storage.get<number>('totalPlayTime', 0),
      }
    }

    const saveData = (data: PlayCountData): void => {
      api.storage.set('playCounts', data.playCounts)
      api.storage.set('playHistory', data.playHistory)
      api.storage.set('totalPlayTime', data.totalPlayTime)
    }

    const recordPlayCount = (track: Track, playDuration: number): void => {
      if (!track || !track.path) return
      
      // 只统计播放时间超过30秒的音乐
      if (playDuration < 30) {
        api.log.debug(`播放时长不足30秒，不记录统计: ${track.title} (${Math.round(playDuration)}秒)`)
        return
      }

      const data = loadData()
      const trackPath = track.path

      data.playCounts[trackPath] = (data.playCounts[trackPath] || 0) + 1

      const historyEntry: HistoryEntry = {
        path: trackPath,
        title: (track.title as string) || '',
        artist: (track.artist as string) || '',
        timestamp: Date.now(),
      }
      data.playHistory.unshift(historyEntry)
      if (data.playHistory.length > 100) {
        data.playHistory = data.playHistory.slice(0, 100)
      }

      saveData(data)
      api.log.debug(`播放记录: ${track.title} - 第 ${data.playCounts[trackPath]} 次 (播放时长: ${Math.round(playDuration)}秒)`)
    }

    const addPlayTime = (seconds: number): void => {
      if (seconds <= 0) return
      
      const data = loadData()
      data.totalPlayTime += seconds
      saveData(data)
      api.log.debug(`累计播放时长: +${Math.round(seconds)}秒`)
    }

    const settleLastTrack = (track: Track | null, isPausing: boolean = false): void => {
      if (playStartTime) {
        const currentPlayDuration = (Date.now() - playStartTime) / 1000
        accumulatedPlayTime += currentPlayDuration
        playStartTime = null
      }
      
      // 如果是暂停，只累计播放时长
      if (isPausing) {
        return
      }
      
      // 切换歌曲或停止播放时，结算统计
      if (track && accumulatedPlayTime > 0) {
        // 累计播放时长
        addPlayTime(accumulatedPlayTime)
        // 只有播放时长超过30秒才记录播放次数
        recordPlayCount(track, accumulatedPlayTime)
        accumulatedPlayTime = 0
      } else if (accumulatedPlayTime > 0) {
        // 如果没有track信息，只累计播放时长
        addPlayTime(accumulatedPlayTime)
        accumulatedPlayTime = 0
      }
    }

    const handleTrackChange = (newTrack: Track | null, isPlaying: boolean): void => {
      const newPath = newTrack?.path || null
      const lastTrackPath = lastTrack?.path || null

      if (newPath !== lastTrackPath) {
        // 切换歌曲时，结算上一首的播放时长和统计
        settleLastTrack(lastTrack)
        lastTrack = newTrack
        accumulatedPlayTime = 0  // 重置累计播放时长
        hasRecordedCurrentTrack = false

        if (newTrack && isPlaying) {
          // 开始播放新歌曲，记录开始时间（但不立即记录播放次数）
          playStartTime = Date.now()
        }
      } else {
        if (isPlaying && newTrack) {
          // 继续播放当前歌曲
          if (!playStartTime) {
            playStartTime = Date.now()
          }
          // 更新当前歌曲信息
          lastTrack = newTrack
        } else {
          // 暂停播放，只累计播放时长
          if (playStartTime) {
            const currentPlayDuration = (Date.now() - playStartTime) / 1000
            accumulatedPlayTime += currentPlayDuration
            playStartTime = null
          }
        }
      }
    }

    const pollPlayerState = async (): Promise<void> => {
      try {
        const state = api.player.getState()
        handleTrackChange(state.currentTrack, state.isPlaying)
      } catch {
        // 忽略错误
      }
    }

    return {
      async activate(): Promise<void> {
        api.log.info('播放统计插件已激活')

        // 定义回调函数
        trackChangedCallback = (data) => {
          const { track, isPlaying } = data as { track: Track | null; isPlaying: boolean }
          handleTrackChange(track, isPlaying)
        }
        
        stateChangedCallback = (data) => {
          const { track, isPlaying } = data as { track: Track | null; isPlaying: boolean }
          handleTrackChange(track, isPlaying)
        }

        // 注册事件监听器
        api.events.on('player:trackChanged', trackChangedCallback)
        api.events.on('player:stateChanged', stateChangedCallback)

        await pollPlayerState()
        pollingInterval = setInterval(pollPlayerState, 5000)
      },

      deactivate(): void {
        // 停用时结算最后一首歌曲的播放时长和统计
        settleLastTrack(lastTrack)
        
        // 正确清理事件监听器
        if (trackChangedCallback) {
          api.events.off('player:trackChanged', trackChangedCallback)
        }
        if (stateChangedCallback) {
          api.events.off('player:stateChanged', stateChangedCallback)
        }
        
        if (pollingInterval) {
          clearInterval(pollingInterval)
          pollingInterval = null
        }
        
        lastTrack = null
        playStartTime = null
        accumulatedPlayTime = 0
        hasRecordedCurrentTrack = false
        api.log.info('播放统计插件已停用')
      },

      getPlayCount(trackPath: string): number {
        const data = loadData()
        return data.playCounts[trackPath] || 0
      },

      getAllPlayCounts(): Record<string, number> {
        return loadData().playCounts
      },

      getMostPlayed(limit = 10): { path: string; count: number }[] {
        const data = loadData()
        return Object.entries(data.playCounts)
          .sort((a, b) => b[1] - a[1])
          .slice(0, limit)
          .map(([path, count]) => ({ path, count }))
      },

      getPlayHistory(limit = 50): HistoryEntry[] {
        const data = loadData()
        return data.playHistory.slice(0, limit)
      },

      getTotalPlayTime(): number {
        let total = loadData().totalPlayTime
        if (playStartTime) {
          total += accumulatedPlayTime + (Date.now() - playStartTime) / 1000
        } else {
          total += accumulatedPlayTime
        }
        return total
      },

      getStats(): PlayCountStats {
        const data = loadData()
        const totalTracks = Object.keys(data.playCounts).length
        const totalPlays = Object.values(data.playCounts).reduce((a, b) => a + b, 0)
        
        let totalPlayTime = data.totalPlayTime
        if (playStartTime) {
          totalPlayTime += accumulatedPlayTime + (Date.now() - playStartTime) / 1000
        } else {
          totalPlayTime += accumulatedPlayTime
        }
        
        return {
          totalTracks,
          totalPlays,
          totalPlayTime,
          totalPlayTimeFormatted: formatDuration(totalPlayTime),
        }
      },

      clearAllData(): void {
        api.storage.set('playCounts', {})
        api.storage.set('playHistory', [])
        api.storage.set('totalPlayTime', 0)
        playStartTime = null
        accumulatedPlayTime = 0
        hasRecordedCurrentTrack = false
        api.log.info('播放统计数据已清除')
      },
    }
  },
}
