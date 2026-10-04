/** 主窗口侧插件沙箱:白名单全局 + 安全 console + 可清理定时器;外置插件走 sandbox/workerSandboxHost 的 Worker 隔离 */

import type { PluginAPI, PluginInstance, PluginMainFunction } from './pluginTypes'

interface SafeConsole {
  log: (...args: unknown[]) => void
  info: (...args: unknown[]) => void
  warn: (...args: unknown[]) => void
  error: (...args: unknown[]) => void
  debug: (...args: unknown[]) => void
}

// 插件在沙箱内可见的全部全局;名单之外的宿主全局一律不可访问
interface AllowedGlobals {
  Object: typeof Object
  Array: typeof Array
  String: typeof String
  Number: typeof Number
  Boolean: typeof Boolean
  Date: typeof Date
  RegExp: typeof RegExp
  Error: typeof Error
  TypeError: typeof TypeError
  RangeError: typeof RangeError
  SyntaxError: typeof SyntaxError
  Map: typeof Map
  Set: typeof Set
  WeakMap: typeof WeakMap
  WeakSet: typeof WeakSet
  JSON: { parse: typeof JSON.parse; stringify: typeof JSON.stringify }
  Math: typeof Math
  console: SafeConsole
  Promise: typeof Promise
  setTimeout: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearTimeout: (id: number) => void
  setInterval: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearInterval: (id: number) => void
  encodeURIComponent: typeof encodeURIComponent
  decodeURIComponent: typeof decodeURIComponent
  encodeURI: typeof encodeURI
  decodeURI: typeof decodeURI
  btoa: typeof btoa
  atob: typeof atob
  isNaN: typeof isNaN
  isFinite: typeof isFinite
  parseInt: typeof parseInt
  parseFloat: typeof parseFloat
  api: PluginAPI
  undefined: undefined
  NaN: number
  Infinity: number
}

export interface PluginSandbox {
  globals: AllowedGlobals
  execute: <T>(fn: PluginMainFunction | (() => T | Promise<T>)) => Promise<T | PluginInstance>
  cleanup: () => void
}

/** 安全 console 代理:log 与 info 都走 api.log.info,其余映射到同名方法 */
function createSafeConsole(log: PluginAPI['log']): SafeConsole {
  return {
    log: log.info,
    info: log.info,
    warn: log.warn,
    error: log.error,
    debug: log.debug,
  }
}

interface SafeTimers {
  setTimeout: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearTimeout: (id: number) => void
  setInterval: (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]) => number
  clearInterval: (id: number) => void
  cleanup: () => void
}

/**
 * 安全定时器组:setTimeout 延迟上限 60s,setInterval 最小间隔 100ms,回调抛错交给 onError
 *
 * cleanup 回收所有未触发的定时器,插件停用时调用,防止插件泄漏的定时器一直跑
 */
function createSafeTimers(onError: (error: unknown) => void): SafeTimers {
  const timers = new Set<number>()
  const intervals = new Set<number>()

  const safeSetTimeout = (
    fn: (...args: unknown[]) => void,
    delay?: number,
    ...args: unknown[]
  ): number => {
    const id = setTimeout(
      () => {
        timers.delete(id)
        try {
          fn(...args)
        } catch (e) {
          onError(e)
        }
      },
      Math.min(delay || 0, 60000),
    )
    timers.add(id)
    return id
  }

  const safeClearTimeout = (id: number): void => {
    timers.delete(id)
    clearTimeout(id)
  }

  const safeSetInterval = (
    fn: (...args: unknown[]) => void,
    delay?: number,
    ...args: unknown[]
  ): number => {
    const safeDelay = Math.max(delay || 100, 100)
    const id = setInterval(() => {
      try {
        fn(...args)
      } catch (e) {
        onError(e)
      }
    }, safeDelay)
    intervals.add(id)
    return id
  }

  const safeClearInterval = (id: number): void => {
    intervals.delete(id)
    clearInterval(id)
  }

  return {
    setTimeout: safeSetTimeout,
    clearTimeout: safeClearTimeout,
    setInterval: safeSetInterval,
    clearInterval: safeClearInterval,

    cleanup(): void {
      for (const id of timers) {
        clearTimeout(id)
      }
      timers.clear()
      for (const id of intervals) {
        clearInterval(id)
      }
      intervals.clear()
    },
  }
}

/** 为单个插件创建沙箱:注入白名单 globals 并绑定该插件的 api */
export function createPluginSandbox(api: PluginAPI): PluginSandbox {
  const safeConsole = createSafeConsole(api.log)

  const safeTimers = createSafeTimers((e) => api.log.error('定时器执行错误:', e))

  const allowedGlobals: AllowedGlobals = Object.freeze({
    Object,
    Array,
    String,
    Number,
    Boolean,
    Date,
    RegExp,
    Error,
    TypeError,
    RangeError,
    SyntaxError,
    Map,
    Set,
    WeakMap,
    WeakSet,
    JSON: Object.freeze({
      parse: JSON.parse,
      stringify: JSON.stringify,
    }),
    Math,
    console: Object.freeze(safeConsole),
    Promise,
    setTimeout: safeTimers.setTimeout,
    clearTimeout: safeTimers.clearTimeout,
    setInterval: safeTimers.setInterval,
    clearInterval: safeTimers.clearInterval,
    encodeURIComponent,
    decodeURIComponent,
    encodeURI,
    decodeURI,
    btoa,
    atob,
    isNaN,
    isFinite,
    parseInt,
    parseFloat,
    api,
    undefined: undefined,
    NaN: NaN,
    Infinity: Infinity,
  }) as AllowedGlobals

  return {
    globals: allowedGlobals,

    /**
     * 在沙箱中执行插件函数
     *
     * 第二参数 globals 注入沙箱全局(安全 console + 可清理定时器),供外置插件的旧格式包装代码解构;新格式与内置插件可忽略
     */
    async execute<T>(fn: PluginMainFunction | (() => T | Promise<T>)): Promise<T | PluginInstance> {
      try {
        return await (
          fn as (api: PluginAPI, globals?: AllowedGlobals) => Promise<T | PluginInstance>
        )(api, this.globals)
      } catch (e) {
        api.log.error('插件执行错误:', e)
        throw e
      }
    },

    cleanup(): void {
      safeTimers.cleanup()
    },
  }
}

interface ForbiddenPattern {
  pattern: RegExp
  msg: string
}

/** 静态黑名单扫描,命中即抛错;调用方按告警处理,不阻断加载(原因见 pluginLoader) */
export function validatePluginCode(code: string): boolean {
  if (code.length > 1024 * 1024) {
    throw new Error('插件代码过大，超过1MB限制')
  }

  // 先剥离注释,避免注释里的内容触发黑名单
  const codeWithoutComments = code
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/.*$/gm, ' ')
    .replace(/<!--[\s\S]*?-->/g, ' ')

  // 字符串形式的危险属性访问必须在下方字符串剥离之前检查:这些模式依赖字符串内容(如 obj['\x63onstructor']),
  // 剥离后内容变成 "",规则将永远命中不到
  const stringFormForbidden: ForbiddenPattern[] = [
    { pattern: /\[\s*['"`]constructor['"`]\s*\]/, msg: '字符串形式的constructor访问' },
    { pattern: /\[\s*['"`]\\x/, msg: '十六进制转义访问' },
    { pattern: /\[\s*['"`]\\u/, msg: 'Unicode 转义访问' },
  ]
  for (const { pattern, msg } of stringFormForbidden) {
    if (pattern.test(codeWithoutComments)) {
      throw new Error(`插件代码包含不安全的模式: ${msg}`)
    }
  }

  // 再剥离字符串内容,只在代码骨架上匹配
  const codeWithoutStrings = codeWithoutComments
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, '``')

  // 黑名单:原型链逃逸,宿主全局,二次求值与常见绕过手法
  const forbidden: ForbiddenPattern[] = [
    { pattern: /\beval\b/, msg: 'eval' },
    { pattern: /\bFunction\b/, msg: 'Function 构造函数' },
    { pattern: /\bdocument\b/, msg: 'document' },
    { pattern: /\bwindow\b/, msg: 'window' },
    { pattern: /\bglobalThis\b/, msg: 'globalThis' },
    { pattern: /\bself\b/, msg: 'self' },
    { pattern: /\btop\b/, msg: 'top' },
    { pattern: /\bparent\b/, msg: 'parent' },
    { pattern: /\bframes\b/, msg: 'frames' },
    { pattern: /\bprocess\b/, msg: 'process' },
    { pattern: /\brequire\b/, msg: 'require' },
    { pattern: /\bmodule\b/, msg: 'module' },
    { pattern: /\b__dirname\b/, msg: '__dirname' },
    { pattern: /\b__filename\b/, msg: '__filename' },
    { pattern: /__proto__/, msg: '__proto__' },
    { pattern: /\bconstructor\b\s*[.[]/, msg: 'constructor 访问' },
    { pattern: /prototype\s*\[/, msg: 'prototype 动态访问' },
    { pattern: /Object\s*\.\s*getPrototypeOf/, msg: 'getPrototypeOf' },
    { pattern: /Object\s*\.\s*setPrototypeOf/, msg: 'setPrototypeOf' },
    { pattern: /Reflect\s*\./, msg: 'Reflect API' },
    { pattern: /\bfetch\b/, msg: 'fetch (请使用 api.network.fetch)' },
    { pattern: /\bXMLHttpRequest\b/, msg: 'XMLHttpRequest' },
    { pattern: /\bWebSocket\b/, msg: 'WebSocket' },
    { pattern: /\bWorker\b/, msg: 'Worker' },
    { pattern: /\bSharedWorker\b/, msg: 'SharedWorker' },
    { pattern: /\blocalStorage\b/, msg: 'localStorage (请使用 api.storage)' },
    { pattern: /\bsessionStorage\b/, msg: 'sessionStorage' },
    { pattern: /\bindexedDB\b/, msg: 'indexedDB' },
    { pattern: /\bimport\s*\(/, msg: '动态 import' },
    { pattern: /\bimportScripts\b/, msg: 'importScripts' },
    { pattern: /fromCharCode/, msg: 'fromCharCode' },
    { pattern: /fromCodePoint/, msg: 'fromCodePoint' },
    { pattern: /\bnew\s+Proxy\b/, msg: 'Proxy' },
    { pattern: /String\s*\.\s*fromCharCode/, msg: 'String.fromCharCode' },
    { pattern: /Array\s*\.\s*from/, msg: 'Array.from (可能用于绕过检查)' },
    { pattern: /Object\s*\.\s*keys\s*\(\s*this\s*\)/, msg: '遍历this对象' },
    { pattern: /Object\s*\.\s*getOwnPropertyNames/, msg: 'getOwnPropertyNames' },
    { pattern: /Object\s*\.\s*getOwnPropertyDescriptor/, msg: 'getOwnPropertyDescriptor' },
    { pattern: /\+\s*['"`]\s*['"`]\s*\+/, msg: '可疑的字符串拼接' },
  ]

  for (const { pattern, msg } of forbidden) {
    if (pattern.test(codeWithoutStrings)) {
      throw new Error(`插件代码包含不安全的模式: ${msg}`)
    }
  }

  // 非数字下标的动态属性访问超过 15 次视为绕过企图
  const bracketAccessPattern = /\[\s*[^0-9\]]/g
  const bracketMatches = codeWithoutStrings.match(bracketAccessPattern)
  if (bracketMatches && bracketMatches.length > 15) {
    throw new Error('插件代码包含过多动态属性访问，可能存在安全风险')
  }

  // 三层及以上的直接嵌套调用判为可疑
  const nestedCallPattern = /\(\s*[^)]*\(\s*[^)]*\(\s*[^)]*\(/g
  if (nestedCallPattern.test(codeWithoutStrings)) {
    throw new Error('插件代码包含过深的嵌套调用，可能存在安全风险')
  }

  return true
}
