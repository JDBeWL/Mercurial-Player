/**
 * Tauri invoke 统一封装: 失败经 errorHandler 分类/落日志/通知监听器后默认吞掉, 避免 fire-and-forget 调用产生 unhandled rejection。
 *
 * args 的键是 camelCase, 由 Tauri 映射到 Rust 命令的 snake_case 参数名; cmd 需与 Rust 侧命令名逐字一致。
 */
import { invoke } from '@tauri-apps/api/core'
import errorHandler, { ErrorSeverity } from './errorHandler'

export interface SafeInvokeOptions {
  /** 错误严重程度, 默认 LOW(仅 debug 日志, 不打扰用户) */
  severity?: ErrorSeverity
  /** 用户可读提示文案(会透传给 error 监听器) */
  userMessage?: string
  /** 与 errorHandler.handle 的 silent 同义: 为 true 时不写日志 */
  silent?: boolean
  /** 记录并上报后是否继续抛出, 默认吞掉; 理由见文件头 */
  rethrow?: boolean
}

export async function safeInvoke<T = void>(
  cmd: string,
  args?: Record<string, unknown>,
  options: SafeInvokeOptions = {},
): Promise<T> {
  try {
    // 与调用方既有惯例保持一致: 无参数命令只传 cmd, 便于 mock 断言单参数形态
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
