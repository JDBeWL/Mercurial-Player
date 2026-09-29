import { ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import logger from '@/utils/logger'
import errorHandler, { ErrorSeverity } from '@/utils/errorHandler'
import type { Track } from '@/types'

/** 长按判定时长（毫秒）。太短会与普通点击混淆，太长用户会以为没反应 */
const LONG_PRESS_MS = 450

/**
 * 从音频路径里解出"不带扩展名的文件名主体"。
 *
 * ⚠️ 安卓的 path 是 content URI，document id 里的分隔符被编码成 `%2F`
 * （如 `content://.../document/primary%3AMusic%2Fmillsage%2F01 everscape.flac`）。
 * 直接按 `/` 切会拿到**整段 document id** —— 真机上实测保存对话框里出现了
 * `primary%3AMusic%2Fm` 这种乱码文件名。取编码段里的最后一段再解码才是真名。
 */
function stemFromAudioPath(audioPath: string): string {
  const lastSegment = audioPath.split(/[/\\]/).pop() ?? ''
  const encoded = lastSegment.split(/%2f/i).pop() ?? lastSegment
  let decoded = encoded
  try {
    decoded = decodeURIComponent(encoded)
  } catch {
    // 畸形编码（孤立的 %）就按原文用
  }
  return decoded.replace(/\.[^/.]+$/, '')
}

/** 提取封面的结果：调用方据此决定要不要提示成功 */
export type ExtractCoverResult = 'saved' | 'cancelled' | 'failed'

/**
 * 专辑封面交互 Composable
 *
 * - **桌面端**：鼠标移入封面右下角 80x80 区域时显示"提取封面"按钮。
 * - **手机端**：触摸屏没有 hover，按钮永远点不出来，改成**长按封面**弹出菜单
 *   （`showCoverMenu`）。菜单由调用方渲染，判定逻辑留在这里，避免把触摸手势
 *   散落进模板。
 *
 * @param options.longPressEnabled 是否启用长按手势（传平台判断，只有 Android 为 true）
 */
export function useAlbumArtInteraction(
  currentTrack: Ref<Track | null>,
  options: { longPressEnabled?: Ref<boolean> } = {},
) {
  const showExtractButton = ref(false)
  const showCoverMenu = ref(false)

  /**
   * 长按是否已经触发过。
   * 手指抬起后浏览器还会补一次 click，若不吞掉，用户长按弹出菜单的同时
   * 还会把封面点成"翻到歌词"（竖屏点封面的语义）—— 菜单一出来就没了。
   */
  let longPressFired = false
  let pressTimer: ReturnType<typeof setTimeout> | null = null

  const clearPressTimer = (): void => {
    if (pressTimer !== null) {
      clearTimeout(pressTimer)
      pressTimer = null
    }
  }

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

  /** 按下：手机端起长按计时器 */
  const handleCoverPointerDown = (): void => {
    if (!options.longPressEnabled?.value) return
    if (!currentTrack.value?.coverPath) return
    longPressFired = false
    clearPressTimer()
    pressTimer = setTimeout(() => {
      pressTimer = null
      longPressFired = true
      showCoverMenu.value = true
    }, LONG_PRESS_MS)
  }

  /** 抬起 / 取消 / 移出：撤掉计时器（没到时间就只是一次普通点击） */
  const handleCoverPointerUp = (): void => {
    clearPressTimer()
  }

  const closeCoverMenu = (): void => {
    showCoverMenu.value = false
  }

  /**
   * 消费长按留下的那次 click。
   *
   * @returns true 表示这次点击属于长按手势，调用方应直接返回、别做别的动作
   */
  const consumeLongPressClick = (): boolean => {
    if (!longPressFired) return false
    longPressFired = false
    return true
  }

  /**
   * 封面长按菜单里的"提取封面"。
   *
   * 刻意失败也返回 `failed` 而不是抛异常：调用方只需要知道要不要打成功提示，
   * 具体失败原因交给 errorHandler 弹给用户（这条链路上原来的写法是只写日志，
   * 手机上点了没反应、也没有任何提示）。
   */
  const extractCover = async (): Promise<ExtractCoverResult> => {
    const track = currentTrack.value
    if (!track || !track.path) return 'cancelled'

    try {
      // 默认文件名带上封面真实扩展名。
      // 安卓的保存对话框返回的是 content:// URI，没有"按 MIME 补扩展名"这一步，
      // 名称里不带扩展名的话落盘就是一个没有后缀的文件；而且后端对 content URI
      // 也没法再补（见 extract_cover_internal）。
      const audioPath = track.path
      const rawStem =
        stemFromAudioPath(audioPath).trim() || track.title?.trim() || 'cover'
      // SAF 目标文件名的非法字符，替换成下划线（曲目名里可能出现 / 等字符）
      const stem = rawStem.replace(/[\\/:*?"<>|]/g, '_') || 'cover'
      const coverExt = (track.coverPath?.split('.').pop() ?? '').toLowerCase()
      const ext = ['jpg', 'jpeg', 'png', 'webp'].includes(coverExt) ? coverExt : 'jpg'

      // 打开保存对话框。安卓走 SAF，返回 content:// URI；桌面返回本地路径。
      const savePath = await save({
        defaultPath: `${stem}_cover.${ext}`,
        filters: [
          {
            name: 'Image',
            extensions: ['jpg', 'png', 'webp'],
          },
        ],
      })

      if (!savePath) return 'cancelled' // 用户取消

      // 调用后端提取封面
      const result = await invoke('extract_cover', {
        audioPath: audioPath,
        outputPath: savePath,
      })

      logger.info('Cover extracted to:', result)
      return 'saved'
    } catch (error) {
      logger.error('Failed to extract cover:', error)
      const message = error instanceof Error ? error.message : String(error)
      errorHandler.handle(new Error(message), {
        severity: ErrorSeverity.MEDIUM,
        showToUser: true,
        userMessage: message,
      })
      return 'failed'
    }
  }

  return {
    showExtractButton,
    showCoverMenu,
    handleAlbumArtMouseMove,
    handleAlbumArtMouseLeave,
    handleCoverPointerDown,
    handleCoverPointerUp,
    closeCoverMenu,
    consumeLongPressClick,
    extractCover,
  }
}
