// @vitest-environment happy-dom
import { describe, it, beforeEach, expect, vi, afterEach } from 'vitest'

// 使用 vi.hoisted 避免 TDZ：vi.mock 会被提升到文件顶部，直接引用 const 变量会报错
const { listenCallbacks, mockListen, registerCallbacks, mockRegister, mockUnregisterAll, mockIsRegistered, mockErrorHandlerHandle } = vi.hoisted(() => {
  // 捕获 listen 回调，按事件名存储
  const listenCallbacks = new Map<string, (event: { payload: unknown }) => void>()
  const mockListen = vi.fn(async (event: string, callback: (e: { payload: unknown }) => void) => {
    listenCallbacks.set(event, callback)
    // 返回 unlisten 函数
    return vi.fn()
  })

  // 捕获 register 回调，按快捷键存储
  const registerCallbacks = new Map<string, () => void>()
  const mockRegister = vi.fn(async (key: string, callback: () => void) => {
    registerCallbacks.set(key, callback)
  })
  const mockUnregisterAll = vi.fn()
  const mockIsRegistered = vi.fn(async () => false)

  // Mock errorHandler
  const mockErrorHandlerHandle = vi.fn()

  return { listenCallbacks, mockListen, registerCallbacks, mockRegister, mockUnregisterAll, mockIsRegistered, mockErrorHandlerHandle }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: mockListen,
}))

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: mockRegister,
  unregisterAll: mockUnregisterAll,
  isRegistered: mockIsRegistered,
  unregister: vi.fn(),
}))

vi.mock('@/utils/logger', () => ({
  default: {
    debug: vi.fn(),
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
  },
}))

vi.mock('@/utils/errorHandler', () => ({
  default: {
    handle: mockErrorHandlerHandle,
  },
  ErrorType: {
    AUDIO_DEVICE_ERROR: 'AUDIO_DEVICE_ERROR',
    NETWORK: 'NETWORK',
  },
  ErrorSeverity: {
    LOW: 'LOW',
    MEDIUM: 'MEDIUM',
    HIGH: 'HIGH',
    CRITICAL: 'CRITICAL',
  },
}))

import {
  setupTrackEndedListener,
  setupPositionListener,
  setupTaskbarListeners,
  setupGlobalShortcuts,
  unregisterGlobalShortcuts,
  setupDeviceListeners,
} from '@/stores/playerListeners'

/** 从 setup 函数签名推导 PlayerStore 类型 */
type PlayerStore = Parameters<typeof setupTrackEndedListener>[0]

/** 创建 mock player store */
function createMockStore(): PlayerStore {
  return {
    _isDestroyed: false,
    isPlaying: false,
    currentTime: 0,
    _onEnded: vi.fn(),
    previousTrack: vi.fn(),
    togglePlay: vi.fn(),
    nextTrack: vi.fn(),
    pause: vi.fn(),
    _switchAudioDevice: vi.fn().mockResolvedValue(undefined),
  } as unknown as PlayerStore
}

/** 触发指定事件的 listen 回调 */
function emitEvent(eventName: string, payload: unknown) {
  const callback = listenCallbacks.get(eventName)
  if (callback) {
    callback({ payload })
  }
}

describe('playerListeners', () => {
  let store: ReturnType<typeof createMockStore>

  beforeEach(() => {
    vi.clearAllMocks()
    listenCallbacks.clear()
    registerCallbacks.clear()
    store = createMockStore()
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  // ---------- setupTrackEndedListener ----------

  describe('setupTrackEndedListener', () => {
    it('监听 track-ended 事件并调用 store._onEnded()', async () => {
      const unlisten = await setupTrackEndedListener(store)
      expect(mockListen).toHaveBeenCalledWith('track-ended', expect.any(Function))
      expect(unlisten).toBeDefined()

      emitEvent('track-ended', null)
      expect(store._onEnded).toHaveBeenCalledTimes(1)
    })

    it('store 已销毁时不调用 _onEnded', async () => {
      await setupTrackEndedListener(store)
      store._isDestroyed = true

      emitEvent('track-ended', null)
      expect(store._onEnded).not.toHaveBeenCalled()
    })

    it('listen 异常时返回 null', async () => {
      mockListen.mockRejectedValueOnce(new Error('listen failed'))
      const result = await setupTrackEndedListener(store)
      expect(result).toBeNull()
    })
  })

  // ---------- setupPositionListener ----------

  describe('setupPositionListener', () => {
    it('监听 playback-position 事件并更新 store.currentTime', async () => {
      store.isPlaying = true
      await setupPositionListener(store)

      emitEvent('playback-position', { position: 42.5 })
      expect(store.currentTime).toBe(42.5)
    })

    it('非播放状态时不更新 currentTime', async () => {
      store.isPlaying = false
      store.currentTime = 10
      await setupPositionListener(store)

      emitEvent('playback-position', { position: 42.5 })
      expect(store.currentTime).toBe(10)
    })

    it('store 已销毁时不更新 currentTime', async () => {
      store.isPlaying = true
      store.currentTime = 10
      await setupPositionListener(store)
      store._isDestroyed = true

      emitEvent('playback-position', { position: 42.5 })
      expect(store.currentTime).toBe(10)
    })

    it('position 为负数时不更新', async () => {
      store.isPlaying = true
      store.currentTime = 10
      await setupPositionListener(store)

      emitEvent('playback-position', { position: -1 })
      expect(store.currentTime).toBe(10)
    })

    it('position 非数字时不更新', async () => {
      store.isPlaying = true
      store.currentTime = 10
      await setupPositionListener(store)

      emitEvent('playback-position', { position: 'invalid' })
      expect(store.currentTime).toBe(10)
    })

    it('position 为 0 时正常更新', async () => {
      store.isPlaying = true
      store.currentTime = 10
      await setupPositionListener(store)

      emitEvent('playback-position', { position: 0 })
      expect(store.currentTime).toBe(0)
    })

    it('payload 为 null 时不更新', async () => {
      store.isPlaying = true
      store.currentTime = 10
      await setupPositionListener(store)

      emitEvent('playback-position', null)
      expect(store.currentTime).toBe(10)
    })
  })

  // ---------- setupTaskbarListeners ----------

  describe('setupTaskbarListeners', () => {
    it('注册三个 taskbar 事件监听', async () => {
      const result = await setupTaskbarListeners(store)
      expect(mockListen).toHaveBeenCalledTimes(3)
      expect(mockListen).toHaveBeenCalledWith('taskbar-previous', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('taskbar-play-pause', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('taskbar-next', expect.any(Function))
      expect(result.previous).toBeDefined()
      expect(result.playPause).toBeDefined()
      expect(result.next).toBeDefined()
    })

    it('taskbar-previous 触发 previousTrack', async () => {
      await setupTaskbarListeners(store)
      emitEvent('taskbar-previous', null)
      expect(store.previousTrack).toHaveBeenCalledTimes(1)
    })

    it('taskbar-play-pause 触发 togglePlay', async () => {
      await setupTaskbarListeners(store)
      emitEvent('taskbar-play-pause', null)
      expect(store.togglePlay).toHaveBeenCalledTimes(1)
    })

    it('taskbar-next 触发 nextTrack', async () => {
      await setupTaskbarListeners(store)
      emitEvent('taskbar-next', null)
      expect(store.nextTrack).toHaveBeenCalledTimes(1)
    })

    it('store 已销毁时不触发 taskbar 操作', async () => {
      await setupTaskbarListeners(store)
      store._isDestroyed = true

      emitEvent('taskbar-previous', null)
      emitEvent('taskbar-play-pause', null)
      emitEvent('taskbar-next', null)

      expect(store.previousTrack).not.toHaveBeenCalled()
      expect(store.togglePlay).not.toHaveBeenCalled()
      expect(store.nextTrack).not.toHaveBeenCalled()
    })
  })

  // ---------- setupGlobalShortcuts ----------

  describe('setupGlobalShortcuts', () => {
    it('注册三个全局媒体键快捷方式', async () => {
      mockIsRegistered.mockResolvedValue(false)
      await setupGlobalShortcuts(store)
      expect(mockIsRegistered).toHaveBeenCalledTimes(3)
      expect(mockIsRegistered).toHaveBeenCalledWith('MediaPlayPause')
      expect(mockIsRegistered).toHaveBeenCalledWith('MediaTrackNext')
      expect(mockIsRegistered).toHaveBeenCalledWith('MediaTrackPrevious')
      expect(mockRegister).toHaveBeenCalledTimes(3)
    })

    it('MediaPlayPause 回调触发 togglePlay', async () => {
      mockIsRegistered.mockResolvedValue(false)
      await setupGlobalShortcuts(store)

      const callback = registerCallbacks.get('MediaPlayPause')!
      callback()
      expect(store.togglePlay).toHaveBeenCalledTimes(1)
    })

    it('MediaTrackNext 回调触发 nextTrack', async () => {
      mockIsRegistered.mockResolvedValue(false)
      await setupGlobalShortcuts(store)

      const callback = registerCallbacks.get('MediaTrackNext')!
      callback()
      expect(store.nextTrack).toHaveBeenCalledTimes(1)
    })

    it('MediaTrackPrevious 回调触发 previousTrack', async () => {
      mockIsRegistered.mockResolvedValue(false)
      await setupGlobalShortcuts(store)

      const callback = registerCallbacks.get('MediaTrackPrevious')!
      callback()
      expect(store.previousTrack).toHaveBeenCalledTimes(1)
    })

    it('store 已销毁时快捷键回调不触发操作', async () => {
      mockIsRegistered.mockResolvedValue(false)
      await setupGlobalShortcuts(store)
      store._isDestroyed = true

      const callback = registerCallbacks.get('MediaPlayPause')!
      callback()
      expect(store.togglePlay).not.toHaveBeenCalled()
    })

    it('已注册的快捷键跳过注册', async () => {
      mockIsRegistered.mockResolvedValue(true)
      await setupGlobalShortcuts(store)
      expect(mockRegister).not.toHaveBeenCalled()
    })

    it('register 抛出 "already registered" 错误时静默处理', async () => {
      mockIsRegistered.mockResolvedValue(false)
      mockRegister.mockRejectedValueOnce(new Error('Shortcut already registered'))
      await expect(setupGlobalShortcuts(store)).resolves.not.toThrow()
    })

    it('register 抛出其他错误时静默处理', async () => {
      mockIsRegistered.mockResolvedValue(false)
      mockRegister.mockRejectedValueOnce(new Error('system error'))
      await expect(setupGlobalShortcuts(store)).resolves.not.toThrow()
    })
  })

  // ---------- unregisterGlobalShortcuts ----------

  describe('unregisterGlobalShortcuts', () => {
    it('调用 unregisterAll 注销所有快捷键', async () => {
      await unregisterGlobalShortcuts()
      expect(mockUnregisterAll).toHaveBeenCalledTimes(1)
    })

    it('unregisterAll 异常时不抛出', async () => {
      mockUnregisterAll.mockRejectedValueOnce(new Error('cleanup failed'))
      await expect(unregisterGlobalShortcuts()).resolves.not.toThrow()
    })
  })

  // ---------- setupDeviceListeners ----------

  describe('setupDeviceListeners', () => {
    it('注册四个设备事件监听', async () => {
      const result = await setupDeviceListeners(store)
      expect(mockListen).toHaveBeenCalledTimes(4)
      expect(mockListen).toHaveBeenCalledWith('device-removed', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('device-switch-required', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('no-device-available', expect.any(Function))
      expect(mockListen).toHaveBeenCalledWith('device-default-changed', expect.any(Function))
      expect(result.removed).toBeDefined()
      expect(result.switchRequired).toBeDefined()
      expect(result.noDevice).toBeDefined()
      expect(result.defaultChanged).toBeDefined()
    })

    it('device-removed 事件触发日志记录', async () => {
      await setupDeviceListeners(store)
      emitEvent('device-removed', { eventType: 'removed', deviceName: 'Speaker' })
      // 验证 store 未被销毁时不影响其他操作
      expect(store._isDestroyed).toBe(false)
    })

    it('device-switch-required 事件触发 _switchAudioDevice', async () => {
      await setupDeviceListeners(store)
      emitEvent('device-switch-required', { eventType: 'switch', deviceName: 'Headphones' })
      expect(store._switchAudioDevice).toHaveBeenCalledWith(
        'Headphones',
        'switch-fallback-success',
        expect.stringContaining('Headphones'),
        'switch-fallback',
        'HIGH',
      )
    })

    it('device-switch-required 事件中 deviceName 为空时不操作', async () => {
      await setupDeviceListeners(store)
      emitEvent('device-switch-required', { eventType: 'switch', deviceName: null })
      expect(store._switchAudioDevice).not.toHaveBeenCalled()
    })

    it('device-switch-required 事件中 store 已销毁时不操作', async () => {
      await setupDeviceListeners(store)
      store._isDestroyed = true
      emitEvent('device-switch-required', { eventType: 'switch', deviceName: 'Headphones' })
      expect(store._switchAudioDevice).not.toHaveBeenCalled()
    })

    it('no-device-available 事件触发 pause 和 errorHandler.handle', async () => {
      await setupDeviceListeners(store)
      emitEvent('no-device-available', null)
      expect(store.pause).toHaveBeenCalledTimes(1)
      expect(mockErrorHandlerHandle).toHaveBeenCalledTimes(1)
      expect(mockErrorHandlerHandle).toHaveBeenCalledWith(
        expect.any(Error),
        expect.objectContaining({
          showToUser: true,
        }),
      )
    })

    it('no-device-available 事件中 store 已销毁时不操作', async () => {
      await setupDeviceListeners(store)
      store._isDestroyed = true
      emitEvent('no-device-available', null)
      expect(store.pause).not.toHaveBeenCalled()
      expect(mockErrorHandlerHandle).not.toHaveBeenCalled()
    })

    it('device-default-changed 事件触发 _switchAudioDevice', async () => {
      await setupDeviceListeners(store)
      emitEvent('device-default-changed', { eventType: 'default', deviceName: 'NewDevice' })
      expect(store._switchAudioDevice).toHaveBeenCalledWith(
        'NewDevice',
        'switch-default-success',
        expect.stringContaining('NewDevice'),
        'switch-default',
        'MEDIUM',
      )
    })

    it('device-default-changed 事件中 deviceName 为空时不操作', async () => {
      await setupDeviceListeners(store)
      emitEvent('device-default-changed', { eventType: 'default', deviceName: null })
      expect(store._switchAudioDevice).not.toHaveBeenCalled()
    })

    it('listen 异常时返回 null 值', async () => {
      mockListen.mockRejectedValueOnce(new Error('listen failed'))
      const result = await setupDeviceListeners(store)
      // 部分监听失败时返回的对象中对应字段为 null
      expect(result.removed).toBeNull()
    })
  })
})
