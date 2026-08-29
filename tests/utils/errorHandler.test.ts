import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  ErrorType,
  ErrorSeverity,
  AppError,
  ErrorHandler,
  handlePromise,
  setErrorHandlerTranslator,
} from '@/utils/errorHandler'

describe('ErrorHandler', () => {
  let handler: ErrorHandler

  beforeEach(() => {
    handler = new ErrorHandler()
    // errorHandler 已与 i18n 装配层解耦,测试注入最小假翻译验证消息映射
    setErrorHandlerTranslator((key) => {
      const messages: Record<string, string> = {
        'errors.network': '网络错误',
        'errors.fileNotFound': '文件未找到',
        'errors.genericError': '发生错误',
      }
      return messages[key] ?? key
    })
  })

  describe('AppError', () => {
    it('should create AppError with defaults', () => {
      const error = new AppError('test message')

      expect(error.message).toBe('test message')
      expect(error.type).toBe(ErrorType.UNKNOWN)
      expect(error.severity).toBe(ErrorSeverity.MEDIUM)
      expect(error.name).toBe('AppError')
    })

    it('should create AppError with custom values', () => {
      const original = new Error('original')
      const error = new AppError(
        'custom message',
        ErrorType.FILE_NOT_FOUND,
        ErrorSeverity.HIGH,
        original,
        { path: '/test' },
      )

      expect(error.type).toBe(ErrorType.FILE_NOT_FOUND)
      expect(error.severity).toBe(ErrorSeverity.HIGH)
      expect(error.originalError).toBe(original)
      expect(error.context.path).toBe('/test')
    })

    it('should serialize to JSON', () => {
      const error = new AppError('test', ErrorType.NETWORK)
      const json = error.toJSON()

      expect(json).toHaveProperty('name', 'AppError')
      expect(json).toHaveProperty('message', 'test')
      expect(json).toHaveProperty('type', ErrorType.NETWORK)
      expect(json).toHaveProperty('timestamp')
    })

    it('should serialize original error in JSON', () => {
      const original = new Error('original error')
      const error = new AppError('wrapper', ErrorType.UNKNOWN, ErrorSeverity.MEDIUM, original)
      const json = error.toJSON() as unknown as Record<string, unknown>

      expect(json.originalError).toHaveProperty('name', 'Error')
      expect(json.originalError).toHaveProperty('message', 'original error')
    })
  })

  describe('handle', () => {
    it('should handle Error objects', () => {
      const error = new Error('test error')
      const result = handler.handle(error, { silent: true })

      expect(result).toBeInstanceOf(AppError)
      expect(result.message).toBe('test error')
    })

    it('should handle AppError objects', () => {
      const appError = new AppError('app error', ErrorType.FILE_READ_ERROR)
      const result = handler.handle(appError, { silent: true })

      expect(result.type).toBe(ErrorType.FILE_READ_ERROR)
    })

    it('should handle string errors', () => {
      const result = handler.handle('string error', { silent: true })

      expect(result.message).toBe('string error')
    })

    it('should merge context', () => {
      const appError = new AppError('test', ErrorType.UNKNOWN, ErrorSeverity.MEDIUM, null, { a: 1 })
      const result = handler.handle(appError, { context: { b: 2 }, silent: true })

      expect(result.context).toEqual({ a: 1, b: 2 })
    })

    it('should use provided type and severity', () => {
      const error = new Error('test')
      const result = handler.handle(error, {
        type: ErrorType.AUDIO_DECODE_ERROR,
        severity: ErrorSeverity.CRITICAL,
        silent: true,
      })

      expect(result.type).toBe(ErrorType.AUDIO_DECODE_ERROR)
      expect(result.severity).toBe(ErrorSeverity.CRITICAL)
    })
  })

  describe('error listeners', () => {
    it('should notify listeners on error', () => {
      const listener = vi.fn()
      handler.onError(listener)

      handler.handle(new Error('test'), { silent: true })

      expect(listener).toHaveBeenCalled()
    })

    it('should unsubscribe listener', () => {
      const listener = vi.fn()
      const unsubscribe = handler.onError(listener)

      unsubscribe()
      handler.handle(new Error('test'), { silent: true })

      expect(listener).not.toHaveBeenCalled()
    })

    it('should pass showToUser and userMessage to listener', () => {
      const listener = vi.fn()
      handler.onError(listener)

      handler.handle(new Error('test'), {
        silent: true,
        showToUser: false,
        userMessage: 'Custom message',
      })

      expect(listener).toHaveBeenCalledWith(
        expect.any(AppError),
        expect.objectContaining({
          showToUser: false,
          userMessage: 'Custom message',
        }),
      )
    })
  })

  describe('getUserFriendlyMessage', () => {
    it('should return friendly message for known error types', () => {
      const error = new AppError('test', ErrorType.NETWORK)
      const message = handler.getUserFriendlyMessage(error)

      expect(message).toContain('网络')
    })

    it('should return friendly message for file errors', () => {
      const error = new AppError('test', ErrorType.FILE_NOT_FOUND)
      const message = handler.getUserFriendlyMessage(error)

      expect(message).toContain('文件')
    })

    it('should return default message for unknown type', () => {
      const error = new AppError('custom message', ErrorType.UNKNOWN)
      const message = handler.getUserFriendlyMessage(error)

      expect(message).toBeTruthy()
    })
  })
})

describe('handlePromise', () => {
  it('should return success result for resolved promise', async () => {
    const result = await handlePromise(Promise.resolve('data'))

    expect(result.success).toBe(true)
    expect(result.data).toBe('data')
    expect(result.error).toBeNull()
  })

  it('should return error result for rejected promise', async () => {
    const result = await handlePromise(Promise.reject(new Error('failed')))

    expect(result.success).toBe(false)
    expect(result.data).toBeNull()
    expect(result.error).toBeInstanceOf(AppError)
  })
})

describe('ErrorType enum', () => {
  it('should have all expected error types', () => {
    expect(ErrorType.NETWORK).toBeDefined()
    expect(ErrorType.FILE_NOT_FOUND).toBeDefined()
    expect(ErrorType.AUDIO_DECODE_ERROR).toBeDefined()
    expect(ErrorType.CONFIG_LOAD_ERROR).toBeDefined()
    expect(ErrorType.UNKNOWN).toBeDefined()
  })
})

describe('ErrorSeverity enum', () => {
  it('should have all severity levels', () => {
    expect(ErrorSeverity.LOW).toBeDefined()
    expect(ErrorSeverity.MEDIUM).toBeDefined()
    expect(ErrorSeverity.HIGH).toBeDefined()
    expect(ErrorSeverity.CRITICAL).toBeDefined()
  })
})
