/**
 * 插件沙箱 Worker 入口:本文件只做接线 (原生网络 API 移除 + 消息桥 + 未捕获拒绝转发),
 * 协议实现见 workerCore.ts;blob 内联 Worker 继承主文档 CSP 的安全考量见 workerSandboxHost 的 defaultWorkerFactory。
 */

import { SandboxWorkerRuntime, removeNetworkGlobals } from './workerCore'
import type { WorkerToHostMessage } from './sandboxProtocol'

// 必须在全局中和之前捕获原生引用:插件与 runtime 共享 realm,留在全局作用域的引用皆可被夺取
// (伪造 api-call 协议消息,或经 addEventListener 窃听宿主下行),运行时通道因此等效于私有 MessagePort。
// 宿主侧另有 API_CALL_POLICY 白名单校验,此处为纵深防御。
const nativePostMessage = self.postMessage.bind(self)
const nativeAddEventListener = self.addEventListener.bind(self)

// 中和 workerCore 的 SANDBOX_BLOCKED_GLOBAL_KEYS 与 navigator.sendBeacon,插件网络访问只能经 api.network.fetch
removeNetworkGlobals(self as unknown as Record<string, unknown>)

const post = (msg: WorkerToHostMessage): void => {
  try {
    nativePostMessage(msg)
  } catch {
    // 不可克隆消息静默丢弃:workerCore 发送前已消毒,此处兜底避免中断 Worker 消息循环
  }
}

const runtime = new SandboxWorkerRuntime(post)

self.onmessage = (event: MessageEvent): void => {
  const data = event.data
  if (data && typeof data === 'object' && typeof (data as { type?: unknown }).type === 'string') {
    void runtime.handleMessage(data as Parameters<typeof runtime.handleMessage>[0])
  }
}

// Worker 内未捕获的 Promise 拒绝转发主窗口落盘 (生产构建 drop console)
nativeAddEventListener('unhandledrejection', (event) => {
  const reason = (event as PromiseRejectionEvent).reason
  runtime.reportUnhandledRejection(reason)
})
