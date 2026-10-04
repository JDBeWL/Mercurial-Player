import { ref, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import logger from '@/utils/logger'
import errorHandler, { ErrorSeverity } from '@/utils/errorHandler'
import type { Track } from '@/types'

/** 长按判定时长(毫秒)。太短会与普通点击混淆,太长用户会以为没反应 */
const LONG_PRESS_MS = 450

/** 从音频路径解出不带扩展名的文件名主体。Android 的 path 是 content URI,分隔符被编码成 %2F,
 *  直接按 / 切会拿到整段 document id,必须先按 %2F 取末段再 decode */
function stemFromAudioPath(audioPath: string): string {
  const lastSegment = audioPath.split(/[/\\]/).pop() ?? ''
  const encoded = lastSegment.split(/%2f/i).pop() ?? lastSegment
  let decoded = encoded
  try {
    decoded = decodeURIComponent(encoded)
  } catch {
    // 畸形编码 (孤立的 %) 就按原文用
  }
  return decoded.replace(/\.[^/.]+$/, '')
}

/** 提取封面的结果:调用方据此决定要不要提示成功 */
export type ExtractCoverResult = 'saved' | 'cancelled' | 'failed'

/**
 * 专辑封面交互 Composable:触摸屏没有 hover,"提取封面"按钮永远点不出来,故 Android 改用长按封面弹菜单
 *
 * @param options.longPressEnabled 仅 Android 传 true。
 */
export function useAlbumArtInteraction(
  currentTrack: Ref<Track | null>,
  options: { longPressEnabled?: Ref<boolean> } = {},
) {
  const showExtractButton = ref(false)
  const showCoverMenu = ref(false)

  /** 长按是否已触发过。手指抬起后浏览器还会补一次 click,不吞掉的话长按弹菜单的同时会把封面
   *  点成"翻到歌词" (竖屏点封面的语义),菜单一出来就被关掉。 */
  let longPressFired = false
  let pressTimer: ReturnType<typeof setTimeout> | null = null

  /** 按下点,用于区分"按住不动"与"在封面上滑动" */
  let pressStart: { x: number; y: number } | null = null
  /** 长按期间手指移动超过这个距离(css px)就判为滑动,取消长按 */
  const LONG_PRESS_MOVE_TOLERANCE = 10

  const clearPressTimer = (): void => {
    if (pressTimer !== null) {
      clearTimeout(pressTimer)
      pressTimer = null
    }
  }

  const handleAlbumArtMouseMove = (event: MouseEvent): void => {
    if (!currentTrack.value || !currentTrack.value.coverPath) {
      showExtractButton.value = false
      return
    }

    const wrapper = event.currentTarget as HTMLElement
    const rect = wrapper.getBoundingClientRect()
    const x = event.clientX - rect.left
    const y = event.clientY - rect.top

    const cornerSize = 80
    showExtractButton.value = x >= rect.width - cornerSize && y >= rect.height - cornerSize
  }

  const handleAlbumArtMouseLeave = (): void => {
    showExtractButton.value = false
  }

  /** 按下:仅 Android 起长按计时器 */
  const handleCoverPointerDown = (event: PointerEvent): void => {
    if (!options.longPressEnabled?.value) return
    if (!currentTrack.value?.coverPath) return
    longPressFired = false
    pressStart = { x: event.clientX, y: event.clientY }
    clearPressTimer()
    pressTimer = setTimeout(() => {
      pressTimer = null
      pressStart = null
      longPressFired = true
      showCoverMenu.value = true
    }, LONG_PRESS_MS)
  }

  /** 在封面上滑动 (翻歌、拖拽) 不该弹出长按菜单 */
  const handleCoverPointerMove = (event: PointerEvent): void => {
    if (pressTimer === null || !pressStart) return
    const moved = Math.hypot(event.clientX - pressStart.x, event.clientY - pressStart.y)
    if (moved > LONG_PRESS_MOVE_TOLERANCE) {
      clearPressTimer()
      pressStart = null
    }
  }

  /** 抬起 / 取消 / 移出:撤掉计时器 (没到时间就只是一次普通点击) */
  const handleCoverPointerUp = (): void => {
    clearPressTimer()
    pressStart = null
  }

  const closeCoverMenu = (): void => {
    showCoverMenu.value = false
  }

  /** 消费长按留下的那次 click;返回 true 表示这次点击属于长按手势,调用方应直接返回 */
  const consumeLongPressClick = (): boolean => {
    if (!longPressFired) return false
    longPressFired = false
    return true
  }

  /** 封面长按菜单里的"提取封面"。失败也返回 failed 而不抛异常:调用方只需决定要不要打成功提示,
   *  失败原因由 errorHandler 直接弹给用户。 */
  const extractCover = async (): Promise<ExtractCoverResult> => {
    const track = currentTrack.value
    if (!track || !track.path) return 'cancelled'

    try {
      // 默认文件名必须自带扩展名:Android 的保存对话框走 SAF,返回 content:// URI,
      // 名称不含后缀时落盘就是个没有后缀的文件,后端也没法再补 (见 extract_cover_internal)
      const audioPath = track.path
      const rawStem = stemFromAudioPath(audioPath).trim() || track.title?.trim() || 'cover'
      // SAF 目标文件名的非法字符替换成下划线 (曲目名里可能出现 / 等字符)
      const stem = rawStem.replace(/[\\/:*?"<>|]/g, '_') || 'cover'
      const coverExt = (track.coverPath?.split('.').pop() ?? '').toLowerCase()
      const ext = ['jpg', 'jpeg', 'png', 'webp'].includes(coverExt) ? coverExt : 'jpg'

      // 安卓走 SAF 返回 content:// URI,桌面返回本地路径
      const savePath = await save({
        defaultPath: `${stem}_cover.${ext}`,
        filters: [
          {
            name: 'Image',
            extensions: ['jpg', 'png', 'webp'],
          },
        ],
      })

      if (!savePath) return 'cancelled'

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
    handleCoverPointerMove,
    handleCoverPointerUp,
    closeCoverMenu,
    consumeLongPressClick,
    extractCover,
  }
}
