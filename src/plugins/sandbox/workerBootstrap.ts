/**
 * 插件沙箱 Worker 入口
 *
 * 由 workerSandboxHost 以 blob URL 内联 Worker 启动 (vite ?worker&inline)。
 * blob Worker 继承主文档的 CSP (script-src/connect-src 在本 Worker 内生效),
 * 阻断插件的原生网络请求与远程模块加载。本文件只做接线
 * (原生网络 API 移除 + 消息桥 + 未捕获拒绝转发),协议实现见 workerCore.ts。
 */

import { SandboxWorkerRuntime, removeNetworkGlobals } from './workerCore'
import type { WorkerToHostMessage } from './sandboxProtocol'

// 必须在全局中和之前捕获原生引用:此后运行时的消息收发与异常监听仅经由这些
// 闭包私有引用,插件代码无法再通过全局 postMessage 伪造沙箱协议消息 (api-call 等),
// 也无法经 addEventListener 窃听宿主下行消息。运行时通道因此等效于私有
// MessagePort (插件与 runtime 共享 realm,任何留在全局作用域的引用都可被夺取)。
// 宿主侧另有 API_CALL_POLICY 白名单校验,此处为纵深防御。
const nativePostMessage = self.postMessage.bind(self)
const nativeAddEventListener = self.addEventListener.bind(self)

// 纵深防御:移除原生网络与逃逸相关 API (含 postMessage/close/indexedDB/
// importScripts/addEventListener 家族,并删除原型链定义阻断 __proto__ 绕过),
// 插件网络访问只能经 api.network.fetch 权限代理
// (与 CSP 继承互为备份,任一生效即可阻断未授权网络访问)
removeNetworkGlobals(self as unknown as Record<string, unknown>)

const post = (msg: WorkerToHostMessage): void => {
  try {
    nativePostMessage(msg)
  } catch {
    // 消息不可克隆时静默丢弃:workerCore 在发送前已做过序列化消毒,
    // 此处兜底避免 postMessage 异常中断 Worker 消息循环
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
// 经由中和前捕获的原生引用注册,不受全局 addEventListener 中和影响
nativeAddEventListener('unhandledrejection', (event) => {
  const reason = (event as PromiseRejectionEvent).reason
  runtime.reportUnhandledRejection(reason)
})
