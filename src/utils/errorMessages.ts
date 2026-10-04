import logger from './logger'

/** 从 unknown 错误中取可读消息：Error 实例用其 message，否则用 fallback */
export const getErrorMessage = (err: unknown, fallback: string): string => {
  return err instanceof Error ? err.message : fallback
}

/** saveConfigNow 的统一封装：失败只记日志、不向上抛出（设置页控件变更的静默持久化） */
export const saveConfigSafely = async (configStore: {
  saveConfigNow: () => Promise<void>
}): Promise<void> => {
  try {
    await configStore.saveConfigNow()
  } catch (error) {
    logger.error('Failed to save config:', error)
  }
}
