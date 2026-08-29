/**
 * 统一错误处理系统
 *
 * 提供统一的错误处理机制，包括错误分类、日志记录和用户提示
 */

import logger from './logger'
import type { ErrorContext, ErrorHandlerOptions, HandleResult } from '@/types'

/**
 * 翻译函数注入点:由应用装配层(src/i18n.ts)注入 i18n.global.t,
 * 避免本工具模块反向依赖 app 装配层,也便于单元测试注入假翻译。
 * 未注入时回退返回 key 本身。
 */
type Translator = (key: string) => string
let translator: Translator | null = null

export function setErrorHandlerTranslator(t: Translator): void {
  translator = t
}

function translate(key: string): string {
  return translator ? translator(key) : key
}

/**
 * 错误类型枚举
 */
export enum ErrorType {
  // 网络相关错误
  NETWORK = 'NETWORK',
  NETWORK_TIMEOUT = 'NETWORK_TIMEOUT',
  NETWORK_OFFLINE = 'NETWORK_OFFLINE',

  // 文件系统相关错误
  FILE_NOT_FOUND = 'FILE_NOT_FOUND',
  FILE_READ_ERROR = 'FILE_READ_ERROR',
  FILE_WRITE_ERROR = 'FILE_WRITE_ERROR',
  FILE_PERMISSION_DENIED = 'FILE_PERMISSION_DENIED',

  // 音频相关错误
  AUDIO_DECODE_ERROR = 'AUDIO_DECODE_ERROR',
  AUDIO_PLAYBACK_ERROR = 'AUDIO_PLAYBACK_ERROR',
  AUDIO_DEVICE_ERROR = 'AUDIO_DEVICE_ERROR',

  // 配置相关错误
  CONFIG_LOAD_ERROR = 'CONFIG_LOAD_ERROR',
  CONFIG_SAVE_ERROR = 'CONFIG_SAVE_ERROR',
  CONFIG_INVALID = 'CONFIG_INVALID',

  // 数据相关错误
  DATA_PARSE_ERROR = 'DATA_PARSE_ERROR',
  DATA_VALIDATION_ERROR = 'DATA_VALIDATION_ERROR',

  // 未知错误
  UNKNOWN = 'UNKNOWN',
}

/**
 * 错误严重程度枚举
 */
export enum ErrorSeverity {
  LOW = 'LOW', // 低严重程度，可以忽略或自动恢复
  MEDIUM = 'MEDIUM', // 中等严重程度，需要用户注意
  HIGH = 'HIGH', // 高严重程度，影响功能使用
  CRITICAL = 'CRITICAL', // 严重错误，可能导致应用崩溃
}

/**
 * 应用错误类
 */
export class AppError extends Error {
  type: ErrorType
  severity: ErrorSeverity
  originalError: Error | unknown | null
  context: ErrorContext
  timestamp: string

  constructor(
    message: string,
    type: ErrorType = ErrorType.UNKNOWN,
    severity: ErrorSeverity = ErrorSeverity.MEDIUM,
    originalError: Error | unknown | null = null,
    context: ErrorContext = {},
  ) {
    super(message)
    this.name = 'AppError'
    this.type = type
    this.severity = severity
    this.originalError = originalError
    this.context = context
    this.timestamp = new Date().toISOString()

    // 保持堆栈跟踪（V8引擎特有API）
    const ErrorWithCapture = Error as {
      captureStackTrace?: (target: object, constructor: object) => void
    }
    if (ErrorWithCapture.captureStackTrace) {
      ErrorWithCapture.captureStackTrace(this, AppError)
    }
  }

  /**
   * 转换为可序列化的对象
   */
  toJSON(): object {
    return {
      name: this.name,
      message: this.message,
      type: this.type,
      severity: this.severity,
      timestamp: this.timestamp,
      context: this.context,
      stack: this.stack,
      originalError:
        this.originalError instanceof Error
          ? {
              name: this.originalError.name,
              message: this.originalError.message,
              stack: this.originalError.stack,
            }
          : this.originalError,
    }
  }
}

type ErrorListener = (
  error: AppError,
  options: { showToUser: boolean; userMessage: string },
) => void

/**
 * 错误处理器类
 */
class ErrorHandler {
  // 错误监听器列表
  private listeners: ErrorListener[] = []

  /**
   * 注册错误监听器
   */
  onError(listener: ErrorListener): () => void {
    this.listeners.push(listener)
    return () => {
      const index = this.listeners.indexOf(listener)
      if (index > -1) {
        this.listeners.splice(index, 1)
      }
    }
  }

  /**
   * 处理错误
   */
  handle(error: Error | AppError | unknown, options: ErrorHandlerOptions = {}): AppError {
    const {
      type = ErrorType.UNKNOWN,
      severity = ErrorSeverity.MEDIUM,
      context = {},
      silent = false,
      showToUser = true,
      userMessage = null,
    } = options

    // 转换为 AppError
    let appError: AppError
    if (error instanceof AppError) {
      appError = error
      // 合并上下文
      appError.context = { ...appError.context, ...context }
    } else if (error instanceof Error) {
      appError = new AppError(
        error.message || translate('errors.unknownError'),
        type,
        severity,
        error,
        context,
      )
    } else {
      appError = new AppError(
        String(error) || translate('errors.unknownError'),
        type,
        severity,
        error,
        context,
      )
    }

    // 记录错误
    if (!silent) {
      this.logError(appError)
    }

    // 通知监听器
    this.notifyListeners(appError, { showToUser, userMessage })

    return appError
  }

  /**
   * 记录错误日志
   */
  private logError(error: AppError): void {
    const logMessage = `[${error.type}] ${error.message}`
    const logContext = {
      severity: error.severity,
      context: error.context,
      originalError: error.originalError,
    }

    // 根据严重程度选择日志级别
    switch (error.severity) {
      case ErrorSeverity.CRITICAL:
      case ErrorSeverity.HIGH:
        logger.error(logMessage, logContext, error.originalError)
        break
      case ErrorSeverity.MEDIUM:
        logger.warn(logMessage, logContext)
        break
      case ErrorSeverity.LOW:
        logger.debug(logMessage, logContext)
        break
      default:
        logger.error(logMessage, logContext)
    }
  }

  /**
   * 通知错误监听器
   */
  private notifyListeners(
    error: AppError,
    options: { showToUser: boolean; userMessage: string | null },
  ): void {
    const { showToUser = true, userMessage = null } = options

    this.listeners.forEach((listener) => {
      try {
        listener(error, {
          showToUser,
          userMessage: userMessage || this.getUserFriendlyMessage(error),
        })
      } catch (listenerError) {
        // 避免监听器错误导致循环
        logger.error('Error in error listener:', listenerError)
      }
    })
  }

  /**
   * 获取用户友好的错误消息
   */
  getUserFriendlyMessage(error: AppError): string {
    // 根据错误类型返回友好的消息
    const messages: Record<ErrorType, string> = {
      [ErrorType.NETWORK]: translate('errors.network'),
      [ErrorType.NETWORK_TIMEOUT]: translate('errors.networkTimeout'),
      [ErrorType.NETWORK_OFFLINE]: translate('errors.networkOffline'),
      [ErrorType.FILE_NOT_FOUND]: translate('errors.fileNotFound'),
      [ErrorType.FILE_READ_ERROR]: translate('errors.fileReadError'),
      [ErrorType.FILE_WRITE_ERROR]: translate('errors.fileWriteError'),
      [ErrorType.FILE_PERMISSION_DENIED]: translate('errors.filePermissionDenied'),
      [ErrorType.AUDIO_DECODE_ERROR]: translate('errors.audioDecodeError'),
      [ErrorType.AUDIO_PLAYBACK_ERROR]: translate('errors.audioPlaybackError'),
      [ErrorType.AUDIO_DEVICE_ERROR]: translate('errors.audioDeviceError'),
      [ErrorType.CONFIG_LOAD_ERROR]: translate('errors.configLoadError'),
      [ErrorType.CONFIG_SAVE_ERROR]: translate('errors.configSaveError'),
      [ErrorType.CONFIG_INVALID]: translate('errors.configInvalid'),
      [ErrorType.DATA_PARSE_ERROR]: translate('errors.dataParseError'),
      [ErrorType.DATA_VALIDATION_ERROR]: translate('errors.dataValidationError'),
      [ErrorType.UNKNOWN]: translate('errors.genericError'),
    }

    return messages[error.type] || error.message || translate('errors.genericError')
  }
}

// 创建全局错误处理器实例
const errorHandler = new ErrorHandler()

/**
 * Promise 错误处理包装器
 */
export async function handlePromise<T>(
  promise: Promise<T>,
  options: ErrorHandlerOptions = {},
): Promise<HandleResult<T>> {
  try {
    const result = await promise
    return {
      success: true,
      data: result,
      error: null,
    }
  } catch (error) {
    const handledError = errorHandler.handle(error, options)
    return {
      success: false,
      data: null,
      error: handledError,
    }
  }
}

// 导出错误处理器实例和工具函数
export default errorHandler
export { ErrorHandler }
