import { watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { usePlayerStore } from '@/stores/player'
import { useConfigStore } from '@/stores/config'
import logger from '@/utils/logger'

let lastCurrentLine = ''
let lastSubLine = ''
let lastProgress = -1
let lastWordsKey = ''
let lastSyncedTime = Number.NaN
let lastIsPlaying = false

let isInitialized = false
let updateFrameId: number | null = null
let updateInFlight = false
let updateQueued = false
let updateIntervalId: number | null = null

const stopFns: Array<() => void> = []
const unlistenFns: Array<() => void> = []

// 引用计数:跟踪当前有多少组件正在使用本 composable。
// 只有最后一个组件卸载时 (refCount 归零) 才真正清理全局监听器/watcher,
// 避免先卸载的组件停掉其他仍挂载组件共享的监听器。
let refCount = 0

function getSubLine(lyrics: any[], index: number): string {
  if (index < 0 || index >= lyrics.length) return ''
  const line = lyrics[index]
  if (line.texts && line.texts.length > 1 && line.texts[1]) {
    return line.texts[1]
  }
  return ''
}

function getCurrentWords(
  lyrics: any[],
  index: number,
): Array<{ text: string; start: number; end: number }> {
  if (index < 0 || index >= lyrics.length) return []
  const line = lyrics[index]
  return Array.isArray(line?.words)
    ? line.words
        .map((word: any) => ({
          text: String(word?.text ?? ''),
          start: Number(word?.start),
          end: Number(word?.end),
        }))
        .filter(
          (word: { text: string; start: number; end: number }) =>
            word.text.length > 0 &&
            Number.isFinite(word.start) &&
            Number.isFinite(word.end) &&
            word.end > word.start,
        )
    : []
}

function getCurrentLine(lyrics: any[], index: number): string {
  if (index < 0 || index >= lyrics.length) return ''
  const line = lyrics[index]
  if (line.texts && line.texts.length > 0) return line.texts[0] || ''
  return line.text || ''
}

function clampProgress(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function getCurrentLyricProgress(
  lyrics: any[] | null,
  index: number,
  currentTime: number,
  lyricsOffset: number,
): number {
  if (!lyrics || index < 0 || index >= lyrics.length) return 0

  const current = lyrics[index]
  const words = Array.isArray(current?.words) ? current.words : []

  // 只有真正带逐字/逐词时间轴的 ASS 歌词才做高亮推进。
  // 普通 LRC 只有逐行时间，没有 words，这里固定返回 0，避免伪过渡效果。
  if (words.length === 0) return 0

  const syncedTime = currentTime - (Number.isFinite(lyricsOffset) ? lyricsOffset : 0)
  const validWords = words
    .map((word: any) => ({
      text: String(word?.text ?? ''),
      start: Number(word?.start),
      end: Number(word?.end),
    }))
    .filter(
      (word: { text: string; start: number; end: number }) =>
        word.text.length > 0 &&
        Number.isFinite(word.start) &&
        Number.isFinite(word.end) &&
        word.end > word.start,
    )

  if (validWords.length === 0) return 0

  const firstStart = validWords[0].start
  const lastEnd = validWords[validWords.length - 1].end
  if (syncedTime <= firstStart) return 0
  if (syncedTime >= lastEnd) return 1

  const totalChars = validWords.reduce(
    (sum: number, word: { text: string }) => sum + word.text.length,
    0,
  )
  if (totalChars <= 0) return 0

  let passedChars = 0
  for (const word of validWords) {
    const charCount = word.text.length
    if (syncedTime >= word.end) {
      passedChars += charCount
      continue
    }
    if (syncedTime > word.start) {
      passedChars += charCount * ((syncedTime - word.start) / (word.end - word.start))
    }
    break
  }

  return clampProgress(passedChars / totalChars)
}

async function updateDesktopLyrics() {
  const configStore = useConfigStore()
  if (!configStore.lyrics?.desktopLyrics?.enabled) return

  const playerStore = usePlayerStore()
  const lyrics = playerStore.lyrics
  const currentIndex = playerStore.currentLyricIndex
  const currentWords = lyrics && currentIndex >= 0 ? getCurrentWords(lyrics, currentIndex) : []
  const syncedTime = playerStore.currentTime - (playerStore.lyricsOffset || 0)
  const isPlaying = playerStore.isPlaying

  let currentLine = ''
  let subLine = ''

  if (lyrics && currentIndex >= 0 && currentIndex < lyrics.length) {
    currentLine = getCurrentLine(lyrics, currentIndex)
    subLine = getSubLine(lyrics, currentIndex)
  }

  const progress = getCurrentLyricProgress(
    lyrics,
    currentIndex,
    playerStore.currentTime,
    playerStore.lyricsOffset,
  )
  const wordsKey = currentWords.map((word) => `${word.start}:${word.end}:${word.text}`).join('|')

  if (
    currentLine === lastCurrentLine &&
    subLine === lastSubLine &&
    wordsKey === lastWordsKey &&
    isPlaying === lastIsPlaying &&
    Math.abs(syncedTime - lastSyncedTime) < 0.02 &&
    Math.abs(progress - lastProgress) < 0.008
  ) {
    return
  }

  lastCurrentLine = currentLine
  lastSubLine = subLine
  lastProgress = progress
  lastWordsKey = wordsKey
  lastSyncedTime = syncedTime
  lastIsPlaying = isPlaying

  try {
    await invoke('update_desktop_lyric', {
      currentLine,
      subLine,
      progress,
      words: currentWords,
      currentTime: syncedTime,
      isPlaying,
    })
  } catch {
    // non-Windows platform or not initialized
  }
}

function scheduleDesktopLyricsUpdate() {
  if (updateFrameId !== null) return

  updateFrameId = window.requestAnimationFrame(() => {
    updateFrameId = null

    if (updateInFlight) {
      updateQueued = true
      return
    }

    updateInFlight = true
    void (async () => {
      try {
        await updateDesktopLyrics()
      } finally {
        updateInFlight = false
        if (updateQueued) {
          updateQueued = false
          scheduleDesktopLyricsUpdate()
        }
      }
    })()
  })
}

function startDesktopLyricsPolling() {
  if (updateIntervalId !== null) return
  updateIntervalId = window.setInterval(() => {
    scheduleDesktopLyricsUpdate()
  }, 33)
}

function stopDesktopLyricsPolling() {
  if (updateIntervalId === null) return
  window.clearInterval(updateIntervalId)
  updateIntervalId = null
}

async function showDesktopLyrics() {
  try {
    await invoke('show_desktop_lyrics')
  } catch (err) {
    logger.error('Failed to show desktop lyrics:', err)
  }
}

async function hideDesktopLyrics() {
  try {
    await invoke('hide_desktop_lyrics')
  } catch (err) {
    logger.error('Failed to hide desktop lyrics:', err)
  }
}

async function syncLockState() {
  const configStore = useConfigStore()
  const locked = configStore.lyrics?.desktopLyrics?.locked ?? true
  try {
    await invoke('set_desktop_lyrics_locked', { locked })
  } catch {
    // ignore
  }
}

async function syncFontSize() {
  const configStore = useConfigStore()
  const fontSize = configStore.lyrics?.desktopLyrics?.fontSize ?? 28
  try {
    await invoke('set_desktop_lyrics_font_size', { size: fontSize })
  } catch {
    // ignore
  }
}

async function syncColorPreset() {
  const configStore = useConfigStore()
  const preset = configStore.lyrics?.desktopLyrics?.colorPreset ?? 'dark'
  try {
    await invoke('set_desktop_lyrics_color_preset', { preset })
  } catch {
    // ignore
  }
}

export function useDesktopLyrics() {
  const playerStore = usePlayerStore()
  const configStore = useConfigStore()

  // 引用计数 +1,跟踪当前使用本 composable 的组件数
  refCount++

  if (!isInitialized) {
    isInitialized = true

    listen('desktop-lyrics-closed', () => {
      logger.info('Desktop lyrics closed from window button')
      configStore.setDesktopLyricsConfig({ enabled: false })
    }).then((unlisten) => unlistenFns.push(unlisten))

    listen<boolean>('desktop-lyrics-lock-changed', (event) => {
      logger.info('Desktop lyrics lock changed:', event.payload)
      configStore.setDesktopLyricsConfig({ locked: event.payload })
    }).then((unlisten) => unlistenFns.push(unlisten))

    const stopWatchLyricIndex = watch(
      () => playerStore.currentLyricIndex,
      () => scheduleDesktopLyricsUpdate(),
    )
    stopFns.push(stopWatchLyricIndex)

    const stopWatchLyricsOffset = watch(
      () => playerStore.lyricsOffset,
      () => scheduleDesktopLyricsUpdate(),
    )
    stopFns.push(stopWatchLyricsOffset)

    const stopWatchLyrics = watch(
      () => playerStore.lyrics,
      () => {
        lastCurrentLine = ''
        lastSubLine = ''
        lastProgress = -1
        scheduleDesktopLyricsUpdate()
      },
    )
    stopFns.push(stopWatchLyrics)

    const stopWatchEnabled = watch(
      () => configStore.lyrics?.desktopLyrics?.enabled,
      (enabled) => {
        if (enabled) {
          showDesktopLyrics()
          syncLockState()
          syncFontSize()
          syncColorPreset()
          startDesktopLyricsPolling()
          scheduleDesktopLyricsUpdate()
        } else {
          stopDesktopLyricsPolling()
          hideDesktopLyrics()
        }
      },
    )
    stopFns.push(stopWatchEnabled)

    const stopWatchLocked = watch(
      () => configStore.lyrics?.desktopLyrics?.locked,
      () => syncLockState(),
    )
    stopFns.push(stopWatchLocked)

    const stopWatchFontSize = watch(
      () => configStore.lyrics?.desktopLyrics?.fontSize,
      () => syncFontSize(),
    )
    stopFns.push(stopWatchFontSize)

    const stopWatchColorPreset = watch(
      () => configStore.lyrics?.desktopLyrics?.colorPreset,
      () => syncColorPreset(),
    )
    stopFns.push(stopWatchColorPreset)

    const stopWatchTrack = watch(
      () => playerStore.currentTrack?.path,
      () => {
        lastCurrentLine = ''
        lastSubLine = ''
        lastProgress = -1
        scheduleDesktopLyricsUpdate()
      },
    )
    stopFns.push(stopWatchTrack)
  }

  onMounted(() => {
    if (configStore.lyrics?.desktopLyrics?.enabled) {
      showDesktopLyrics()
      syncLockState()
      syncFontSize()
      syncColorPreset()
      startDesktopLyricsPolling()
      scheduleDesktopLyricsUpdate()
    }
  })

  onUnmounted(() => {
    // 引用计数 -1,只有最后一个组件卸载时才真正清理全局资源,
    // 避免先卸载的组件停掉其他仍挂载组件共享的监听器
    refCount--
    if (refCount > 0) return

    stopDesktopLyricsPolling()
    if (updateFrameId !== null) {
      window.cancelAnimationFrame(updateFrameId)
      updateFrameId = null
    }
    // 停止所有 watcher
    stopFns.forEach((fn) => fn())
    stopFns.length = 0
    // 取消所有 Tauri 事件监听
    unlistenFns.forEach((fn) => fn())
    unlistenFns.length = 0
    // 重置初始化标记,允许下次调用重新建立监听器/watcher
    isInitialized = false
  })
}
