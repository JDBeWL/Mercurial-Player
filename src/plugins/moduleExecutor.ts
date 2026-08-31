/**
 * 外置插件模块执行器
 *
 * 通过 blob URL + 动态 import() 将插件代码作为 ES 模块执行。
 * 生产环境 CSP 禁止 'unsafe-eval'（new Function/eval 均不可用），
 * 而 blob: 模块动态导入是浏览器原生支持的 CSP 兼容执行方式
 * （tauri.conf.json 的 script-src 已允许 blob:）。
 *
 * 支持两种外置插件代码格式：
 * 1. ES 模块格式（推荐）：`export default (api, globals) => pluginInstance`
 * 2. 旧脚本格式：顶层代码定义 `plugin` 变量，由本模块包装为 ES 模块
 */

import type { PluginMainFunction } from './pluginTypes'

export type PluginModuleFactory = PluginMainFunction

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
  // 在函数作用域内解构安全全局对象（console 代理、可清理的定时器），
  // 使旧格式插件代码继续获得沙箱注入，而非直接使用真实全局对象
  return `${code}\nexport default function (api, globals) {\n  ${GLOBALS_DESTRUCTURE}\n  return typeof plugin !== 'undefined' ? plugin : {}\n}\n`
}

/**
 * 以 ES 模块方式导入外置插件代码，返回默认导出的工厂函数
 *
 * blob URL 在导入完成后立即释放：模块已被求值，命名空间对象保持有效，
 * 且每次调用生成独立 URL，不受浏览器模块缓存影响（插件重载可生效）
 */
export async function importPluginModule(code: string): Promise<PluginModuleFactory> {
  const moduleCode = toModuleCode(code)
  const blob = new Blob([moduleCode], { type: 'text/javascript' })
  const url = URL.createObjectURL(blob)
  let mod: { default?: unknown }
  try {
    mod = (await import(/* @vite-ignore */ url)) as { default?: unknown }
  } catch (error) {
    // import() 失败通常是语法错误或不可解析的导入语句
    throw new Error(
      `插件模块加载失败（代码可能存在语法错误）: ${error instanceof Error ? error.message : String(error)}`,
    )
  } finally {
    URL.revokeObjectURL(url)
  }

  const factory = mod.default
  if (typeof factory !== 'function') {
    throw new Error('插件模块缺少默认导出函数 (export default function/api => {...})')
  }
  return factory as PluginModuleFactory
}
