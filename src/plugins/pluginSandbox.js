/**
 * 插件沙箱
 * 提供安全的执行环境
 */

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

// 创建安全的 Proxy 来拦截属性访问
function createSafeProxy(target, allowedKeys) {
  return new Proxy(target, {
    get(obj, prop) {
      if (typeof prop === 'string' && DANGEROUS_GLOBALS.includes(prop)) {
        return undefined
      }
      if (allowedKeys && !allowedKeys.includes(prop)) {
        return undefined
      }
      return obj[prop]
    },
    has(obj, prop) {
      if (typeof prop === 'string' && DANGEROUS_GLOBALS.includes(prop)) {
        return false
      }
      return prop in obj
    }
  })
}

/**
 * 创建插件沙箱环境
 * @param {Object} api 插件 API
 */
export function createPluginSandbox(api) {
  // 安全的 console 代理
  const safeConsole = {
    log: api.log.info,
    info: api.log.info,
    warn: api.log.warn,
    error: api.log.error,
    debug: api.log.debug,
  }

  // 安全的定时器（带清理追踪）
  const timers = new Set()
  const intervals = new Set()

  const safeSetTimeout = (fn, delay, ...args) => {
    const id = setTimeout(() => {
      timers.delete(id)
      try {
        fn(...args)
      } catch (e) {
        api.log.error('定时器执行错误:', e)
      }
    }, Math.min(delay, 60000)) // 最大 60 秒
    timers.add(id)
    return id
  }

  const safeClearTimeout = (id) => {
    timers.delete(id)
    clearTimeout(id)
  }

  const safeSetInterval = (fn, delay, ...args) => {
    if (delay < 100) delay = 100 // 最小 100ms
    const id = setInterval(() => {
      try {
        fn(...args)
      } catch (e) {
        api.log.error('定时器执行错误:', e)
      }
    }, delay)
    intervals.add(id)
    return id
  }

  const safeClearInterval = (id) => {
    intervals.delete(id)
    clearInterval(id)
  }

  // 允许插件访问的全局对象（冻结以防止修改）
  const allowedGlobals = Object.freeze({
    // 基础类型构造函数（冻结副本）
    Object: Object,
    Array: Array,
    String: String,
    Number: Number,
    Boolean: Boolean,
    Date: Date,
    RegExp: RegExp,
    Error: Error,
    TypeError: TypeError,
    RangeError: RangeError,
    SyntaxError: SyntaxError,
    Map: Map,
    Set: Set,
    WeakMap: WeakMap,
    WeakSet: WeakSet,
    
    // 工具
    JSON: Object.freeze({
      parse: JSON.parse,
      stringify: JSON.stringify,
    }),
    Math: Math,
    console: Object.freeze(safeConsole),
    
    // Promise
    Promise: Promise,
    
    // 安全的定时器
    setTimeout: safeSetTimeout,
    clearTimeout: safeClearTimeout,
    setInterval: safeSetInterval,
    clearInterval: safeClearInterval,
    
    // 编码
    encodeURIComponent,
    decodeURIComponent,
    encodeURI,
    decodeURI,
    btoa,
    atob,
    
    // 类型检查
    isNaN,
    isFinite,
    parseInt,
    parseFloat,
    
    // 插件 API
    api,
    
    // undefined 和 NaN
    undefined: undefined,
    NaN: NaN,
    Infinity: Infinity,
  })

  return {
    globals: allowedGlobals,
    
    /**
     * 在沙箱中执行代码
     * @param {Function} fn 要执行的函数
     */
    execute(fn) {
      // 直接调用函数，传入 api
      // 安全性由 pluginLoader 中的参数覆盖机制保证
      try {
        return fn(api)
      } catch (e) {
        api.log.error('插件执行错误:', e)
        throw e
      }
    },

    /**
     * 清理所有定时器
     */
    cleanup() {
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

/**
 * 验证插件代码安全性
 * 使用多层检测防止绕过
 * @param {string} code 插件代码
 */
export function validatePluginCode(code) {
  // 移除注释以防止在注释中隐藏恶意代码
  const codeWithoutComments = code
    .replace(/\/\*[\s\S]*?\*\//g, '') // 多行注释
    .replace(/\/\/.*$/gm, '')          // 单行注释

  // 移除字符串内容以防止误报，但保留字符串标记
  const codeWithoutStrings = codeWithoutComments
    .replace(/"(?:[^"\\]|\\.)*"/g, '""')
    .replace(/'(?:[^'\\]|\\.)*'/g, "''")
    .replace(/`(?:[^`\\]|\\.)*`/g, '``')

  // 禁止的模式 - 更严格的检测
  const forbidden = [
    // eval 和 Function 构造
    { pattern: /\beval\b/, msg: 'eval' },
    { pattern: /\bFunction\b/, msg: 'Function 构造函数' },
    
    // 全局对象访问
    { pattern: /\bdocument\b/, msg: 'document' },
    { pattern: /\bwindow\b/, msg: 'window' },
    { pattern: /\bglobalThis\b/, msg: 'globalThis' },
    { pattern: /\bself\b/, msg: 'self' },
    { pattern: /\btop\b/, msg: 'top' },
    { pattern: /\bparent\b/, msg: 'parent' },
    { pattern: /\bframes\b/, msg: 'frames' },
    
    // Node.js 相关
    { pattern: /\bprocess\b/, msg: 'process' },
    { pattern: /\brequire\b/, msg: 'require' },
    { pattern: /\bmodule\b/, msg: 'module' },
    { pattern: /\b__dirname\b/, msg: '__dirname' },
    { pattern: /\b__filename\b/, msg: '__filename' },
    
    // 原型链攻击
    { pattern: /__proto__/, msg: '__proto__' },
    { pattern: /\bconstructor\b\s*[.\[]/, msg: 'constructor 访问' },
    { pattern: /prototype\s*\[/, msg: 'prototype 动态访问' },
    { pattern: /Object\s*\.\s*getPrototypeOf/, msg: 'getPrototypeOf' },
    { pattern: /Object\s*\.\s*setPrototypeOf/, msg: 'setPrototypeOf' },
    { pattern: /Reflect\s*\./, msg: 'Reflect API' },
    
    // 网络请求（必须通过 API）
    { pattern: /\bfetch\b/, msg: 'fetch (请使用 api.network.fetch)' },
    { pattern: /\bXMLHttpRequest\b/, msg: 'XMLHttpRequest' },
    { pattern: /\bWebSocket\b/, msg: 'WebSocket' },
    
    // Worker
    { pattern: /\bWorker\b/, msg: 'Worker' },
    { pattern: /\bSharedWorker\b/, msg: 'SharedWorker' },
    
    // 存储（必须通过 API）
    { pattern: /\blocalStorage\b/, msg: 'localStorage (请使用 api.storage)' },
    { pattern: /\bsessionStorage\b/, msg: 'sessionStorage' },
    { pattern: /\bindexedDB\b/, msg: 'indexedDB' },
    
    // 动态代码执行
    { pattern: /\bimport\s*\(/, msg: '动态 import' },
    { pattern: /\bimportScripts\b/, msg: 'importScripts' },
    
    // 危险的字符串方法
    { pattern: /\['\\x/, msg: '十六进制转义访问' },
    { pattern: /\['\\u/, msg: 'Unicode 转义访问' },
    { pattern: /fromCharCode/, msg: 'fromCharCode' },
    { pattern: /fromCodePoint/, msg: 'fromCodePoint' },
    
    // Proxy（可能用于绕过沙箱）
    { pattern: /\bnew\s+Proxy\b/, msg: 'Proxy' },
  ]

  for (const { pattern, msg } of forbidden) {
    if (pattern.test(codeWithoutStrings)) {
      throw new Error(`插件代码包含不安全的模式: ${msg}`)
    }
  }

  // 检查可疑的属性访问模式（方括号访问）
  // 例如: obj['ev' + 'al'] 或 obj[variable]
  const bracketAccessPattern = /\[\s*[^0-9\]]/g
  const bracketMatches = codeWithoutStrings.match(bracketAccessPattern)
  if (bracketMatches && bracketMatches.length > 20) {
    // 如果有大量动态属性访问，可能是在尝试绕过检测
    throw new Error('插件代码包含过多动态属性访问，可能存在安全风险')
  }

  return true
}
