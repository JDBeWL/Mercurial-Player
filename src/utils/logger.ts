/**
 * 日志系统
 *
 * 提供统一的日志管理，支持不同日志级别和环境配置
 * 在生产环境自动禁用调试日志
 */

import type { LogData } from '@/types'

// 日志级别枚举
export enum LogLevel {
  DEBUG = 0,
  INFO = 1,
  WARN = 2,
  ERROR = 3,
  NONE = 4,
}

// 日志级别名称映射
const LEVEL_NAMES: Record<LogLevel, string> = {
  [LogLevel.DEBUG]: 'DEBUG',
  [LogLevel.INFO]: 'INFO',
  [LogLevel.WARN]: 'WARN',
  [LogLevel.ERROR]: 'ERROR',
  [LogLevel.NONE]: 'NONE',
}

// 日志等级持久化 key(开发者选项中设置,跨启动生效)
const LOG_LEVEL_STORAGE_KEY = 'mercurial-player.log-level'

// 日志级别颜色映射（用于控制台输出）
const LEVEL_COLORS: Record<LogLevel, string> = {
  [LogLevel.DEBUG]: '#888',
  [LogLevel.INFO]: '#2196F3',
  [LogLevel.WARN]: '#FF9800',
  [LogLevel.ERROR]: '#F44336',
  [LogLevel.NONE]: '#000',
}

/**
 * 日志系统类
 */
class Logger {
  private isDev: boolean
  private isDebug: boolean
  private minLevel: LogLevel
  private enableConsole: boolean
  private enableFile: boolean
  /** 落盘失败是否已告警过(只告警一次,避免循环刷屏) */
  private fileWriteWarned: boolean
  private logHistory: LogData[]
  private maxHistorySize: number

  constructor() {
    // 获取环境变量
    this.isDev = import.meta.env.DEV
    this.isDebug = import.meta.env.MODE === 'development' || import.meta.env.DEBUG === 'true'

    // 根据环境设置默认日志级别(开发者选项持久化的等级优先)
    this.minLevel = this.resolveInitialLevel()

    // 是否启用控制台输出
    this.enableConsole = true

    // 是否启用文件输出（通过Tauri后端写入日志目录,
    // 每次启动轮转:上一次运行的 mercurial-player.log → mercurial-player-prev.log）
    this.enableFile = true
    this.fileWriteWarned = false

    // 日志历史（用于调试）
    this.logHistory = []
    this.maxHistorySize = 100
  }

  /**
   * 解析初始日志级别:优先读取开发者选项中持久化的等级,否则按环境默认
   */
  private resolveInitialLevel(): LogLevel {
    try {
      const saved = localStorage.getItem(LOG_LEVEL_STORAGE_KEY)
      const parsed = saved === null ? NaN : Number(saved)
      if (Object.values(LogLevel).includes(parsed as LogLevel)) {
        return parsed as LogLevel
      }
    } catch {
      // localStorage 不可用(如非浏览器环境)时忽略,回落默认值
    }
    return this.isDev || this.isDebug ? LogLevel.DEBUG : LogLevel.INFO
  }

  /**
   * 设置最小日志级别(持久化到 localStorage,跨启动生效)
   */
  setMinLevel(level: LogLevel): void {
    this.minLevel = level
    try {
      localStorage.setItem(LOG_LEVEL_STORAGE_KEY, String(level))
    } catch {
      // 持久化失败不影响本次会话的级别生效
    }
  }

  /**
   * 获取当前最小日志级别
   */
  getMinLevel(): LogLevel {
    return this.minLevel
  }

  /**
   * 启用/禁用控制台输出
   */
  setConsoleEnabled(enable: boolean): void {
    this.enableConsole = enable
  }

  /**
   * 启用/禁用文件输出
   */
  setFileEnabled(enable: boolean): void {
    this.enableFile = enable
  }

  /**
   * 格式化时间戳
   */
  private formatTimestamp(): string {
    const now = new Date()
    const hours = String(now.getHours()).padStart(2, '0')
    const minutes = String(now.getMinutes()).padStart(2, '0')
    const seconds = String(now.getSeconds()).padStart(2, '0')
    const milliseconds = String(now.getMilliseconds()).padStart(3, '0')
    return `${hours}:${minutes}:${seconds}.${milliseconds}`
  }

  /**
   * 格式化日期（写入日志文件时用于跨天区分,控制台输出不展示）
   */
  private formatDate(): string {
    const now = new Date()
    const year = now.getFullYear()
    const month = String(now.getMonth() + 1).padStart(2, '0')
    const day = String(now.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}`
  }

  /**
   * 格式化日志消息
   */
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

  /**
   * 输出日志到控制台
   */
  private outputToConsole(logData: LogData): void {
    if (!this.enableConsole) return

    const { timestamp, levelValue, message, args } = logData
    const levelName = LEVEL_NAMES[levelValue]
    const color = LEVEL_COLORS[levelValue]

    // 构建控制台输出样式
    const style = `color: ${color}; font-weight: bold;`
    const prefix = `%c[${timestamp}] [${levelName}]`

    // 根据日志级别选择不同的控制台方法
    const consoleMethod =
      levelValue === LogLevel.ERROR
        ? console.error
        : levelValue === LogLevel.WARN
          ? console.warn
          : levelValue === LogLevel.DEBUG
            ? console.debug
            : console.log

    // 输出日志
    if (args && args.length > 0) {
      consoleMethod(prefix, style, message, ...args)
    } else {
      consoleMethod(prefix, style, message)
    }
  }

  /**
   * 输出日志到文件（通过Tauri后端）
   */
  private async outputToFile(logData: LogData): Promise<void> {
    if (!this.enableFile) return

    try {
      // 动态导入以避免在非Tauri环境中出错
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('write_log', { logData })
      this.fileWriteWarned = false
    } catch (error) {
      // 落盘失败只告警一次(如参数不匹配、磁盘不可写),避免日志系统循环报错刷屏
      if (!this.fileWriteWarned) {
        this.fileWriteWarned = true
        console.warn('[logger] 日志落盘失败,后续同类错误不再提示:', error)
      }
    }
  }

  /**
   * 记录日志历史
   */
  private recordHistory(logData: LogData): void {
    this.logHistory.push(logData)

    // 限制历史记录大小
    if (this.logHistory.length > this.maxHistorySize) {
      this.logHistory.shift()
    }
  }

  /**
   * 核心日志方法
   */
  log(level: LogLevel, message: string, ...args: unknown[]): void {
    // 检查日志级别
    if (level < this.minLevel) {
      return
    }

    // 格式化日志
    const logData = this.formatLog(level, message, args)

    // 记录到历史
    this.recordHistory(logData)

    // 输出到控制台
    this.outputToConsole(logData)

    // 输出到文件（异步，不阻塞）
    this.outputToFile(logData).catch(() => {
      // 静默处理文件输出错误
    })
  }

  /**
   * 调试日志
   */
  debug(message: string, ...args: unknown[]): void {
    this.log(LogLevel.DEBUG, message, ...args)
  }

  /**
   * 信息日志
   */
  info(message: string, ...args: unknown[]): void {
    this.log(LogLevel.INFO, message, ...args)
  }

  /**
   * 警告日志
   */
  warn(message: string, ...args: unknown[]): void {
    this.log(LogLevel.WARN, message, ...args)
  }

  /**
   * 错误日志
   */
  error(message: string, ...args: unknown[]): void {
    this.log(LogLevel.ERROR, message, ...args)
  }

  /**
   * 获取日志历史
   */
  getHistory(limit: number | null = null): LogData[] {
    if (limit === null) {
      return [...this.logHistory]
    }
    return this.logHistory.slice(-limit)
  }

  /**
   * 清空日志历史
   */
  clearHistory(): void {
    this.logHistory = []
  }

  /**
   * 导出日志历史为文本
   */
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

// 创建全局日志实例
const logger = new Logger()

// 导出日志实例和日志级别
export default logger
export { Logger }
