/**
 * 统一 Tauri invoke 封装
 *
 * 消除各处「invoke(...).catch(err => logger.error(...))」的样板重复：
 * 失败统一走 errorHandler（分类 + 日志 + 通知监听器），默认不上抛，
 * 避免 fire-and-forget 场景产生 unhandled rejection；需要异常流的调用点传
 * `rethrow: true`。
 */
import { invoke } from '@tauri-apps/api/core'
import errorHandler, { ErrorSeverity } from './errorHandler'

export interface SafeInvokeOptions {
  /** 错误严重程度,默认 LOW(仅 debug 日志,不打扰用户) */
  severity?: ErrorSeverity
  /** 用户可读提示文案(会透传给 error 监听器) */
  userMessage?: string
  /** 与 errorHandler.handle 的 silent 一致:为 true 时不写日志 */
  silent?: boolean
  /**
   * 记录并上报错误后是否继续抛出。默认 false(吞掉,适合 void 调用);
   * 需要按异常流处理的调用点(如播放失败要清理状态)传 true
   */
  rethrow?: boolean
}

export async function safeInvoke<T = void>(
  cmd: string,
  args?: Record<string, unknown>,
  options: SafeInvokeOptions = {},
): Promise<T> {
  try {
    // 与调用方既有惯例保持一致:无参数命令只传 cmd,便于 mock 断言单参数形态
    return args === undefined ? await invoke<T>(cmd) : await invoke<T>(cmd, args)
  } catch (err) {
    const {
      severity = ErrorSeverity.LOW,
      userMessage = undefined,
      silent = false,
      rethrow = false,
    } = options
    const handled = errorHandler.handle(err instanceof Error ? err : new Error(String(err)), {
      severity,
      silent,
      showToUser: false,
      userMessage,
    })
    if (rethrow) {
      throw handled
    }
    return undefined as T
  }
}
