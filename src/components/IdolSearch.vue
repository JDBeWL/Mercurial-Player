<template>
  <div class="idol-search" :class="{ 'slide-out': isClosing }">
    <div class="idol-search-header">
      <h2 class="library-title">{{ $t('idolSearch.title') }}</h2>
      <div class="header-actions">
        <button class="icon-button" @click="handleClose">
          <span class="material-symbols-rounded">close</span>
        </button>
      </div>
    </div>

    <div class="idol-search-content">
      <div class="idol-group-tabs">
        <button
          v-for="group in idolGroups"
          :key="group.id"
          class="tab-button"
          :class="{ active: selectedGroup === group.id }"
          @click="selectedGroup = group.id"
        >
          <span class="material-symbols-rounded">{{ group.icon }}</span>
          {{ group.name }}
        </button>
      </div>

      <div class="search-section">
        <div class="search-input-wrapper" :class="{ 'is-searching': isSearching }">
          <span class="material-symbols-rounded search-icon">search</span>
          <input
            type="text"
            v-model="searchQuery"
            :placeholder="$t('idolSearch.searchPlaceholder')"
            @keyup.enter="handleSearch"
            :disabled="isSearching"
          />
          <!-- 圆形进度指示器 -->
          <div v-if="isSearching" class="circular-progress">
            <svg viewBox="0 0 24 24" class="progress-ring">
              <circle
                cx="12"
                cy="12"
                r="10"
                fill="none"
                stroke-width="2"
                class="progress-track"
              />
              <circle
                cx="12"
                cy="12"
                r="10"
                fill="none"
                stroke-width="2"
                class="progress-value"
              />
            </svg>
          </div>
        </div>

        <div class="platform-buttons">
          <button
            v-for="platform in platforms"
            :key="platform.id"
            class="platform-button"
            :class="{ active: selectedPlatform === platform.id }"
            @click="selectedPlatform = platform.id"
          >
            <span class="material-symbols-rounded">{{ platform.icon }}</span>
            {{ platform.name }}
          </button>
        </div>
      </div>

      <div class="search-results" v-if="searchResults.length > 0">
        <div class="results-header">
          <h3 class="section-title">{{ $t('idolSearch.results') }} ({{ totalCount }})</h3>
        </div>
        <div class="list">
          <div
            v-for="(result, index) in searchResults"
            :key="`result-${index}`"
            class="list-item"
            @click="openExternalLink(result.url)"
          >
            <div class="list-item-leading">
              <span class="material-symbols-rounded">{{ selectedPlatform === 'bilibili' ? 'video_library' : 'music_note' }}</span>
            </div>
            <div class="list-item-content">
              <div class="list-item-headline" :title="result.title">{{ result.title }}</div>
              <div class="list-item-supporting">
                <span>{{ result.artist || result.author }}</span>
                <span v-if="result.duration || result.playCount" class="result-meta">
                  <span v-if="result.duration">{{ formatDuration(result.duration) }}</span>
                  <span v-if="result.playCount"> • {{ formatPlayCount(result.playCount) }}播放</span>
                </span>
              </div>
            </div>
            <div class="list-item-trailing">
              <span class="material-symbols-rounded">open_in_new</span>
            </div>
          </div>
        </div>

        <!-- 分页控制 -->
        <div class="pagination" v-if="totalPages > 1">
          <button
            class="page-btn"
            :disabled="currentPage <= 1 || isSearching"
            @click="goToPage(currentPage - 1)"
          >
            <span class="material-symbols-rounded">chevron_left</span>
          </button>
          <div class="page-info">
            {{ currentPage }} / {{ totalPages }}
          </div>
          <button
            class="page-btn"
            :disabled="currentPage >= totalPages || isSearching"
            @click="goToPage(currentPage + 1)"
          >
            <span class="material-symbols-rounded">chevron_right</span>
          </button>
        </div>
      </div>

      <div class="empty-state" v-else-if="!isSearching && hasSearched">
        <span class="material-symbols-rounded">search_off</span>
        <h3>{{ $t('idolSearch.noResults') }}</h3>
        <p>{{ $t('idolSearch.tryDifferent') }}</p>
      </div>

      <div class="welcome-state" v-else-if="!hasSearched">
        <span class="material-symbols-rounded">stars</span>
        <h3>{{ $t('idolSearch.welcome') }}</h3>
        <p>{{ $t('idolSearch.welcomeDesc') }}</p>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { openUrl } from '@tauri-apps/plugin-opener'
import bilibiliApi from '../utils/bilibiliApi'
import neteaseApi from '../utils/neteaseApi'

const emit = defineEmits(['close'])

const { t } = useI18n()

const isClosing = ref(false)
const searchQuery = ref('')
const selectedGroup = ref('lovelive')
const selectedPlatform = ref('bilibili')
const searchResults = ref([])
const isSearching = ref(false)
const hasSearched = ref(false)
const currentPage = ref(1)
const totalCount = ref(0)
const pageSize = 10

const totalPages = computed(() => {
  return Math.ceil(totalCount.value / pageSize)
})

const idolGroups = [
  { id: 'lovelive', name: 'LoveLive', icon: 'favorite' },
  { id: 'bangdream', name: 'BanG Dream', icon: 'music_note' }
]

const platforms = [
  { id: 'bilibili', name: '哔哩哔哩', icon: 'video_library' },
  { id: 'netease', name: '网易云音乐', icon: 'headphones' }
]

const handleSearch = async (resetPage = true) => {
  if (!searchQuery.value.trim() || isSearching.value) return

  if (resetPage) {
    currentPage.value = 1
  }

  isSearching.value = true
  hasSearched.value = true
  searchResults.value = []

  try {
    const query = searchQuery.value.trim()
    const groupPrefix = selectedGroup.value === 'lovelive' ? 'Lovelive' : 'BanG Dream'
    const fullQuery = `${groupPrefix} ${query}`

    if (selectedPlatform.value === 'bilibili') {
      const videos = await bilibiliApi.searchVideos(fullQuery, pageSize, currentPage.value)
      searchResults.value = videos.map(video => ({
        title: video.title,
        artist: video.author,
        author: video.author,
        duration: video.duration,
        playCount: video.play_count,
        url: video.url
      }))
      totalCount.value = videos.length >= pageSize ? currentPage.value * pageSize + 1 : (currentPage.value - 1) * pageSize + videos.length
    } else if (selectedPlatform.value === 'netease') {
      const songs = await neteaseApi.searchSongs(fullQuery, pageSize, (currentPage.value - 1) * pageSize)
      searchResults.value = songs.map(song => ({
        title: song.name,
        artist: song.artist,
        author: song.artist,
        duration: Math.floor(song.duration / 1000),
        playCount: null,
        url: `https://music.163.com/#/song?id=${song.id}`
      }))
      totalCount.value = songs.length >= pageSize ? currentPage.value * pageSize + 1 : (currentPage.value - 1) * pageSize + songs.length
    }
  } catch (error) {
    console.error('Search error:', error)
  } finally {
    isSearching.value = false
  }
}

const goToPage = (page) => {
  if (page < 1 || page > totalPages.value || isSearching.value) return
  currentPage.value = page
  handleSearch(false)
}

const openExternalLink = async (url) => {
  try {
    await openUrl(url)
  } catch (error) {
    console.error('Failed to open link:', error)
  }
}

const formatDuration = (duration) => {
  if (!duration) return ''
  
  if (typeof duration === 'string' && duration.includes(':')) {
    return duration
  }
  
  if (typeof duration === 'number') {
    const minutes = Math.floor(duration / 60)
    const seconds = duration % 60
    return `${minutes}:${seconds.toString().padStart(2, '0')}`
  }
  
  return String(duration)
}

const formatPlayCount = (count) => {
  if (!count) return ''
  
  if (count >= 10000) {
    return `${(count / 10000).toFixed(1)}万`
  }
  
  return count.toString()
}

const handleClose = () => {
  isClosing.value = true
  setTimeout(() => {
    emit('close')
  }, 200)
}
</script>

<style scoped>
.idol-search {
  position: fixed;
  top: 0;
  left: 0;
  width: 400px;
  height: 100vh;
  background: var(--md-sys-color-surface);
  box-shadow: var(--shadow-strong);
  z-index: 100;
  display: flex;
  flex-direction: column;
  transform: translateX(0);
  transition: transform 0.2s var(--ease-out-expo);
}

.idol-search.slide-out {
  transform: translateX(-100%);
}

.idol-search-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.library-title {
  font-size: 20px;
  font-weight: 500;
  margin: 0;
  color: var(--md-sys-color-on-surface);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.idol-search-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.idol-group-tabs {
  display: flex;
  gap: 8px;
  margin-bottom: 16px;
}

.tab-button {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 16px;
  border: none;
  border-radius: var(--md-sys-shape-corner-large);
  background: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all var(--transition-normal) var(--ease-out-expo);
}

.tab-button:hover {
  background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, var(--md-sys-color-surface-variant));
}

.tab-button.active {
  background: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.tab-button .material-symbols-rounded {
  font-size: 20px;
}

.search-section {
  margin-bottom: 20px;
}

.search-input-wrapper {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--md-sys-color-surface-variant);
  border-radius: var(--md-sys-shape-corner-large);
  margin-bottom: 12px;
  position: relative;
}

.search-input-wrapper.is-searching .search-icon {
  opacity: 0.4;
}

.search-icon {
  font-size: 20px;
  color: var(--md-sys-color-on-surface-variant);
  transition: opacity var(--transition-fast);
}

.search-input-wrapper input {
  flex: 1;
  border: none;
  background: transparent;
  font-size: 14px;
  color: var(--md-sys-color-on-surface);
  outline: none;
}

.search-input-wrapper input::placeholder {
  color: var(--md-sys-color-on-surface-variant);
}

.search-input-wrapper input:disabled {
  cursor: not-allowed;
}

/* 圆形进度指示器 */
.circular-progress {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
}

.progress-ring {
  width: 100%;
  height: 100%;
  animation: rotate 1.5s linear infinite;
}

.progress-track {
  stroke: var(--md-sys-color-outline-variant);
  opacity: 0.3;
}

.progress-value {
  stroke: var(--md-sys-color-primary);
  stroke-linecap: round;
  stroke-dasharray: 62.83;
  stroke-dashoffset: 47;
  transition: stroke-dashoffset 0.35s;
}

@keyframes rotate {
  to {
    transform: rotate(360deg);
  }
}

.platform-buttons {
  display: flex;
  gap: 8px;
}

.platform-button {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-medium);
  background: transparent;
  color: var(--md-sys-color-on-surface);
  font-size: 13px;
  cursor: pointer;
  transition: all var(--transition-normal) var(--ease-out-expo);
}

.platform-button:hover {
  background: var(--md-sys-color-surface-variant);
}

.platform-button.active {
  background: var(--md-sys-color-secondary-container);
  border-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.platform-button .material-symbols-rounded {
  font-size: 18px;
}

.search-results {
  margin-top: 16px;
}

.results-header {
  margin-bottom: 12px;
}

.section-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
  margin: 0;
}

.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.list-item {
  display: flex;
  align-items: center;
  padding: 12px;
  border-radius: var(--md-sys-shape-corner-medium);
  cursor: pointer;
  transition: background var(--transition-fast);
  position: relative;
}

.list-item::before {
  display: none;
}

.list-item:hover {
  background: var(--md-sys-color-surface-variant);
}

.list-item-leading {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--md-sys-color-secondary-container);
  border-radius: var(--md-sys-shape-corner-small);
  margin-right: 12px;
}

.list-item-leading .material-symbols-rounded {
  color: var(--md-sys-color-on-secondary-container);
  font-size: 20px;
}

.list-item-content {
  flex: 1;
  min-width: 0;
}

.list-item-headline {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.list-item-supporting {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  display: flex;
  align-items: center;
  gap: 4px;
}

.result-meta {
  opacity: 0.7;
}

.list-item-trailing {
  margin-left: 8px;
  color: var(--md-sys-color-on-surface-variant);
  transition: color var(--transition-fast);
}

.list-item:hover .list-item-trailing {
  color: var(--md-sys-color-primary);
}

/* 分页样式 */
.pagination {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  margin-top: 16px;
  padding-top: 12px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
}

.page-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  border-radius: var(--md-sys-shape-corner-small);
  background: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: all var(--transition-fast);
}

.page-btn:hover:not(:disabled) {
  background: var(--md-sys-color-primary);
  color: var(--md-sys-color-on-primary);
}

.page-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.page-btn .material-symbols-rounded {
  font-size: 18px;
}

.page-info {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
  min-width: 48px;
  text-align: center;
}

.empty-state,
.welcome-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 20px;
  text-align: center;
}

.empty-state .material-symbols-rounded,
.welcome-state .material-symbols-rounded {
  font-size: 64px;
  color: var(--md-sys-color-on-surface-variant);
  opacity: 0.5;
  margin-bottom: 16px;
}

.empty-state h3,
.welcome-state h3 {
  font-size: 18px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin: 0 0 8px 0;
}

.empty-state p,
.welcome-state p {
  font-size: 14px;
  color: var(--md-sys-color-on-surface-variant);
  margin: 0;
}
</style>
