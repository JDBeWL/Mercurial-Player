<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.playStats') || '播放统计' }}</h3>
      <button class="text-button danger" @click="clearStats" v-if="playStats?.totalPlays > 0">
        <span class="material-symbols-rounded">delete</span>
        清除数据
      </button>
    </div>

    <div v-if="!playStats || playStats.totalPlays === 0" class="empty-state">
      <span class="material-symbols-rounded">bar_chart</span>
      <p>暂无播放记录</p>
      <p class="hint">播放音乐后这里会显示统计数据</p>
    </div>

    <template v-else>
      <!-- 统计概览 -->
      <div class="stats-overview">
        <div class="stat-card">
          <span class="material-symbols-rounded">play_circle</span>
          <div class="stat-content">
            <span class="stat-value">{{ playStats.totalPlays }}</span>
            <span class="stat-label">总播放次数</span>
          </div>
        </div>
        <div class="stat-card">
          <span class="material-symbols-rounded">library_music</span>
          <div class="stat-content">
            <span class="stat-value">{{ playStats.totalTracks }}</span>
            <span class="stat-label">播放过的歌曲</span>
          </div>
        </div>
        <div class="stat-card">
          <span class="material-symbols-rounded">schedule</span>
          <div class="stat-content">
            <span class="stat-value">{{ playStats.totalPlayTimeFormatted }}</span>
            <span class="stat-label">总播放时长</span>
          </div>
        </div>
      </div>

      <!-- 最常播放 -->
      <div class="section" v-if="mostPlayed.length > 0">
        <h4>
          <span class="material-symbols-rounded">trending_up</span>
          最常播放
        </h4>
        <div class="track-list">
          <div v-for="(item, index) in mostPlayed" :key="item.path" class="track-item">
            <span class="rank" :class="{ 'top-3': index < 3 }">{{ index + 1 }}</span>
            <div class="track-info">
              <span class="track-title">{{ item.title || '未知曲目' }}</span>
              <span class="track-artist" v-if="item.artist">{{ item.artist }}</span>
            </div>
            <span class="play-count">{{ item.count }} 次</span>
          </div>
        </div>
      </div>

      <!-- 最近播放 -->
      <div class="section" v-if="recentPlayed.length > 0">
        <h4>
          <span class="material-symbols-rounded">history</span>
          最近播放
        </h4>
        <div class="track-list">
          <div v-for="item in recentPlayed" :key="item.timestamp" class="track-item">
            <span class="material-symbols-rounded track-icon">music_note</span>
            <div class="track-info">
              <span class="track-title">{{ item.title || '未知曲目' }}</span>
              <span class="track-artist" v-if="item.artist">{{ item.artist }}</span>
            </div>
            <span class="play-time">{{ formatTime(item.timestamp) }}</span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup>
import { ref, onMounted, onUnmounted } from 'vue'
import { pluginManager } from '../../plugins'
import { useErrorNotification } from '../../composables/useErrorNotification'

const { showError } = useErrorNotification()

const playStats = ref(null)
const mostPlayed = ref([])
const recentPlayed = ref([])
let refreshInterval = null

// 刷新统计数据
const refreshStats = async () => {
  const playCountInstance = pluginManager.instances.get('builtin-play-count')
  if (playCountInstance?.instance) {
    const instance = playCountInstance.instance
    playStats.value = instance.getStats()
    
    // 获取最常播放并补充曲目信息
    const mostPlayedRaw = instance.getMostPlayed(10)
    mostPlayed.value = await enrichTrackInfo(mostPlayedRaw)
    
    // 获取最近播放
    recentPlayed.value = instance.getPlayHistory(20)
  }
}

// 补充曲目信息（从 player store 获取）
const enrichTrackInfo = async (tracks) => {
  try {
    const { usePlayerStore } = await import('../../stores/player')
    const playerStore = usePlayerStore()
    
    return tracks.map(item => {
      // 尝试从播放列表中找到对应的曲目信息
      const track = playerStore.playlist.find(t => t.path === item.path)
      if (track) {
        return {
          ...item,
          title: track.title || track.displayTitle || track.name,
          artist: track.artist || track.displayArtist || '',
        }
      }
      // 如果找不到，从路径提取文件名
      return {
        ...item,
        title: extractFileName(item.path),
        artist: '',
      }
    })
  } catch {
    return tracks.map(item => ({
      ...item,
      title: extractFileName(item.path),
      artist: '',
    }))
  }
}

// 从路径提取文件名
const extractFileName = (path) => {
  if (!path) return '未知'
  const parts = path.replace(/\\/g, '/').split('/')
  const filename = parts[parts.length - 1]
  return filename.replace(/\.[^/.]+$/, '')
}

// 格式化时间
const formatTime = (timestamp) => {
  const date = new Date(timestamp)
  const now = new Date()
  const diff = now - date
  
  // 今天
  if (diff < 24 * 60 * 60 * 1000 && date.getDate() === now.getDate()) {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }
  // 昨天
  if (diff < 48 * 60 * 60 * 1000) {
    return '昨天 ' + date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }
  // 更早
  return date.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
}

// 清除统计数据
const clearStats = () => {
  const playCountInstance = pluginManager.instances.get('builtin-play-count')
  if (playCountInstance?.instance) {
    playCountInstance.instance.clearAllData()
    refreshStats()
    showError('播放统计已清除', 'info')
  }
}

onMounted(async () => {
  // 内置插件已在 main.js 中加载
  setTimeout(refreshStats, 100)
  refreshInterval = setInterval(refreshStats, 5000)
})

onUnmounted(() => {
  if (refreshInterval) {
    clearInterval(refreshInterval)
  }
})
</script>

<style scoped>
.tab-content {
  max-width: 800px;
}

.content-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

/* 统计概览 */
.stats-overview {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  margin-bottom: 32px;
}

.stat-card {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 20px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 16px;
}

.stat-card > .material-symbols-rounded {
  font-size: 32px;
  color: var(--md-sys-color-primary);
}

.stat-content {
  display: flex;
  flex-direction: column;
}

.stat-value {
  font-size: 24px;
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

.stat-label {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

/* 区块 */
.section {
  margin-bottom: 32px;
}

.section h4 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 16px 0;
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.section h4 .material-symbols-rounded {
  font-size: 20px;
  color: var(--md-sys-color-primary);
}

/* 曲目列表 */
.track-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.track-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
  transition: background-color 0.2s;
}

.track-item:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.rank {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-on-surface-variant);
  border-radius: 8px;
  font-size: 14px;
  font-weight: 600;
  flex-shrink: 0;
}

.rank.top-3 {
  background-color: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
}

.track-icon {
  font-size: 24px;
  color: var(--md-sys-color-on-surface-variant);
  flex-shrink: 0;
}

.track-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.track-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.track-artist {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.play-count, .play-time {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  flex-shrink: 0;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 64px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

.empty-state .material-symbols-rounded {
  font-size: 64px;
  margin-bottom: 16px;
  opacity: 0.5;
}

.empty-state p {
  margin: 0;
  font-size: 16px;
}

.empty-state .hint {
  margin-top: 8px;
  font-size: 14px;
  opacity: 0.7;
}

/* 按钮 */
.text-button {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 16px;
  background: none;
  color: var(--md-sys-color-primary);
  border: none;
  border-radius: 20px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s ease;
}

.text-button:hover {
  background-color: var(--md-sys-color-primary-container);
}

.text-button.danger {
  color: var(--md-sys-color-error);
}

.text-button.danger:hover {
  background-color: var(--md-sys-color-error-container);
}

@media (max-width: 600px) {
  .stats-overview {
    grid-template-columns: 1fr;
  }
}
</style>
