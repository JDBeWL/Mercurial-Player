import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: {
    fileExists: vi.fn(async () => true),
    findLyricsFile: vi.fn(async () => null),
    readFile: vi.fn(async () => ''),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  default: {
    parseAsync: vi.fn(async () => []),
  },
}))

import { setActivePinia, createPinia } from 'pinia'
import { ErrorType } from '@/utils/errorHandler'
import errorHandler from '@/utils/errorHandler'
import { usePlayerStore } from '@/stores/player'
import { invoke } from '@tauri-apps/api/core'
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/utils/fileUtils', () => ({
  default: {
    fileExists: vi.fn(async () => true),
    findLyricsFile: vi.fn(async () => null),
    readFile: vi.fn(async () => ''),
    getFileName: vi.fn((p: string) => p.split(/[\\/]/).pop() || p),
    getFileExtension: vi.fn(() => 'mp3'),
  },
}))

vi.mock('@/utils/lyricsParser', () => ({
  default: {
    parseAsync: vi.fn(async () => []),
  },
}))

const track = {
  path: '/music/test.mp3',
  name: 'Test',
} as any

const invokeMock = vi.mocked(invoke)

describe('usePlayerStore playTrack error classification', () => {

  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    invokeMock.mockResolvedValue(undefined)
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  async function expectErrorType(err: unknown, expectedType: ErrorType) {
    const store = usePlayerStore()
    invokeMock.mockReset()
    invokeMock.mockResolvedValue(undefined) // default for any extra calls
    invokeMock.mockResolvedValueOnce(undefined) // pause_track
    invokeMock.mockRejectedValueOnce(err) // play_track
    const handleSpy = vi.spyOn(errorHandler, 'handle')

    await store.playTrack(track)

    expect(handleSpy).toHaveBeenCalled()
    const lastCall = handleSpy.mock.calls[handleSpy.mock.calls.length - 1] || []
    const handledError = lastCall[0]
    const options = lastCall[1]
    expect((handledError as Error).message).toBe(err instanceof Error ? err.message : String(err))
    expect(options?.type).toBe(expectedType)
  }

  it('maps decode/probe errors to AUDIO_DECODE_ERROR', async () => {
    await expectErrorType(new Error('Failed to probe format: end of stream'), ErrorType.AUDIO_DECODE_ERROR)
    await expectErrorType(new Error('Unrecognized format'), ErrorType.AUDIO_DECODE_ERROR)
  })

  it('maps device/output errors to AUDIO_DEVICE_ERROR', async () => {
    await expectErrorType(new Error('Failed to open output stream'), ErrorType.AUDIO_DEVICE_ERROR)
    await expectErrorType(new Error('Audio device not found'), ErrorType.AUDIO_DEVICE_ERROR)
  })

  it('falls back to AUDIO_PLAYBACK_ERROR for unknown errors', async () => {
    await expectErrorType(new Error('random failure'), ErrorType.AUDIO_PLAYBACK_ERROR)
  })
})


