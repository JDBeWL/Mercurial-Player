/** 曲库搜索: 300ms 防抖, 跨播放列表按 path 去重再按配置排序; 封面分批加载与 generation 取消机制见 searchCovers 声明处 */
import { ref, shallowRef, type Ref } from 'vue'
import type { Track, Playlist } from '@/types'
import type { usePlayerStore } from '@/stores/player'
import type { useConfigStore } from '@/stores/config'
import { getTrackCoverPath } from '@/services/mediaService'

/** 搜索结果类型 (在 Track 基础上扩展文件夹信息) */
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

  // 封面走本地 shallowRef Map: 渲染只依赖 Map 本身, 每批加载完整体替换一次只触发一次渲染,
  // 避免逐条响应式 mutation 造成的 O(N^2) patch (coverFor 读这个 Map, 绝不能读 file.coverPath)
  const searchCovers = shallowRef<Map<string, string>>(new Map())

  let searchTimeout: ReturnType<typeof setTimeout> | null = null
  // generation 计数器: 每次重新搜索自增, 让上一轮加载循环在下一个检查点自己退出
  let coverLoadGeneration = 0

  const handleSearch = async (): Promise<void> => {
    if (searchTimeout) {
      clearTimeout(searchTimeout)
    }

    if (!searchTerm.value.trim()) {
      searchResults.value = []
      coverLoadGeneration++ // 取消正在进行的封面加载
      return
    }

    searchTimeout = setTimeout(() => {
      const lowerCaseSearchTerm = searchTerm.value.toLowerCase()
      const uniqueResults = new Map<string, Track>()

      for (const playlist of playlists.value) {
        if (playlist.files) {
          const results = playlist.files.filter(
            (file) =>
              (file.title && file.title.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.artist && file.artist.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.album && file.album.toLowerCase().includes(lowerCaseSearchTerm)) ||
              (file.name && file.name.toLowerCase().includes(lowerCaseSearchTerm)),
          )

          for (const file of results) {
            if (!uniqueResults.has(file.path)) {
              uniqueResults.set(file.path, file)
            }
          }
        }
      }

      const isAscOrder = configStore.playlist.sortOrder === 'asc'
      searchResults.value = Array.from(uniqueResults.values()).sort((a, b) => {
        const titleA = (a.title || a.name || '').toLowerCase()
        const titleB = (b.title || b.name || '').toLowerCase()

        if (isAscOrder) {
          if (titleA < titleB) return -1
          if (titleA > titleB) return 1
        } else {
          if (titleA > titleB) return -1
          if (titleA < titleB) return 1
        }

        return 0
      })

      // 从缓存恢复的 track 没有 coverPath, 先用已有值播种 Map 再异步补齐
      searchCovers.value = new Map(
        searchResults.value.filter((f) => f.coverPath).map((f) => [f.path, f.coverPath as string]),
      )
      coverLoadGeneration++
      void loadSearchResultCovers(coverLoadGeneration)
    }, 300)
  }

  // 每批 BATCH 首并行加载, 批间检查 generation 以便被新一轮搜索打断; 整体替换 Map 的渲染理由见 searchCovers 注释
  // 每条封面同时调 playerStore.recordCoverUpdate, 播放队列视图靠 store 的版本号自行刷新
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
                // 写回原对象供播放等逻辑直接读 coverPath, 本组件渲染不依赖它
                file.coverPath = coverPath
                found.push([file.path, coverPath])
              }
            } catch {
              // 单首封面加载失败不影响其它
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
