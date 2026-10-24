import { beforeEach, describe, expect, it, vi } from 'vitest'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }))

import { analyzeTrackBpm, tracksNeedingBpm } from '@/services/bpmService'
import type { Track } from '@/types'

const track = (path: string, bpm?: number | null): Track =>
  ({ path, name: path, bpm: bpm ?? null }) as unknown as Track

beforeEach(() => {
  invokeMock.mockReset()
})

describe('tracksNeedingBpm', () => {
  it('keeps tracks without a numeric bpm', () => {
    const tracks = [track('/a.mp3'), track('/b.mp3'), track('/c.mp3')]
    expect(tracksNeedingBpm(tracks).map((t) => t.path)).toEqual(['/a.mp3', '/b.mp3', '/c.mp3'])
  })

  it('skips tracks that already have a bpm', () => {
    const tracks = [track('/a.mp3', 120), track('/b.mp3')]
    expect(tracksNeedingBpm(tracks).map((t) => t.path)).toEqual(['/b.mp3'])
  })

  it('ignores tracks with an empty path', () => {
    expect(tracksNeedingBpm([{ path: '', bpm: null } as unknown as Track])).toEqual([])
  })
})

describe('analyzeTrackBpm', () => {
  it('does not call the backend when the track already has a bpm', async () => {
    const t = track('/a.mp3', 120)
    await expect(analyzeTrackBpm(t)).resolves.toBe(120)
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('ignores a track with an empty path', async () => {
    await expect(analyzeTrackBpm(track(''))).resolves.toBeNull()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('writes the detected bpm back onto the track', async () => {
    invokeMock.mockResolvedValue([{ path: '/a.mp3', bpm: 97.5 }])
    const t = track('/a.mp3')
    await expect(analyzeTrackBpm(t)).resolves.toBe(97.5)
    expect(invokeMock).toHaveBeenCalledWith('analyze_tracks_bpm', { paths: ['/a.mp3'] })
    expect(t.bpm).toBe(97.5)
  })

  it('asks for exactly one path, never the whole playlist', async () => {
    invokeMock.mockResolvedValue([{ path: '/a.mp3', bpm: 128 }])
    await analyzeTrackBpm(track('/a.mp3'))
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(invokeMock.mock.calls[0]![1]).toEqual({ paths: ['/a.mp3'] })
  })

  it('leaves the track untouched when the backend reports no result', async () => {
    invokeMock.mockResolvedValue([{ path: '/a.mp3', bpm: null }])
    const t = track('/a.mp3')
    await expect(analyzeTrackBpm(t)).resolves.toBeNull()
    expect(t.bpm).toBeNull()
  })

  it('returns null when the backend payload is not an array', async () => {
    // 后端异常时会回 null；调用方是后台任务，抛出的异常没人接会变成 unhandled rejection
    invokeMock.mockResolvedValue(null)
    const t = track('/a.mp3')
    await expect(analyzeTrackBpm(t)).resolves.toBeNull()
    expect(t.bpm).toBeNull()
  })

  it('swallows a backend failure instead of rejecting', async () => {
    invokeMock.mockRejectedValue(new Error('backend down'))
    await expect(analyzeTrackBpm(track('/a.mp3'))).resolves.toBeNull()
  })

  it('does nothing when the signal is already aborted', async () => {
    const controller = new AbortController()
    controller.abort()
    await expect(
      analyzeTrackBpm(track('/a.mp3'), { signal: controller.signal }),
    ).resolves.toBeNull()
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('drops the result when aborted while the request was in flight', async () => {
    const controller = new AbortController()
    invokeMock.mockImplementation(() => {
      controller.abort()
      return Promise.resolve([{ path: '/a.mp3', bpm: 120 }])
    })
    await expect(
      analyzeTrackBpm(track('/a.mp3'), { signal: controller.signal }),
    ).resolves.toBeNull()
  })
})
