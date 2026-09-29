// @vitest-environment happy-dom
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import errorHandler from '@/utils/errorHandler'
import type { Track } from '@/types'

vi.mock('@/utils/logger', () => ({
  default: { debug: vi.fn(), info: vi.fn(), warn: vi.fn(), error: vi.fn() },
}))

const { useAlbumArtInteraction } = await import('@/composables/useAlbumArtInteraction')
const logger = (await import('@/utils/logger')).default

const CORNER = 80

/** 构造一个 200x200、位于 (10, 20) 的封面容器 */
function makeWrapper() {
  return {
    getBoundingClientRect: () => ({
      left: 10,
      top: 20,
      width: 200,
      height: 200,
      right: 210,
      bottom: 220,
      x: 10,
      y: 20,
      toJSON: () => ({}),
    }),
  } as unknown as HTMLElement
}

function mouseEvent(clientX: number, clientY: number): MouseEvent {
  return { clientX, clientY, currentTarget: makeWrapper() } as unknown as MouseEvent
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(invoke).mockResolvedValue(undefined)
  vi.mocked(save).mockResolvedValue(null)
  vi.spyOn(errorHandler, 'handle')
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useAlbumArtInteraction - 热区判定', () => {
  it('shows the button inside the bottom-right corner', () => {
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/cover/a.jpg' } as Track)
    const { showExtractButton, handleAlbumArtMouseMove } = useAlbumArtInteraction(track)

    // 右下角区域: x >= 120, y >= 120 (相对坐标)
    handleAlbumArtMouseMove(mouseEvent(10 + 150, 20 + 150))

    expect(showExtractButton.value).toBe(true)
  })

  it('hides the button outside the corner', () => {
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/cover/a.jpg' } as Track)
    const { showExtractButton, handleAlbumArtMouseMove } = useAlbumArtInteraction(track)

    handleAlbumArtMouseMove(mouseEvent(10 + 150, 20 + 10))

    expect(showExtractButton.value).toBe(false)
  })

  it('treats the exact corner boundary as inside', () => {
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/cover/a.jpg' } as Track)
    const { showExtractButton, handleAlbumArtMouseMove } = useAlbumArtInteraction(track)

    handleAlbumArtMouseMove(mouseEvent(10 + 200 - CORNER, 20 + 200 - CORNER))

    expect(showExtractButton.value).toBe(true)
  })

  it('hides the button without a current track', () => {
    const track = ref<Track | null>(null)
    const { showExtractButton, handleAlbumArtMouseMove } = useAlbumArtInteraction(track)

    handleAlbumArtMouseMove(mouseEvent(10 + 190, 20 + 190))

    expect(showExtractButton.value).toBe(false)
  })

  it('hides the button when the track has no cover', () => {
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)
    const { showExtractButton, handleAlbumArtMouseMove } = useAlbumArtInteraction(track)

    handleAlbumArtMouseMove(mouseEvent(10 + 190, 20 + 190))

    expect(showExtractButton.value).toBe(false)
  })

  it('hides the button on mouse leave', () => {
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { showExtractButton, handleAlbumArtMouseMove, handleAlbumArtMouseLeave } =
      useAlbumArtInteraction(track)

    handleAlbumArtMouseMove(mouseEvent(10 + 190, 20 + 190))
    expect(showExtractButton.value).toBe(true)

    handleAlbumArtMouseLeave()
    expect(showExtractButton.value).toBe(false)
  })
})

describe('useAlbumArtInteraction - extractCover', () => {
  it('does nothing without a current track', async () => {
    const track = ref<Track | null>(null)
    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBe('cancelled')
    expect(save).not.toHaveBeenCalled()
  })

  it('does nothing when the track has no path', async () => {
    const track = ref<Track | null>({ coverPath: '/c.jpg' } as Track)
    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBe('cancelled')
    expect(save).not.toHaveBeenCalled()
  })

  it('derives the default file name from the audio path, with the cover extension', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({
      path: 'D:\\Music\\My Song.mp3',
      coverPath: '/cache/abc.png',
    } as Track)

    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBe('saved')

    // 扩展名取自封面缓存文件的真实扩展名：安卓的保存对话框返回 content:// URI，
    // 名称里不带扩展名就会落盘成一个没有后缀的文件
    expect(save).toHaveBeenCalledWith({
      defaultPath: 'My Song_cover.png',
      filters: [{ name: 'Image', extensions: ['jpg', 'png', 'webp'] }],
    })
    expect(invoke).toHaveBeenCalledWith('extract_cover', {
      audioPath: 'D:\\Music\\My Song.mp3',
      outputPath: '/out/cover.png',
    })
  })

  it('falls back to jpg when the cover path has an unexpected extension', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.jpg')
    const track = ref<Track | null>({ path: '/a.flac', coverPath: '/cache/no-ext' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'a_cover.jpg' }),
    )
  })

  it('handles unix-style paths', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({ path: '/home/user/track.flac' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: 'track_cover.jpg' }))
  })

  it('decodes the real file name out of an Android content URI', async () => {
    vi.mocked(save).mockResolvedValue('content://downloads/document/123')
    // 真机实测：document id 里的分隔符是编码后的 %2F，按 "/" 切会拿到整段 id，
    // 保存框里出现 "primary%3AMusic%2Fm" 这种乱码文件名
    const track = ref<Track | null>({
      path: 'content://com.android.externalstorage.documents/document/primary%3AMusic%2Fmillsage%2F01%20everscape.flac',
      coverPath: '/cache/abc.png',
    } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: '01 everscape_cover.png' }),
    )
  })

  it('replaces filename-hostile characters from the decoded name', async () => {
    vi.mocked(save).mockResolvedValue('content://downloads/document/123')
    // 解码后可能剩下 ":" 这类文件名非法字符（document id 里的 "primary:Music"）
    const track = ref<Track | null>({
      path: 'content://com.android.externalstorage.documents/document/primary%3AMusic',
      coverPath: '/cache/c.jpg',
    } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: 'primary_Music_cover.jpg' }),
    )
  })

  it('stops when the user cancels the dialog and reports no success', async () => {
    vi.mocked(save).mockResolvedValue(null)
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBe('cancelled')

    expect(invoke).not.toHaveBeenCalled()
    expect(errorHandler.handle).not.toHaveBeenCalled()
  })

  it('logs the backend result', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    vi.mocked(invoke).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(logger.info).toHaveBeenCalledWith('Cover extracted to:', '/out/cover.png')
  })

  it('reports the failure to the user instead of only logging it', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    vi.mocked(invoke).mockRejectedValue(new Error('no embedded cover'))
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    // 手机上"点了没反应"是最糟的反馈：失败必须走到 errorHandler（弹给用户），
    // 但对外只回 'failed'，不抛异常
    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBe('failed')
    expect(logger.error).toHaveBeenCalledWith('Failed to extract cover:', expect.any(Error))
    expect(errorHandler.handle).toHaveBeenCalledTimes(1)
    expect(vi.mocked(errorHandler.handle).mock.calls[0]?.[1]).toMatchObject({ showToUser: true })
  })
})

describe('useAlbumArtInteraction - 长按手势（手机端）', () => {
  const enabled = () => ref(true)

  it('opens the menu after a long press', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { showCoverMenu, handleCoverPointerDown } = useAlbumArtInteraction(track, {
      longPressEnabled: enabled(),
    })

    handleCoverPointerDown()
    expect(showCoverMenu.value).toBe(false)

    vi.advanceTimersByTime(450)
    expect(showCoverMenu.value).toBe(true)
  })

  it('does not open the menu for a short tap', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { showCoverMenu, handleCoverPointerDown, handleCoverPointerUp } = useAlbumArtInteraction(
      track,
      { longPressEnabled: enabled() },
    )

    handleCoverPointerDown()
    vi.advanceTimersByTime(200) // 还没到 450ms
    handleCoverPointerUp()
    vi.advanceTimersByTime(1000)

    expect(showCoverMenu.value).toBe(false)
  })

  it('does nothing when long press is disabled (桌面端)', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { showCoverMenu, handleCoverPointerDown } = useAlbumArtInteraction(track, {
      longPressEnabled: ref(false),
    })

    handleCoverPointerDown()
    vi.advanceTimersByTime(1000)

    expect(showCoverMenu.value).toBe(false)
  })

  it('does not arm the timer without a cover', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)
    const { showCoverMenu, handleCoverPointerDown } = useAlbumArtInteraction(track, {
      longPressEnabled: enabled(),
    })

    handleCoverPointerDown()
    vi.advanceTimersByTime(1000)

    expect(showCoverMenu.value).toBe(false)
  })

  it('consumes the click that follows a long press, exactly once', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { handleCoverPointerDown, consumeLongPressClick } = useAlbumArtInteraction(track, {
      longPressEnabled: enabled(),
    })

    handleCoverPointerDown()
    vi.advanceTimersByTime(450)

    // 手指抬起后浏览器补的那次 click 必须被吞掉，否则菜单一出现就被切成歌词
    expect(consumeLongPressClick()).toBe(true)
    expect(consumeLongPressClick()).toBe(false)
  })

  it('does not consume a click when there was no long press', () => {
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { consumeLongPressClick } = useAlbumArtInteraction(track, {
      longPressEnabled: enabled(),
    })

    expect(consumeLongPressClick()).toBe(false)
  })

  it('closes the menu on request', async () => {
    vi.useFakeTimers()
    const track = ref<Track | null>({ path: '/a.mp3', coverPath: '/c.jpg' } as Track)
    const { showCoverMenu, handleCoverPointerDown, closeCoverMenu } = useAlbumArtInteraction(track, {
      longPressEnabled: enabled(),
    })

    handleCoverPointerDown()
    vi.advanceTimersByTime(450)
    expect(showCoverMenu.value).toBe(true)

    closeCoverMenu()
    expect(showCoverMenu.value).toBe(false)
  })
})
