import { ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import logger from '@/utils/logger'
import type { Track } from '@/types'

/**
 * 专辑封面交互 Composable
 *
 * 管理封面右下角"提取封面"按钮的显隐判定，以及点击后通过后端导出封面图片的逻辑。
 * 鼠标移入封面右下角 80x80 区域时显示按钮，离开封面区域时隐藏。
 */
export function useAlbumArtInteraction(currentTrack: Ref<Track | null>) {
  const showExtractButton = ref(false)

  // 处理专辑封面鼠标移动事件，检测是否在右下角区域
  const handleAlbumArtMouseMove = (event: MouseEvent): void => {
    if (!currentTrack.value || !currentTrack.value.coverPath) {
      showExtractButton.value = false
      return
    }

    const wrapper = event.currentTarget as HTMLElement
    const rect = wrapper.getBoundingClientRect()
    const x = event.clientX - rect.left
    const y = event.clientY - rect.top

    // 定义右下角区域（右下角80x80像素区域）
    const cornerSize = 80
    showExtractButton.value = x >= rect.width - cornerSize && y >= rect.height - cornerSize
  }

  // 鼠标离开封面区域时隐藏按钮
  const handleAlbumArtMouseLeave = (): void => {
    showExtractButton.value = false
  }

  // 提取封面功能
  const extractCover = async (): Promise<void> => {
    if (!currentTrack.value || !currentTrack.value.path) return

    try {
      // 获取默认文件名（基于音频文件名）
      const audioPath = currentTrack.value.path
      const baseName = audioPath.replace(/\.[^/.]+$/, '') // 移除扩展名
      const defaultName = baseName.split(/[/\\]/).pop() + '_cover'

      // 打开保存对话框
      const savePath = await save({
        defaultPath: defaultName,
        filters: [
          {
            name: 'Image',
            extensions: ['jpg', 'png', 'webp'],
          },
        ],
      })

      if (!savePath) return // 用户取消

      // 调用后端提取封面
      const result = await invoke('extract_cover', {
        audioPath: audioPath,
        outputPath: savePath,
      })

      logger.info('Cover extracted to:', result)
    } catch (error) {
      logger.error('Failed to extract cover:', error)
    }
  }

  return {
    showExtractButton,
    handleAlbumArtMouseMove,
    handleAlbumArtMouseLeave,
    extractCover,
  }
}
