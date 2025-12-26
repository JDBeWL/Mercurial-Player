/**
 * 播放统计插件
 * 记录每首歌曲的播放次数和播放历史
 */

import { PluginPermission } from '../pluginManager'

export const playCountPlugin = {
  id: 'builtin-play-count',
  name: '播放统计',
  version: '1.0.0',
  author: 'Mercurial Player',
  description: '记录每首歌曲的播放次数和播放历史',
  permissions: [
    PluginPermission.PLAYER_READ,
    PluginPermission.STORAGE,
  ],

  main: (api) => {
    let lastTrackPath = null
    let playStartTime = null
    let hasRecordedCurrentTrack = false
    let pollingInterval = null

    // 加载或初始化存储
    const loadData = () => {
      return {
        playCounts: api.storage.get('playCounts', {}),
        playHistory: api.storage.get('playHistory', []),
        totalPlayTime: api.storage.get('totalPlayTime', 0),
      }
    }

    // 保存数据
    const saveData = (data) => {
      api.storage.set('playCounts', data.playCounts)
      api.storage.set('playHistory', data.playHistory)
      api.storage.set('totalPlayTime', data.totalPlayTime)
    }

    // 记录播放次数（切歌时调用）
    const recordPlayCount = (track) => {
      if (!track || !track.path) return

      const data = loadData()
      const trackPath = track.path

      // 增加播放次数
      data.playCounts[trackPath] = (data.playCounts[trackPath] || 0) + 1

      // 添加到播放历史（保留最近100条）
      const historyEntry = {
        path: trackPath,
        title: track.title || track.name || '',
        artist: track.artist || '',
        timestamp: Date.now(),
      }
      data.playHistory.unshift(historyEntry)
      if (data.playHistory.length > 100) {
        data.playHistory = data.playHistory.slice(0, 100)
      }

      saveData(data)
      api.log.debug(`播放记录: ${track.title} - 第 ${data.playCounts[trackPath]} 次`)
    }

    // 累计播放时长
    const addPlayTime = (seconds) => {
      if (seconds <= 0) return
      
      const data = loadData()
      data.totalPlayTime += seconds
      saveData(data)
      api.log.debug(`累计播放时长: +${Math.round(seconds)}秒`)
    }

    // 结算上一首歌的播放时长
    const settleLastTrack = () => {
      if (playStartTime) {
        const playDuration = (Date.now() - playStartTime) / 1000
        addPlayTime(playDuration)
        playStartTime = null
      }
    }

    // 处理歌曲切换
    const handleTrackChange = (newTrack, isPlaying) => {
      const newPath = newTrack?.path || null

      // 歌曲切换了
      if (newPath !== lastTrackPath) {
        // 结算上一首歌的播放时长
        settleLastTrack()

        // 更新当前歌曲
        lastTrackPath = newPath
        hasRecordedCurrentTrack = false

        // 如果新歌曲正在播放，记录播放次数并开始计时
        if (newTrack && isPlaying) {
          recordPlayCount(newTrack)
          hasRecordedCurrentTrack = true
          playStartTime = Date.now()
        }
      } else {
        // 同一首歌，但播放状态变化
        if (isPlaying && newTrack) {
          // 开始播放
          if (!hasRecordedCurrentTrack) {
            recordPlayCount(newTrack)
            hasRecordedCurrentTrack = true
          }
          if (!playStartTime) {
            playStartTime = Date.now()
          }
        } else {
          // 暂停播放，结算时长
          settleLastTrack()
        }
      }
    }

    // 通过 API 轮询播放状态
    const pollPlayerState = async () => {
      try {
        const state = await api.player.getState()
        handleTrackChange(state.currentTrack, state.isPlaying)
      } catch (e) {
        // 忽略错误，下次轮询重试
      }
    }

    return {
      async activate() {
        api.log.info('播放统计插件已激活')

        // 通过事件 API 监听播放状态变化
        api.events.on('player:trackChanged', (data) => {
          handleTrackChange(data.track, data.isPlaying)
        })
        
        api.events.on('player:stateChanged', (data) => {
          handleTrackChange(data.track, data.isPlaying)
        })

        // 初始获取一次状态
        await pollPlayerState()
        
        // 备用：定期轮询（每 5 秒），以防事件丢失
        pollingInterval = setInterval(pollPlayerState, 5000)
      },

      deactivate() {
        // 结算当前播放时长
        settleLastTrack()
        
        // 取消事件监听
        api.events.off('player:trackChanged')
        api.events.off('player:stateChanged')
        
        if (pollingInterval) {
          clearInterval(pollingInterval)
          pollingInterval = null
        }
        
        lastTrackPath = null
        playStartTime = null
        hasRecordedCurrentTrack = false
        api.log.info('播放统计插件已停用')
      },

      // 获取播放次数
      getPlayCount(trackPath) {
        const data = loadData()
        return data.playCounts[trackPath] || 0
      },

      // 获取所有播放次数
      getAllPlayCounts() {
        return loadData().playCounts
      },

      // 获取最常播放的歌曲
      getMostPlayed(limit = 10) {
        const data = loadData()
        return Object.entries(data.playCounts)
          .sort((a, b) => b[1] - a[1])
          .slice(0, limit)
          .map(([path, count]) => ({ path, count }))
      },

      // 获取播放历史
      getPlayHistory(limit = 50) {
        const data = loadData()
        return data.playHistory.slice(0, limit)
      },

      // 获取总播放时间（秒）
      getTotalPlayTime() {
        // 包含当前正在播放的时长
        let total = loadData().totalPlayTime
        if (playStartTime) {
          total += (Date.now() - playStartTime) / 1000
        }
        return total
      },

      // 获取统计信息
      getStats() {
        const data = loadData()
        const totalTracks = Object.keys(data.playCounts).length
        const totalPlays = Object.values(data.playCounts).reduce((a, b) => a + b, 0)
        
        // 包含当前正在播放的时长
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

      // 清除所有数据
      clearAllData() {
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

// 格式化时长
function formatDuration(seconds) {
  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  
  if (hours > 0) {
    return `${hours}小时${minutes}分钟`
  }
  return `${minutes}分钟`
}
