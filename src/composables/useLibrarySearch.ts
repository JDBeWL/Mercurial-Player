/**
 * 曲库搜索 composable
 *
 * 从 MusicLibrary.vue 抽离:搜索防抖(300ms)、跨播放列表去重、按配置排序、
 * 搜索结果封面分批加载(每批 5 首并行,generation 计数器取消上一次加载)。
 */
import { ref, shallowRef, type Ref } from 'vue'
import type { Track, Playlist } from '@/types'
import type { usePlayerStore } from '@/stores/player'
import type { useConfigStore } from '@/stores/config'
import { getTrackCoverPath } from '@/services/mediaService'

/** 搜索结果类型(在 Track 基础上扩展文件夹信息) */
export interface SearchResult extends Track {
  folderPath?: string
  folderName?: string
}

export function useLibrarySearch(
  playlists: Ref<Playlist[]>,
  configStore: ReturnType<typeof useConfigStore>,
  playerStore: ReturnType<typeof usePlayerStore>,
): {
  searchTerm: Ref<string>
  searchResults: Ref<SearchResult[]>
  searchCovers: Ref<Map<string, string>>
  handleSearch: () => Promise<void>
  clearSearch: () => void
} {
  const searchTerm = ref<string>('')
  const searchResults = ref<SearchResult[]>([])

  // ===== 封面加载去响应式化 =====
  // 搜索结果封面存放在本地 shallowRef Map，模板经 coverFor(file) 读取。
  // 渲染只依赖 Map 本身（非逐条 coverPath），封面加载每批替换一次 Map 触发一次
  // 渲染，v-memo 只重渲染封面真正出现/变化的项目，消除逐条 mutation 的 O(N²) patch。
  // 注意：coverFor 绝不能读 file.coverPath（那是响应式属性，会重建逐条依赖）；
  // 搜索完成时把已有 coverPath 播种进 Map。
  const searchCovers = shallowRef<Map<string, string>>(new Map())

  // 搜索功能 - 添加防抖
  let searchTimeout: ReturnType<typeof setTimeout> | null = null
  // 封面加载的 generation 计数器，用于在重新搜索时取消上一次加载
  let coverLoadGeneration = 0

  const handleSearch = async (): Promise<void> => {
    // 清除之前的定时器
    if (searchTimeout) {
      clearTimeout(searchTimeout)
    }

    if (!searchTerm.value.trim()) {
      searchResults.value = []
      coverLoadGeneration++ // 取消正在进行的封面加载
      return
    }

    // 防抖：300ms 后执行搜索
    searchTimeout = setTimeout(() => {
      const lowerCaseSearchTerm = searchTerm.value.toLowerCase()
      const uniqueResults = new Map<string, Track>() // 使用Map来去重

      for (const playlist of playlists.value) {
        if (playlist.files) {
          const results = playlist.files.filter(
            (file) =>
              (file.title && file.title.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.artist && file.artist.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.album && file.album.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.name && file.name.toLowerCase().includes(lowerCaseSearchTerm)),
          )

          // 去重
          for (const file of results) {
            if (!uniqueResults.has(file.path)) {
              uniqueResults.set(file.path, file)
            }
          }
        }
      }

      // 根据配置排序
      const isAscOrder = configStore.playlist.sortOrder === 'asc'
      searchResults.value = Array.from(uniqueResults.values()).sort((a, b) => {
        const titleA = (a.title || a.name || '').toLowerCase()
        const titleB = (b.title || b.name || '').toLowerCase()

        if (isAscOrder) {
          // A-Z order
          if (titleA < titleB) return -1
          if (titleA > titleB) return 1
        } else {
          // Z-A order
          if (titleA > titleB) return -1
          if (titleA < titleB) return 1
        }

        return 0
      })

      // 异步加载搜索结果的封面（从缓存恢复的 track 无 coverPath）。
      // 先把已有 coverPath 播种进本地 Map，加载循环不再依赖逐条 mutation 传播到 UI
      searchCovers.value = new Map(
        searchResults.value.filter((f) => f.coverPath).map((f) => [f.path, f.coverPath as string]),
      )
      coverLoadGeneration++
      loadSearchResultCovers(coverLoadGeneration)
    }, 300)
  }

  // 批量加载搜索结果封面，每批 5 首并行；每批完成后把新封面合并进本地 Map
  // （一次性替换触发一次渲染，v-memo 只重渲染封面变化的项目），
  // 同时通过 store 的版本号机制通知播放队列视图
  const loadSearchResultCovers = async (gen: number): Promise<void> => {
    const files = searchResults.value
    const BATCH = 5
    for (let i = 0; i < files.length; i += BATCH) {
      if (gen !== coverLoadGeneration) return
      const batch = files.slice(i, i + BATCH)
      const found: Array<[string, string]> = []
      await Promise.all(
        batch.map(async (file) => {
          if (gen !== coverLoadGeneration) return
          if (!file.coverPath) {
            try {
              const coverPath = await getTrackCoverPath(file.path)
              if (gen !== coverLoadGeneration) return
              if (coverPath) {
                // 写回原对象供播放等逻辑直接读取 coverPath（本组件渲染不依赖它）
                file.coverPath = coverPath
                found.push([file.path, coverPath])
              }
            } catch {
              // 忽略单首加载失败
            }
          }
        }),
      )
      if (found.length > 0) {
        const next = new Map(searchCovers.value)
        for (const [path, coverPath] of found) {
          next.set(path, coverPath)
          playerStore.recordCoverUpdate(path, coverPath)
        }
        searchCovers.value = next
      }
    }
  }

  const clearSearch = (): void => {
    searchTerm.value = ''
    searchResults.value = []
    searchCovers.value = new Map()
    coverLoadGeneration++ // 取消正在进行的封面加载
  }

  return { searchTerm, searchResults, searchCovers, handleSearch, clearSearch }
}
