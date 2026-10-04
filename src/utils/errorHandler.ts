/** 统一错误处理：错误分类、日志记录与用户提示 */

import logger from './logger'
import type { ErrorContext, ErrorHandlerOptions, HandleResult } from '@/types'

/**
 * 翻译注入点：由应用装配层(src/i18n.ts)注入 i18n.global.t，避免本模块反向依赖 app 装配层，并便于单测注入假翻译。
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

  UNKNOWN = 'UNKNOWN',
}

export enum ErrorSeverity {
  LOW = 'LOW', // 可忽略或自动恢复
  MEDIUM = 'MEDIUM', // 需用户注意
  HIGH = 'HIGH', // 影响功能使用
  CRITICAL = 'CRITICAL', // 可能导致应用崩溃
}

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

    // captureStackTrace 仅 V8 引擎提供，需按存在性守卫
    const ErrorWithCapture = Error as {
      captureStackTrace?: (target: object, constructor: object) => void
    }
    if (ErrorWithCapture.captureStackTrace) {
      ErrorWithCapture.captureStackTrace(this, AppError)
    }
  }

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

class ErrorHandler {
  private listeners: ErrorListener[] = []

  onError(listener: ErrorListener): () => void {
    this.listeners.push(listener)
    return () => {
      const index = this.listeners.indexOf(listener)
      if (index > -1) {
        this.listeners.splice(index, 1)
      }
    }
  }

  handle(error: Error | AppError | unknown, options: ErrorHandlerOptions = {}): AppError {
    const {
      type = ErrorType.UNKNOWN,
      severity = ErrorSeverity.MEDIUM,
      context = {},
      silent = false,
      showToUser = true,
      userMessage = null,
    } = options

    let appError: AppError
    if (error instanceof AppError) {
      appError = error
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

    if (!silent) {
      this.logError(appError)
    }

    this.notifyListeners(appError, { showToUser, userMessage })

    return appError
  }

  private logError(error: AppError): void {
    const logMessage = `[${error.type}] ${error.message}`
    const logContext = {
      severity: error.severity,
      context: error.context,
      originalError: error.originalError,
    }

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

  getUserFriendlyMessage(error: AppError): string {
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

const errorHandler = new ErrorHandler()

/**
 * Promise 错误处理包装器。
 *
 * options.throw 为 true 时记录并通知后重新抛出 AppError；默认返回 { success: false }。
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
    if (options.throw) {
      throw handledError
    }
    return {
      success: false,
      data: null,
      error: handledError,
    }
  }
}

export default errorHandler
export { ErrorHandler }
