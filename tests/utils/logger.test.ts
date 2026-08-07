import { describe, it, expect, beforeEach, vi } from 'vitest'
import { Logger, LogLevel } from '@/utils/logger'

describe('Logger', () => {
  let logger: Logger

  beforeEach(() => {
    logger = new Logger()
    // Disable console output during tests
    logger.setConsoleEnabled(false)
  })

  describe('log levels', () => {
    it('should log when level meets minimum', () => {
      logger.setMinLevel(LogLevel.DEBUG)
      logger.debug('test message')

      const history = logger.getHistory()
      expect(history).toHaveLength(1)
      expect(history[0].message).toBe('test message')
    })

    it('should not log when level is below minimum', () => {
      logger.setMinLevel(LogLevel.ERROR)
      logger.debug('should not appear')
      logger.info('should not appear')
      logger.warn('should not appear')

      expect(logger.getHistory()).toHaveLength(0)
    })

    it('should log error when minimum is ERROR', () => {
      logger.setMinLevel(LogLevel.ERROR)
      logger.error('error message')

      expect(logger.getHistory()).toHaveLength(1)
    })
  })

  describe('log methods', () => {
    beforeEach(() => {
      logger.setMinLevel(LogLevel.DEBUG)
    })

    it('should log debug messages', () => {
      logger.debug('debug test')
      const history = logger.getHistory()

      expect(history[0].level).toBe('DEBUG')
    })

    it('should log info messages', () => {
      logger.info('info test')
      const history = logger.getHistory()

      expect(history[0].level).toBe('INFO')
    })

    it('should log warn messages', () => {
      logger.warn('warn test')
      const history = logger.getHistory()

      expect(history[0].level).toBe('WARN')
    })

    it('should log error messages', () => {
      logger.error('error test')
      const history = logger.getHistory()

      expect(history[0].level).toBe('ERROR')
    })

    it('should include additional arguments', () => {
      logger.info('message', { key: 'value' }, 123)
      const history = logger.getHistory()

      expect(history[0].args).toHaveLength(2)
      expect(history[0].args![0]).toEqual({ key: 'value' })
      expect(history[0].args![1]).toBe(123)
    })

    it('should capture error stack for ERROR level', () => {
      const error = new Error('test error')
      logger.error('error occurred', error)
      const history = logger.getHistory()

      expect(history[0].stack).toBeDefined()
    })
  })

  describe('history management', () => {
    beforeEach(() => {
      logger.setMinLevel(LogLevel.DEBUG)
    })

    it('should return all history', () => {
      logger.info('one')
      logger.info('two')
      logger.info('three')

      expect(logger.getHistory()).toHaveLength(3)
    })

    it('should return limited history', () => {
      logger.info('one')
      logger.info('two')
      logger.info('three')

      const limited = logger.getHistory(2)
      expect(limited).toHaveLength(2)
      expect(limited[0].message).toBe('two')
      expect(limited[1].message).toBe('three')
    })

    it('should clear history', () => {
      logger.info('test')
      logger.clearHistory()

      expect(logger.getHistory()).toHaveLength(0)
    })

    it('should limit history size', () => {
      // Logger has maxHistorySize of 100
      for (let i = 0; i < 110; i++) {
        logger.info(`message ${i}`)
      }

      const history = logger.getHistory()
      expect(history.length).toBeLessThanOrEqual(100)
    })
  })

  describe('exportHistoryAsText', () => {
    beforeEach(() => {
      logger.setMinLevel(LogLevel.DEBUG)
    })

    it('should export history as text', () => {
      logger.info('test message')
      const text = logger.exportHistoryAsText()

      expect(text).toContain('[INFO]')
      expect(text).toContain('test message')
    })

    it('should include arguments in export', () => {
      logger.info('message', { data: 'value' })
      const text = logger.exportHistoryAsText()

      expect(text).toContain('data')
      expect(text).toContain('value')
    })

    it('should handle Error objects in arguments', () => {
      const error = new Error('test error')
      logger.error('failed', error)
      const text = logger.exportHistoryAsText()

      expect(text).toContain('test error')
    })

    it('should handle non-serializable objects', () => {
      const circular: Record<string, unknown> = { a: 1 }
      circular.self = circular

      logger.info('circular', circular)
      const text = logger.exportHistoryAsText()

      // Should not throw, should convert to string
      expect(text).toContain('circular')
    })
  })

  describe('configuration', () => {
    it('should enable/disable console output', () => {
      const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {})

      logger.setMinLevel(LogLevel.INFO)
      logger.setConsoleEnabled(true)
      logger.info('visible')

      expect(consoleSpy).toHaveBeenCalled()

      consoleSpy.mockClear()
      logger.setConsoleEnabled(false)
      logger.info('invisible')

      expect(consoleSpy).not.toHaveBeenCalled()

      consoleSpy.mockRestore()
    })

    it('should enable/disable file output', () => {
      logger.setFileEnabled(true)
      logger.setFileEnabled(false)
      // Just verify no errors are thrown
    })
  })

  describe('timestamp formatting', () => {
    it('should include timestamp in log', () => {
      logger.setMinLevel(LogLevel.DEBUG)
      logger.info('test')
      const history = logger.getHistory()

      expect(history[0].timestamp).toMatch(/^\d{2}:\d{2}:\d{2}\.\d{3}$/)
    })
  })
})
