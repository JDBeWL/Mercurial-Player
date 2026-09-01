/**
 * 外置插件模块代码包装
 *
 * 将外置插件代码包装为 ES 模块源码,由插件沙箱 Worker
 * (src/plugins/sandbox/workerCore.ts) 以 blob URL + 动态 import() 求值。
 * Worker 环境中 CSP script-src 已允许 blob:,且 Worker 不存在
 * __TAURI_INTERNALS__ 与 DOM,插件代码与主窗口权限物理隔离。
 *
 * 支持两种外置插件代码格式:
 * 1. ES 模块格式（推荐）：`export default (api, globals) => pluginInstance`
 * 2. 旧脚本格式：顶层代码定义 `plugin` 变量，由本模块包装为 ES 模块
 */

export type { PluginMainFunction } from './pluginTypes'

// 旧脚本格式包装时注入的沙箱全局解构（与原 new Function 参数注入语义一致）
const GLOBALS_DESTRUCTURE =
  'const { console, setTimeout, clearTimeout, setInterval, clearInterval } = globals'

// 检测 ES 模块格式（含默认导出）
const MODULE_FORMAT_RE = /(^|[\n;})\]])\s*export\s+default\b/

/**
 * 将外置插件代码包装为 ES 模块源码
 */
export function toModuleCode(code: string): string {
  // ES 模块格式：直接使用插件自身的默认导出
  if (MODULE_FORMAT_RE.test(code)) {
    return code
  }
  // 旧脚本格式：包装为接收 (api, globals) 的默认导出函数。
  // 在函数作用域内解构安全全局对象（console 代理、Worker 原生定时器），
  // 使旧格式插件代码获得沙箱注入，而非直接使用 Worker 全局对象
  return `${code}\nexport default function (api, globals) {\n  ${GLOBALS_DESTRUCTURE}\n  return typeof plugin !== 'undefined' ? plugin : {}\n}\n`
}
