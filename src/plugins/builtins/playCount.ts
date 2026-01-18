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
  version: '1.0.0',
  author: 'Mercurial Player',
  description: '记录每首歌曲的播放次数和播放历史',
  permissions: [
    PluginPermission.PLAYER_READ,
    PluginPermission.STORAGE,
  ],

  main: (api: PluginAPI) => {
    let lastTrackPath: string | null = null
    let playStartTime: number | null = null
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

    const recordPlayCount = (track: Track): void => {
      if (!track || !track.path) return

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
      api.log.debug(`播放记录: ${track.title} - 第 ${data.playCounts[trackPath]} 次`)
    }

    const addPlayTime = (seconds: number): void => {
      if (seconds <= 0) return
      
      const data = loadData()
      data.totalPlayTime += seconds
      saveData(data)
      api.log.debug(`累计播放时长: +${Math.round(seconds)}秒`)
    }

    const settleLastTrack = (): void => {
      if (playStartTime) {
        const playDuration = (Date.now() - playStartTime) / 1000
        addPlayTime(playDuration)
        playStartTime = null
      }
    }

    const handleTrackChange = (newTrack: Track | null, isPlaying: boolean): void => {
      const newPath = newTrack?.path || null

      if (newPath !== lastTrackPath) {
        settleLastTrack()
        lastTrackPath = newPath
        hasRecordedCurrentTrack = false

        if (newTrack && isPlaying) {
          recordPlayCount(newTrack)
          hasRecordedCurrentTrack = true
          playStartTime = Date.now()
        }
      } else {
        if (isPlaying && newTrack) {
          if (!hasRecordedCurrentTrack) {
            recordPlayCount(newTrack)
            hasRecordedCurrentTrack = true
          }
          if (!playStartTime) {
            playStartTime = Date.now()
          }
        } else {
          settleLastTrack()
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
        settleLastTrack()
        
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
        
        lastTrackPath = null
        playStartTime = null
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
          total += (Date.now() - playStartTime) / 1000
        }
        return total
      },

      getStats(): PlayCountStats {
        const data = loadData()
        const totalTracks = Object.keys(data.playCounts).length
        const totalPlays = Object.values(data.playCounts).reduce((a, b) => a + b, 0)
        
        let totalPlayTime = data.totalPlayTime
        if (playStartTime) {
          totalPlayTime += (Date.now() - playStartTime) / 1000
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
        hasRecordedCurrentTrack = false
        api.log.info('播放统计数据已清除')
      },
    }
  },
}
