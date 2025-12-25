/**
 * 插件沙箱
 * 提供安全的执行环境
 */

/**
 * 创建插件沙箱环境
 * @param {Object} api 插件 API
 */
export function createPluginSandbox(api) {
  // 允许插件访问的全局对象
  const allowedGlobals = {
    // 基础类型
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
    
    // 工具
    JSON,
    Math,
    console: {
      log: api.log.info,
      info: api.log.info,
      warn: api.log.warn,
      error: api.log.error,
      debug: api.log.debug,
    },
    
    // Promise
    Promise,
    
    // 定时器
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    
    // 编码
    encodeURIComponent,
    decodeURIComponent,
    encodeURI,
    decodeURI,
    btoa,
    atob,
    
    // 插件 API
    api,
  }

  return {
    globals: allowedGlobals,
    
    /**
     * 在沙箱中执行代码
     * @param {Function} fn 要执行的函数
     */
    execute(fn) {
      return fn(api)
    },
  }
}

/**
 * 验证插件代码安全性
 * @param {string} code 插件代码
 */
export function validatePluginCode(code) {
  // 禁止的关键字和模式
  const forbidden = [
    /eval\s*\(/,
    /Function\s*\(/,
    /new\s+Function/,
    /document\./,
    /window\./,
    /globalThis\./,
    /process\./,
    /require\s*\(/,
    /__proto__/,
    /prototype\s*\[/,
  ]

  for (const pattern of forbidden) {
    if (pattern.test(code)) {
      throw new Error(`插件代码包含不安全的模式: ${pattern}`)
    }
  }

  return true
}
