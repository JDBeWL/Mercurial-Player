// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
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
    await useAlbumArtInteraction(track).extractCover()
    expect(save).not.toHaveBeenCalled()
  })

  it('does nothing when the track has no path', async () => {
    const track = ref<Track | null>({ coverPath: '/c.jpg' } as Track)
    await useAlbumArtInteraction(track).extractCover()
    expect(save).not.toHaveBeenCalled()
  })

  it('derives the default file name from the audio path', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({ path: 'D:\\Music\\My Song.mp3' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith({
      defaultPath: 'My Song_cover',
      filters: [{ name: 'Image', extensions: ['jpg', 'png', 'webp'] }],
    })
    expect(invoke).toHaveBeenCalledWith('extract_cover', {
      audioPath: 'D:\\Music\\My Song.mp3',
      outputPath: '/out/cover.png',
    })
  })

  it('handles unix-style paths', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({ path: '/home/user/track.flac' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(save).toHaveBeenCalledWith(expect.objectContaining({ defaultPath: 'track_cover' }))
  })

  it('stops when the user cancels the dialog', async () => {
    vi.mocked(save).mockResolvedValue(null)
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(invoke).not.toHaveBeenCalled()
  })

  it('logs the backend result', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    vi.mocked(invoke).mockResolvedValue('/out/cover.png')
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    await useAlbumArtInteraction(track).extractCover()

    expect(logger.info).toHaveBeenCalledWith('Cover extracted to:', '/out/cover.png')
  })

  it('logs instead of throwing when the backend fails', async () => {
    vi.mocked(save).mockResolvedValue('/out/cover.png')
    vi.mocked(invoke).mockRejectedValue(new Error('no embedded cover'))
    const track = ref<Track | null>({ path: '/a.mp3' } as Track)

    await expect(useAlbumArtInteraction(track).extractCover()).resolves.toBeUndefined()
    expect(logger.error).toHaveBeenCalledWith('Failed to extract cover:', expect.any(Error))
  })
})
