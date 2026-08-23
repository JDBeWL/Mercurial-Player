import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { register, unregisterAll, isRegistered } from '@tauri-apps/plugin-global-shortcut'
import logger from '@/utils/logger'
import i18n from '@/i18n'
import errorHandler, { ErrorType, ErrorSeverity } from '@/utils/errorHandler'
import type { usePlayerStore } from './player'

/**
 * Player store 的监听器设置函数,从 player.ts 抽离以降低单文件复杂度。
 * 所有函数接收 store 实例参数,在运行时与 player store 共享同一 Pinia 实例。
 */
type PlayerStore = ReturnType<typeof usePlayerStore>

/** 设置 track-ended 事件监听,返回 unlisten 函数 */
export async function setupTrackEndedListener(store: PlayerStore): Promise<UnlistenFn | null> {
  try {
    return await listen('track-ended', () => {
      if (store._isDestroyed) return
      logger.debug('Received track-ended event')
      store._onEnded()
    })
  } catch (err) {
    logger.error('Failed to setup track-ended listener:', err)
    return null
  }
}

/** 设置 playback-position 事件监听,返回 unlisten 函数 */
export async function setupPositionListener(store: PlayerStore): Promise<UnlistenFn | null> {
  try {
    return await listen<{ position: number }>('playback-position', (event) => {
      if (store._isDestroyed || !store.isPlaying) return
      const position = event.payload?.position
      if (typeof position === 'number' && position >= 0) {
        store.currentTime = position
      }
    })
  } catch (err) {
    logger.error('Failed to setup position listener:', err)
    return null
  }
}

export interface TaskbarListeners {
  previous: UnlistenFn | null
  playPause: UnlistenFn | null
  next: UnlistenFn | null
}

/** 设置任务栏按钮事件监听 (上一首/播放暂停/下一首) */
export async function setupTaskbarListeners(store: PlayerStore): Promise<TaskbarListeners> {
  const result: TaskbarListeners = { previous: null, playPause: null, next: null }
  try {
    result.previous = await listen('taskbar-previous', () => {
      if (store._isDestroyed) return
      logger.debug('Taskbar: Previous button clicked')
      store.previousTrack()
    })

    result.playPause = await listen('taskbar-play-pause', () => {
      if (store._isDestroyed) return
      logger.debug('Taskbar: Play/Pause button clicked')
      store.togglePlay()
    })

    result.next = await listen('taskbar-next', () => {
      if (store._isDestroyed) return
      logger.debug('Taskbar: Next button clicked')
      store.nextTrack()
    })

    logger.info('Taskbar listeners setup complete')
  } catch (err) {
    logger.error('Failed to setup taskbar listeners:', err)
  }
  return result
}

/**
 * 注册全局媒体键快捷方式 (MediaPlayPause / MediaTrackNext / MediaTrackPrevious)
 */
export async function setupGlobalShortcuts(store: PlayerStore): Promise<void> {
  const shortcuts = [
    { key: 'MediaPlayPause', handler: () => store.togglePlay() },
    { key: 'MediaTrackNext', handler: () => store.nextTrack() },
    { key: 'MediaTrackPrevious', handler: () => store.previousTrack() },
  ]

  for (const { key, handler } of shortcuts) {
    try {
      if (await isRegistered(key)) {
        logger.debug(`Shortcut ${key} already registered, skipping`)
        continue
      }

      await register(key, () => {
        if (store._isDestroyed) return
        logger.debug(`Global shortcut: ${key}`)
        handler()
      })
    } catch (err: unknown) {
      // 如果错误信息包含 "already registered",说明已经注册过了,可以忽略
      const errorMsg = String(err)
      if (errorMsg.includes('already registered')) {
        logger.debug(`Shortcut ${key} was already registered (caught exception)`)
      } else {
        logger.error(`Failed to register shortcut ${key}:`, err)
      }
    }
  }

  logger.info('Global media shortcuts setup complete')
}

/** 注销所有全局媒体键快捷方式 */
export async function unregisterGlobalShortcuts(): Promise<void> {
  try {
    await unregisterAll()
    logger.debug('Global shortcuts unregistered')
  } catch {
    // 忽略清理错误
  }
}

export interface DeviceListeners {
  removed: UnlistenFn | null
  switchRequired: UnlistenFn | null
  noDevice: UnlistenFn | null
  defaultChanged: UnlistenFn | null
}

/**
 * 设置音频设备事件监听 (设备移除/切换请求/无可用设备/默认设备变更)
 */
export async function setupDeviceListeners(store: PlayerStore): Promise<DeviceListeners> {
  const result: DeviceListeners = {
    removed: null,
    switchRequired: null,
    noDevice: null,
    defaultChanged: null,
  }
  try {
    // 监听设备移除事件
    result.removed = await listen<{ eventType: string; deviceName: string | null }>(
      'device-removed',
      (event) => {
        if (store._isDestroyed) return
        const deviceName = event.payload?.deviceName
        logger.warn(`Audio device removed: ${deviceName}`)
      },
    )

    // 监听设备切换请求事件
    result.switchRequired = await listen<{ eventType: string; deviceName: string | null }>(
      'device-switch-required',
      async (event) => {
        if (store._isDestroyed) return
        const deviceName = event.payload?.deviceName
        if (!deviceName) return

        logger.info(`Switching to fallback device: ${deviceName}`)
        await store._switchAudioDevice(
          deviceName,
          'switch-fallback-success',
          i18n.global.t('errors.audioDeviceDisconnectedSwitched', { device: deviceName }),
          'switch-fallback',
          ErrorSeverity.HIGH,
        )
      },
    )

    // 监听无可用设备事件
    result.noDevice = await listen('no-device-available', () => {
      if (store._isDestroyed) return
      logger.error('No audio device available')

      // 暂停播放
      store.pause()

      errorHandler.handle(new Error('No audio device available'), {
        type: ErrorType.AUDIO_DEVICE_ERROR,
        severity: ErrorSeverity.CRITICAL,
        context: { action: 'no-device' },
        showToUser: true,
        userMessage: i18n.global.t('errors.noAudioDeviceAvailable'),
      })
    })

    // 监听默认设备变更事件 (新设备添加且为系统默认)
    result.defaultChanged = await listen<{ eventType: string; deviceName: string | null }>(
      'device-default-changed',
      async (event) => {
        if (store._isDestroyed) return
        const deviceName = event.payload?.deviceName
        if (!deviceName) return

        logger.info(`System default device changed to: ${deviceName}`)
        await store._switchAudioDevice(
          deviceName,
          'switch-default-success',
          i18n.global.t('errors.audioDeviceSwitched', { device: deviceName }),
          'switch-default',
          ErrorSeverity.MEDIUM,
        )
      },
    )

    logger.info('Device listeners setup complete')
  } catch (err) {
    logger.error('Failed to setup device listeners:', err)
  }
  return result
}
