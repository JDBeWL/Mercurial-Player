/**
 * 插件沙箱
 * 提供安全的执行环境
 */

import type { PluginAPI, PluginInstance, PluginMainFunction } from './pluginManager'

// 危险的全局对象列表
const DANGEROUS_GLOBALS = [
  'window', 'document', 'globalThis', 'self', 'top', 'parent', 'frames',
  'eval', 'Function', 'constructor',
  'process', 'require', 'module', 'exports', '__dirname', '__filename',
  'XMLHttpRequest', 'fetch', 'WebSocket', 'Worker', 'SharedWorker',
  'importScripts', 'postMessage',
  'localStorage', 'sessionStorage', 'indexedDB', 'caches',
  'navigator', 'location', 'history',
  'alert', 'confirm', 'prompt', 'open', 'close',
  'requestAnimationFrame', 'cancelAnimationFrame',
  'MutationObserver', 'IntersectionObserver', 'ResizeObserver',
  'Proxy', 'Reflect',
]

// 安全的 console 类型
interface SafeConsole {
  log: (...args: unknown[]) => void
  info: (...args: unknown[]) => void
  warn: (...args: unknown[]) => void
  error: (...args: unknown[]) => void
  debug: (...args: unknown[]) => void
}

// 允许的全局对象类型
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

// 沙箱类型
export interface PluginSandbox {
  globals: AllowedGlobals
  execute: <T>(fn: PluginMainFunction | (() => T | Promise<T>)) => Promise<T | PluginInstance>
  cleanup: () => void
}

/**
 * 创建插件沙箱环境
 */
export function createPluginSandbox(api: PluginAPI): PluginSandbox {
  // 安全的 console 代理
  const safeConsole: SafeConsole = {
    log: api.log.info,
    info: api.log.info,
    warn: api.log.warn,
    error: api.log.error,
    debug: api.log.debug,
  }

  // 安全的定时器（带清理追踪）
  const timers = new Set<number>()
  const intervals = new Set<number>()

  const safeSetTimeout = (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]): number => {
    const id = setTimeout(() => {
      timers.delete(id as unknown as number)
      try {
        fn(...args)
      } catch (e) {
        api.log.error('定时器执行错误:', e)
      }
    }, Math.min(delay || 0, 60000)) as unknown as number
    timers.add(id)
    return id
  }

  const safeClearTimeout = (id: number): void => {
    timers.delete(id)
    clearTimeout(id)
  }

  const safeSetInterval = (fn: (...args: unknown[]) => void, delay?: number, ...args: unknown[]): number => {
    const safeDelay = Math.max(delay || 100, 100)
    const id = setInterval(() => {
      try {
        fn(...args)
      } catch (e) {
        api.log.error('定时器执行错误:', e)
      }
    }, safeDelay) as unknown as number
    intervals.add(id)
    return id
  }

  const safeClearInterval = (id: number): void => {
    intervals.delete(id)
    clearInterval(id)
  }

  // 允许插件访问的全局对象
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
    setTimeout: safeSetTimeout,
    clearTimeout: safeClearTimeout,
    setInterval: safeSetInterval,
    clearInterval: safeClearInterval,
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
     * 在沙箱中执行代码
     */
    async execute<T>(fn: PluginMainFunction | (() => T | Promise<T>)): Promise<T | PluginInstance> {
      try {
        return await (fn as (api: PluginAPI) => Promise<T | PluginInstance>)(api)
      } catch (e) {
        api.log.error('插件执行错误:', e)
        throw e
      }
    },

    /**
     * 清理所有定时器
     */
    cleanup(): void {
      for (const id of timers) {
        clearTimeout(id)
      }
      timers.clear()
      for (const id of intervals) {
        clearInterval(id)
      }
      intervals.clear()
    }
  }
}

// 禁止的模式类型
interface ForbiddenPattern {
  pattern: RegExp
  msg: string
}

/**
 * 验证插件代码安全性
 */
export function validatePluginCode(code: string): boolean {
  // 基本长度检查
  if (code.length > 1024 * 1024) { // 1MB限制
    throw new Error('插件代码过大，超过1MB限制')
  }

  // 移除注释（更严格的注释移除）
  const codeWithoutComments = code
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/\/\/.*$/gm, ' ')
    .replace(/<!--[\s\S]*?-->/g, ' ')

  // 移除字符串内容（更严格的字符串移除）
  const codeWithoutStrings = codeWithoutComments
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, '``')

  // 禁止的模式
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
    { pattern: /\bconstructor\b\s*[.\[]/, msg: 'constructor 访问' },
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
    { pattern: /\['\\x/, msg: '十六进制转义访问' },
    { pattern: /\['\\u/, msg: 'Unicode 转义访问' },
    { pattern: /fromCharCode/, msg: 'fromCharCode' },
    { pattern: /fromCodePoint/, msg: 'fromCodePoint' },
    { pattern: /\bnew\s+Proxy\b/, msg: 'Proxy' },
    // 新增的安全检查
    { pattern: /String\s*\.\s*fromCharCode/, msg: 'String.fromCharCode' },
    { pattern: /Array\s*\.\s*from/, msg: 'Array.from (可能用于绕过检查)' },
    { pattern: /Object\s*\.\s*keys\s*\(\s*this\s*\)/, msg: '遍历this对象' },
    { pattern: /Object\s*\.\s*getOwnPropertyNames/, msg: 'getOwnPropertyNames' },
    { pattern: /Object\s*\.\s*getOwnPropertyDescriptor/, msg: 'getOwnPropertyDescriptor' },
    { pattern: /\[\s*['"`]constructor['"`]\s*\]/, msg: '字符串形式的constructor访问' },
    { pattern: /\+\s*['"`]\s*['"`]\s*\+/, msg: '可疑的字符串拼接' },
  ]

  for (const { pattern, msg } of forbidden) {
    if (pattern.test(codeWithoutStrings)) {
      throw new Error(`插件代码包含不安全的模式: ${msg}`)
    }
  }

  // 检查可疑的属性访问模式
  const bracketAccessPattern = /\[\s*[^0-9\]]/g
  const bracketMatches = codeWithoutStrings.match(bracketAccessPattern)
  if (bracketMatches && bracketMatches.length > 15) { // 降低阈值
    throw new Error('插件代码包含过多动态属性访问，可能存在安全风险')
  }

  // 检查嵌套函数调用深度
  const nestedCallPattern = /\(\s*[^)]*\(\s*[^)]*\(\s*[^)]*\(/g
  if (nestedCallPattern.test(codeWithoutStrings)) {
    throw new Error('插件代码包含过深的嵌套调用，可能存在安全风险')
  }

  return true
}
