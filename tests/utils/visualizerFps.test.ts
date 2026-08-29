import { beforeEach, describe, expect, it, vi } from 'vitest'

// vi.mock 工厂会被提升到文件顶部,invokeMock 必须经 vi.hoisted 同步提升。
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

import { applyVisualizerFps } from '@/utils/visualizerFps'

beforeEach(() => {
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
})

describe('applyVisualizerFps', () => {
  it('returns null and does nothing when config is missing', async () => {
    expect(await applyVisualizerFps(undefined)).toBeNull()
    expect(await applyVisualizerFps(null)).toBeNull()
    expect(await applyVisualizerFps({ targetFps: 0, enableVerticalSync: false })).toBeNull()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('applies target FPS as-is when the limit is off', async () => {
    const result = await applyVisualizerFps({ targetFps: 120, enableVerticalSync: false })
    expect(result).toEqual({ fps: 120, screenRate: null })
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(invokeMock).toHaveBeenCalledWith('set_target_fps', { fps: 120 })
  })

  it('caps target FPS at the live screen refresh rate when the limit is on', async () => {
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === 'get_screen_refresh_rate' ? 60 : undefined,
    )
    const result = await applyVisualizerFps({ targetFps: 144, enableVerticalSync: true })
    expect(result).toEqual({ fps: 60, screenRate: 60 })
    expect(invokeMock).toHaveBeenCalledWith('set_target_fps', { fps: 60 })
  })

  it('falls back to target FPS when the rate query fails', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_screen_refresh_rate') {
        throw new Error('no display')
      }
      return undefined
    })
    const result = await applyVisualizerFps({ targetFps: 144, enableVerticalSync: true })
    expect(result).toEqual({ fps: 144, screenRate: null })
    expect(invokeMock).toHaveBeenCalledWith('set_target_fps', { fps: 144 })
  })
})
