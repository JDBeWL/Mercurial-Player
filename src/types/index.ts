// ============ 基础类型 ============

export interface Track {
  path: string
  name?: string
  title?: string
  displayTitle?: string
  artist?: string
  displayArtist?: string
  album?: string
  duration?: number
  bitrate?: number | null
  sampleRate?: number | null
  channels?: number | null
  bitDepth?: number | null
  format?: string | null
}

export interface AudioInfo {
  bitrate: number | null
  sampleRate: number | null
  channels: number | null
  bitDepth: number | null
  format: string | null
}

// ============ 歌词类型 ============

export interface LyricLine {
  time: number
  text?: string
  texts?: string[]
  karaoke?: KaraokeData | null
  words?: KaraokeWord[]
}

export interface KaraokeData {
  fullText: string
  timings: Array<{ time: number; position: number }>
}

export interface KaraokeWord {
  text: string
  start: number
  end: number
}

export type LyricsFormat = 'lrc' | 'ass' | 'srt' | 'auto'

// ============ 播放列表类型 ============

export interface Playlist {
  name: string
  files: Track[]
  totalFiles?: number
}

export type RepeatMode = 'none' | 'track' | 'list'
export type SortOrder = 'asc' | 'desc'

// ============ 配置类型 ============

export interface DirectoryScanConfig {
  enableSubdirectoryScan: boolean
  maxDepth: number
  ignoreHiddenFolders: boolean
  folderBlacklist: string[]
}

export interface TitleExtractionConfig {
  preferMetadata: boolean
  separator: string
  customSeparators: string[]
  hideFileExtension: boolean
  parseArtistTitle: boolean
}

export interface PlaylistConfig {
  generateAllSongsPlaylist: boolean
  folderBasedPlaylists: boolean
  playlistNameFormat: string
  sortOrder: SortOrder
}

export interface GeneralConfig {
  language: string
  theme: string
  startupLoadLastConfig: boolean
  autoSaveConfig: boolean
  showAudioInfo: boolean
}

export interface LyricsConfig {
  enableOnlineFetch: boolean
  autoSaveOnlineLyrics: boolean
  preferTranslation: boolean
  onlineSource: string
  lyricsAlignment: string
  lyricsFontFamily: string
  lyricsStyle: string
}

export interface UIConfig {
  showSettings: boolean
  showConfigPanel: boolean
  miniMode: boolean
}

export interface AudioConfig {
  exclusiveMode: boolean
  volume: number
}

export interface AppConfig {
  musicDirectories: string[]
  directoryScan: DirectoryScanConfig
  titleExtraction: TitleExtractionConfig
  playlist: PlaylistConfig
  general: GeneralConfig
  lyrics: LyricsConfig
  ui: UIConfig
  audio: AudioConfig
}

// ============ 错误处理类型 ============

export enum ErrorType {
  NETWORK = 'NETWORK',
  NETWORK_TIMEOUT = 'NETWORK_TIMEOUT',
  NETWORK_OFFLINE = 'NETWORK_OFFLINE',
  FILE_NOT_FOUND = 'FILE_NOT_FOUND',
  FILE_READ_ERROR = 'FILE_READ_ERROR',
  FILE_WRITE_ERROR = 'FILE_WRITE_ERROR',
  FILE_PERMISSION_DENIED = 'FILE_PERMISSION_DENIED',
  AUDIO_DECODE_ERROR = 'AUDIO_DECODE_ERROR',
  AUDIO_PLAYBACK_ERROR = 'AUDIO_PLAYBACK_ERROR',
  AUDIO_DEVICE_ERROR = 'AUDIO_DEVICE_ERROR',
  CONFIG_LOAD_ERROR = 'CONFIG_LOAD_ERROR',
  CONFIG_SAVE_ERROR = 'CONFIG_SAVE_ERROR',
  CONFIG_INVALID = 'CONFIG_INVALID',
  DATA_PARSE_ERROR = 'DATA_PARSE_ERROR',
  DATA_VALIDATION_ERROR = 'DATA_VALIDATION_ERROR',
  UNKNOWN = 'UNKNOWN'
}

export enum ErrorSeverity {
  LOW = 'LOW',
  MEDIUM = 'MEDIUM',
  HIGH = 'HIGH',
  CRITICAL = 'CRITICAL'
}

export interface ErrorContext {
  [key: string]: unknown
}

export interface ErrorHandlerOptions {
  type?: ErrorType
  severity?: ErrorSeverity
  context?: ErrorContext
  silent?: boolean
  showToUser?: boolean
  userMessage?: string | null
  throw?: boolean
}

export interface HandleResult<T> {
  success: boolean
  data: T | null
  error: AppError | null
}

export interface AppError extends Error {
  type: ErrorType
  severity: ErrorSeverity
  originalError: Error | unknown | null
  context: ErrorContext
  timestamp: string
}

// ============ 日志类型 ============

export enum LogLevel {
  DEBUG = 0,
  INFO = 1,
  WARN = 2,
  ERROR = 3,
  NONE = 4
}

export interface LogData {
  timestamp: string
  level: string
  levelValue: LogLevel
  message: string
  args?: unknown[]
  stack?: string
}

// ============ 主题类型 ============

export type ThemePreference = 'auto' | 'light' | 'dark' | string

export interface TonalVariants {
  [key: string]: string
}

export interface HarmonyColors {
  complementary: string
  analogous1: string
  analogous2: string
  triadic1: string
  triadic2: string
}

// ============ 插件类型 ============

export interface Plugin {
  id: string
  name: string
  version?: string
  description?: string
  author?: string
  activate?: () => void | Promise<void>
  deactivate?: () => void | Promise<void>
  [key: string]: unknown
}

// ============ 缓存类型 ============

export interface CacheItem<T> {
  value: T
  timestamp: number
}

// ============ 统计类型 ============

export interface LibraryStats {
  totalDirectories: number
  totalAudioFiles: number
  totalPlaylists: number
  maxDepth: number
}

export interface ErrorStats {
  total: number
  byType: Record<string, number>
  bySeverity: Record<string, number>
  recent: Array<{ error: object; timestamp: string }>
}
