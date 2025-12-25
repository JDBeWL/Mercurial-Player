import { ref, watch } from 'vue'
import { useConfigStore } from '@/stores/config'
import FileUtils from '@/utils/fileUtils'
import { TitleExtractor } from '@/utils/titleExtractor'
import logger from '@/utils/logger'

/**
 * 音轨信息处理 composable
 * 用于智能提取和缓存音轨的标题、艺术家等信息
 */
export function useTrackInfo() {
    const configStore = useConfigStore()

    // 存储处理后的音轨信息
    const processedTracks = ref({})

    /**
     * 获取音轨标题
     * @param {Object} track - 音轨对象
     * @param {string} fallback - 无音轨时的回退文本
     * @returns {string} 音轨标题
     */
    const getTrackTitle = (track, fallback = '') => {
        if (!track || !track.path) {
            return fallback
        }

        const trackPath = track.path

        // 如果已经处理过该音轨，直接返回结果
        if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
            return processedTracks.value[trackPath].title
        }

        // 异步处理音轨信息，但不阻塞当前渲染
        if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
            processTrackInfo(trackPath)
        }

        // 如果还没处理完，暂时返回track中已有的name或文件名
        return track.name || FileUtils.getFileName(trackPath)
    }

    /**
     * 获取音轨艺术家
     * @param {Object} track - 音轨对象
     * @param {string} fallback - 无艺术家时的回退文本
     * @returns {string} 艺术家名称
     */
    const getTrackArtist = (track, fallback = '') => {
        if (!track || !track.path) {
            return fallback
        }

        const trackPath = track.path

        // 如果已经处理过该音轨，直接返回结果
        if (processedTracks.value[trackPath] && !processedTracks.value[trackPath].processing) {
            return processedTracks.value[trackPath].artist
        }

        // 异步处理音轨信息，但不阻塞当前渲染
        if (!processedTracks.value[trackPath] || !processedTracks.value[trackPath].processing) {
            processTrackInfo(trackPath)
        }

        // 如果还没处理完，暂时返回track中已有的artist信息
        return track.artist || fallback
    }

    /**
     * 异步处理音轨信息
     * @param {string} trackPath - 音轨文件路径
     */
    const processTrackInfo = async (trackPath) => {
        try {
            // 如果已经在处理中，跳过
            if (processedTracks.value[trackPath]?.processing) return

            // 标记为处理中
            processedTracks.value[trackPath] = { processing: true }

            // 获取配置
            const config = {
                preferMetadata: configStore.titleExtraction?.preferMetadata ?? true,
                hideFileExtension: configStore.titleExtraction?.hideFileExtension ?? true,
                parseArtistTitle: configStore.titleExtraction?.parseArtistTitle ?? true,
                separator: configStore.titleExtraction?.separator ?? '-',
                customSeparators: configStore.titleExtraction?.customSeparators ?? ['-', '_', '.', ' ']
            }

            // 使用 TitleExtractor 智能提取标题信息
            const titleInfo = await TitleExtractor.extractTitle(trackPath, config)

            // 更新处理结果
            processedTracks.value[trackPath] = {
                processing: false,
                ...titleInfo
            }

        } catch (error) {
            logger.error('处理音轨信息失败:', trackPath, error)
            // 出错时使用文件名作为标题
            processedTracks.value[trackPath] = {
                processing: false,
                title: FileUtils.getFileName(trackPath),
                artist: '',
                fileName: FileUtils.getFileName(trackPath),
                isFromMetadata: false
            }
        }
    }

    /**
     * 设置音轨变化监听器
     * @param {Function} trackGetter - 返回当前音轨的函数（通常是 computed 或 ref）
     * @returns {Function} 停止监听的函数
     */
    const watchTrack = (trackGetter) => {
        return watch(trackGetter, (newTrack) => {
            if (newTrack && newTrack.path) {
                processTrackInfo(newTrack.path)
            }
        }, { immediate: true })
    }

    /**
     * 清除指定音轨的缓存
     * @param {string} trackPath - 音轨文件路径
     */
    const clearCache = (trackPath) => {
        if (trackPath && processedTracks.value[trackPath]) {
            delete processedTracks.value[trackPath]
        }
    }

    /**
     * 清除所有缓存
     */
    const clearAllCache = () => {
        processedTracks.value = {}
    }

    return {
        processedTracks,
        getTrackTitle,
        getTrackArtist,
        processTrackInfo,
        watchTrack,
        clearCache,
        clearAllCache
    }
}
