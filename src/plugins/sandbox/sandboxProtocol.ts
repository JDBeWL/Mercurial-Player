/**
 * Worker 沙箱通信协议
 *
 * 主窗口 (workerSandboxHost) 与插件 Worker (workerCore) 之间的消息契约。
 * 仅包含类型与纯序列化函数,两侧共用,不得引入任何运行时依赖。
 */

import type { PlayerState, Playlist, ThemeInfo, Track } from '../pluginTypes'

/** 参数中的函数被替换为该标记对象 (值为主窗口侧回调句柄 id) */
export const SANDBOX_FN_MARKER = '__sandboxFn'

/** network.fetch 返回值序列化标记 (Response 无法结构化克隆) */
export const SANDBOX_RESPONSE_MARKER = '__sandboxResponse'

/** 可结构化克隆的错误表示 */
export interface SerializedError {
  name: string
  message: string
  stack?: string
}

export function serializeError(error: unknown): SerializedError {
  if (error instanceof Error) {
    return { name: error.name, message: error.message, stack: error.stack }
  }
  return { name: 'Error', message: String(error) }
}

export function deserializeError(error: SerializedError): Error {
  const restored = new Error(error.message)
  restored.name = error.name
  if (error.stack) restored.stack = error.stack
  return restored
}

/**
 * 镜像状态:主窗口侧定期推送的快照数据。
 * Worker 侧同步读 API (player.getState / storage.get / library.* / theme.*) 从中取值,
 * 使沙箱插件无需异步化即可保持原有 API 契约。
 */
export interface MirrorData {
  playerState: PlayerState
  currentLyricIndex: number
  theme: ThemeInfo
  /** 驼峰键的颜色表 (与 api.theme.getAllColors 一致);getCSSVariable 反查 */
  colors: Record<string, string>
  /** null 表示插件无 storage 权限 */
  storage: Record<string, unknown> | null
  playlists: Playlist[]
  currentPlaylist: Playlist | null
  tracks: Track[]
}

// ---------- 主窗口 → Worker ----------

export type HostToWorkerMessage =
  | { type: 'init'; pluginId: string; permissions: string[]; code: string }
  | { type: 'run-main' }
  | { type: 'mirror'; data: Partial<MirrorData> }
  | { type: 'api-result'; callId: number; ok: true; value: unknown }
  | { type: 'api-result'; callId: number; ok: false; error: SerializedError }
  /** callId: 主窗口侧挂起调用 id (回执关联);cbId: Worker 侧回调句柄 id */
  | { type: 'callback-call'; callId: number; cbId: number; args: unknown[] }

// ---------- Worker → 主窗口 ----------

export type WorkerToHostMessage =
  | { type: 'init-result'; ok: true }
  | { type: 'init-result'; ok: false; error: SerializedError }
  | { type: 'main-result'; ok: true; instance: unknown }
  | { type: 'main-result'; ok: false; error: SerializedError }
  | { type: 'api-call'; callId: number; path: string; args: unknown[] }
  | { type: 'callback-result'; callId: number; ok: true; value: unknown }
  | { type: 'callback-result'; callId: number; ok: false; error: SerializedError }
  | { type: 'log'; level: 'info' | 'warn' | 'error' | 'debug'; args: unknown[] }
