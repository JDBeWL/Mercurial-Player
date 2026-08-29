import logger from './logger'

/**
 * 从未知类型的错误对象中提取可读消息。
 * err 为 Error 实例时返回其 message，否则返回 fallback。
 */
export const getErrorMessage = (err: unknown, fallback: string): string => {
  return err instanceof Error ? err.message : fallback
}

/**
 * 保存配置的统一封装：调用 saveConfigNow，失败时记录日志但不向上抛出。
 * 用于设置页各控件变更后的静默持久化。
 */
export const saveConfigSafely = async (configStore: {
  saveConfigNow: () => Promise<void>
}): Promise<void> => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}
