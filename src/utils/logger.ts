/** 统一日志入口: 级别阈值过滤, 控制台与文件双输出, 并保留内存历史供导出 */

import type { LogData } from '@/types'

export enum LogLevel {
  DEBUG = 0,
  INFO = 1,
  WARN = 2,
  ERROR = 3,
  NONE = 4,
}

const LEVEL_NAMES: Record<LogLevel, string> = {
  [LogLevel.DEBUG]: 'DEBUG',
  [LogLevel.INFO]: 'INFO',
  [LogLevel.WARN]: 'WARN',
  [LogLevel.ERROR]: 'ERROR',
  [LogLevel.NONE]: 'NONE',
}

// 日志等级持久化 key(开发者选项中设置, 跨启动生效)
const LOG_LEVEL_STORAGE_KEY = 'mercurial-player.log-level'

const LEVEL_COLORS: Record<LogLevel, string> = {
  [LogLevel.DEBUG]: '#888',
  [LogLevel.INFO]: '#2196F3',
  [LogLevel.WARN]: '#FF9800',
  [LogLevel.ERROR]: '#F44336',
  [LogLevel.NONE]: '#000',
}

class Logger {
  private isDev: boolean
  private isDebug: boolean
  private minLevel: LogLevel
  private enableConsole: boolean
  private enableFile: boolean
  /** 落盘失败告警去重标记, 原因见 outputToFile */
  private fileWriteWarned: boolean
  private logHistory: LogData[]
  private maxHistorySize: number

  constructor() {
    this.isDev = import.meta.env.DEV
    this.isDebug = import.meta.env.MODE === 'development' || import.meta.env.DEBUG === 'true'
    this.minLevel = this.resolveInitialLevel()

    // 经 Tauri 后端写日志目录, 每次启动把旧日志轮转为 -prev.log
    this.enableConsole = true
    this.enableFile = true
    this.fileWriteWarned = false
    this.logHistory = []
    this.maxHistorySize = 100
  }

  /** 优先读取开发者选项中持久化的等级, 否则按环境默认 */
  private resolveInitialLevel(): LogLevel {
    try {
      const saved = localStorage.getItem(LOG_LEVEL_STORAGE_KEY)
      const parsed = saved === null ? NaN : Number(saved)
      if (Object.values(LogLevel).includes(parsed as LogLevel)) {
        return parsed as LogLevel
      }
    } catch {
      // localStorage 不可用(如非浏览器环境)时忽略, 回落默认值
    }
    return this.isDev || this.isDebug ? LogLevel.DEBUG : LogLevel.INFO
  }

  /** 写入 localStorage, 生效范围见 LOG_LEVEL_STORAGE_KEY */
  setMinLevel(level: LogLevel): void {
    this.minLevel = level
    try {
      localStorage.setItem(LOG_LEVEL_STORAGE_KEY, String(level))
    } catch {
      // 持久化失败不影响本次会话的级别生效
    }
  }

  getMinLevel(): LogLevel {
    return this.minLevel
  }

  setConsoleEnabled(enable: boolean): void {
    this.enableConsole = enable
  }

  setFileEnabled(enable: boolean): void {
    this.enableFile = enable
  }

  private formatTimestamp(): string {
    const now = new Date()
    const hours = String(now.getHours()).padStart(2, '0')
    const minutes = String(now.getMinutes()).padStart(2, '0')
    const seconds = String(now.getSeconds()).padStart(2, '0')
    const milliseconds = String(now.getMilliseconds()).padStart(3, '0')
    return `${hours}:${minutes}:${seconds}.${milliseconds}`
  }

  /** 写入日志文件时用于跨天区分, 控制台输出不展示 */
  private formatDate(): string {
    const now = new Date()
    const year = now.getFullYear()
    const month = String(now.getMonth() + 1).padStart(2, '0')
    const day = String(now.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}`
  }

  private formatLog(level: LogLevel, message: string, args: unknown[] = []): LogData {
    const timestamp = this.formatTimestamp()
    const levelName = LEVEL_NAMES[level]

    return {
      timestamp,
      level: levelName,
      levelValue: level,
      date: this.formatDate(),
      message,
      args: args.length > 0 ? args : undefined,
      stack:
        level >= LogLevel.ERROR && args[0] instanceof Error ? (args[0] as Error).stack : undefined,
    }
  }

  private outputToConsole(logData: LogData): void {
    if (!this.enableConsole) return

    const { timestamp, levelValue, message, args } = logData
    const levelName = LEVEL_NAMES[levelValue]
    const color = LEVEL_COLORS[levelValue]

    const style = `color: ${color}; font-weight: bold;`
    const prefix = `%c[${timestamp}] [${levelName}]`

    const consoleMethod =
      levelValue === LogLevel.ERROR
        ? console.error
        : levelValue === LogLevel.WARN
          ? console.warn
          : levelValue === LogLevel.DEBUG
            ? console.debug
            : console.log

    if (args && args.length > 0) {
      consoleMethod(prefix, style, message, ...args)
    } else {
      consoleMethod(prefix, style, message)
    }
  }

  private async outputToFile(logData: LogData): Promise<void> {
    if (!this.enableFile) return

    try {
      // 动态导入: 非 Tauri 环境下没有 invoke 后端, 不能顶层依赖它
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('write_log', { logData })
      this.fileWriteWarned = false
    } catch (error) {
      // 落盘失败只告警一次(如参数不匹配, 磁盘不可写), 避免日志系统自身循环报错刷屏
      if (!this.fileWriteWarned) {
        this.fileWriteWarned = true
        console.warn('[logger] 日志落盘失败,后续同类错误不再提示:', error)
      }
    }
  }

  private recordHistory(logData: LogData): void {
    this.logHistory.push(logData)

    if (this.logHistory.length > this.maxHistorySize) {
      this.logHistory.shift()
    }
  }

  log(level: LogLevel, message: string, ...args: unknown[]): void {
    if (level < this.minLevel) {
      return
    }

    const logData = this.formatLog(level, message, args)
    this.recordHistory(logData)
    this.outputToConsole(logData)

    this.outputToFile(logData).catch(() => {
      // 落盘错误已在 outputToFile 内去重告警
    })
  }

  debug(message: string, ...args: unknown[]): void {
    this.log(LogLevel.DEBUG, message, ...args)
  }

  info(message: string, ...args: unknown[]): void {
    this.log(LogLevel.INFO, message, ...args)
  }

  warn(message: string, ...args: unknown[]): void {
    this.log(LogLevel.WARN, message, ...args)
  }

  error(message: string, ...args: unknown[]): void {
    this.log(LogLevel.ERROR, message, ...args)
  }

  getHistory(limit: number | null = null): LogData[] {
    if (limit === null) {
      return [...this.logHistory]
    }
    return this.logHistory.slice(-limit)
  }

  clearHistory(): void {
    this.logHistory = []
  }

  exportHistoryAsText(): string {
    return this.logHistory
      .map((log) => {
        const { timestamp, level, message, args, stack } = log
        let text = `[${timestamp}] [${level}] ${message}`

        if (args && args.length > 0) {
          text +=
            ' ' +
            args
              .map((arg) => {
                if (arg instanceof Error) {
                  return arg.toString()
                }
                try {
                  return JSON.stringify(arg)
                } catch {
                  return String(arg)
                }
              })
              .join(' ')
        }

        if (stack) {
          text += '\n' + stack
        }

        return text
      })
      .join('\n')
  }
}

const logger = new Logger()

export default logger
export { Logger }
