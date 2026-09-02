// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { mockInvoke } from '../setup'

// pluginLoader 使用单例 pluginManager,这里 mock 掉以隔离生命周期副作用,
// 只保留 loadPlugin 依赖的方法 (plugins/deactivate/uninstall/register/activate)。
// vi.mock 会被提升到文件顶部,用 vi.hoisted 避免 TDZ 引用错误。
const { mockRegister, mockActivate, mockDeactivate, mockUninstall } = vi.hoisted(() => ({
  mockRegister: vi.fn(),
  mockActivate: vi.fn(),
  mockDeactivate: vi.fn(),
  mockUninstall: vi.fn(),
}))

vi.mock('@/plugins/pluginManager', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/plugins/pluginManager')>()
  return {
    ...actual,
    default: {
      plugins: new Map(),
      register: mockRegister,
      activate: mockActivate,
      deactivate: mockDeactivate,
      uninstall: mockUninstall,
    },
  }
})

// Node 测试环境不支持真实 Worker,mock 掉沙箱宿主,
// 只保留 loadPlugin 依赖的清单校验与注册流程。
const { mockHostInit, mockHostTerminate, mockHostCreateMain } = vi.hoisted(() => ({
  mockHostInit: vi.fn(async () => {}),
  mockHostTerminate: vi.fn(),
  mockHostCreateMain: vi.fn(() => vi.fn()),
}))

vi.mock('@/plugins/sandbox/workerSandboxHost', () => {
  return {
    // 注意:实现必须用 function 形式返回对象,箭头函数不能被 new 调用
    PluginWorkerHost: vi.fn(function () {
      return {
        init: mockHostInit,
        terminate: mockHostTerminate,
        createMainFunction: mockHostCreateMain,
      }
    }),
  }
})

import { loadPlugin } from '@/plugins/pluginLoader'
import logger from '@/utils/logger'

function manifestFor(id: string, extra: Record<string, unknown> = {}): void {
  mockInvoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'read_plugin_manifest') {
      return { id, name: 'Test Plugin', ...extra }
    }
    if (cmd === 'read_plugin_main') {
      return 'var plugin = { activate: function () {} };'
    }
    return null
  })
}

describe('pluginLoader - loadPlugin 清单与代码安全校验', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockInvoke.mockReset()
  })

  it('加载合法外置插件: 注册并默认自动激活', async () => {
    manifestFor('test-plugin')
    await loadPlugin('/plugins/test-plugin')
    expect(mockRegister).toHaveBeenCalledTimes(1)
    expect(mockRegister.mock.calls[0]![0]).toMatchObject({ id: 'test-plugin', name: 'Test Plugin' })
    expect(mockActivate).toHaveBeenCalledWith('test-plugin')
  })

  it('加载 ES 模块格式外置插件: 默认导出工厂被注册', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'read_plugin_manifest') {
        return { id: 'esm-plugin', name: 'ESM Plugin' }
      }
      if (cmd === 'read_plugin_main') {
        return 'export default (api) => ({ activate() { api.log.info("ok") } })'
      }
      return null
    })
    await loadPlugin('/plugins/esm-plugin')
    expect(mockRegister).toHaveBeenCalledTimes(1)
    const registered = mockRegister.mock.calls[0]![0]
    expect(registered.id).toBe('esm-plugin')
    expect(typeof registered.main).toBe('function')
    expect(mockActivate).toHaveBeenCalledWith('esm-plugin')
  })

  it('模块缺少默认导出时加载失败且不注册', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'read_plugin_manifest') {
        return { id: 'bad-esm-plugin', name: 'Bad ESM' }
      }
      if (cmd === 'read_plugin_main') {
        return 'export const plugin = {}'
      }
      return null
    })
    mockHostInit.mockRejectedValueOnce(
      new Error('插件模块缺少默认导出函数 (export default function/api => {...})'),
    )
    await expect(loadPlugin('/plugins/bad-esm')).rejects.toThrow('插件模块缺少默认导出')
    expect(mockRegister).not.toHaveBeenCalled()
    expect(mockHostTerminate).toHaveBeenCalled() // 加载失败时终止沙箱宿主
  })

  it.each([
    ['路径穿越 id', '../evil'],
    ['含斜杠 id', 'a/b'],
    ['含特殊字符 id', 'evil plugin!'],
  ])('拒绝非法插件 id: %s', async (_name, id) => {
    manifestFor(id)
    await expect(loadPlugin('/plugins/bad')).rejects.toThrow('插件ID只能包含')
    expect(mockRegister).not.toHaveBeenCalled()
  })

  it('拒绝缺少 name 的清单', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'read_plugin_manifest') return { id: 'test-plugin' }
      return null
    })
    await expect(loadPlugin('/plugins/bad')).rejects.toThrow('name')
    expect(mockRegister).not.toHaveBeenCalled()
  })

  it('拒绝未定义的权限申请 (最小权限原则)', async () => {
    manifestFor('test-plugin', { permissions: ['fileSystem:write'] })
    await expect(loadPlugin('/plugins/bad')).rejects.toThrow('无效的权限')
    expect(mockRegister).not.toHaveBeenCalled()
  })

  it('拒绝格式非法的版本号', async () => {
    manifestFor('test-plugin', { version: 'not-semver' })
    await expect(loadPlugin('/plugins/bad')).rejects.toThrow('版本号格式无效')
    expect(mockRegister).not.toHaveBeenCalled()
  })

  it('外置插件代码未通过静态检查时仅告警,不阻断加载 (安全边界由沙箱承担)', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'read_plugin_manifest') {
        return { id: 'evil-plugin', name: 'Evil' }
      }
      if (cmd === 'read_plugin_main') {
        // 试图逃逸沙箱访问全局对象 (正则黑名单可被等价变形绕过,故不阻断)
        return 'var exfil = eval("globalThis"); var plugin = {};'
      }
      return null
    })
    const warnSpy = vi.spyOn(logger, 'warn').mockImplementation(() => {})
    try {
      await loadPlugin('/plugins/evil')
      // 告警不阻断:插件仍被注册并在沙箱中隔离执行
      expect(mockRegister).toHaveBeenCalledTimes(1)
      expect(mockActivate).toHaveBeenCalledWith('evil-plugin')
      expect(warnSpy).toHaveBeenCalled()
    } finally {
      warnSpy.mockRestore()
    }
  })

  it('清单读取失败 (null) 时报错且不注册', async () => {
    mockInvoke.mockResolvedValue(null)
    await expect(loadPlugin('/plugins/none')).rejects.toThrow('无法读取插件清单')
    expect(mockRegister).not.toHaveBeenCalled()
  })

  it('已存在的插件会先卸载再重新加载', async () => {
    manifestFor('reload-plugin')
    // 模拟已注册
    const { default: manager } = await import('@/plugins/pluginManager')
    ;(manager.plugins as Map<string, unknown>).set('reload-plugin', { id: 'reload-plugin' })
    await loadPlugin('/plugins/reload-plugin')
    expect(mockDeactivate).toHaveBeenCalledWith('reload-plugin')
    expect(mockUninstall).toHaveBeenCalledWith('reload-plugin', false)
    expect(mockRegister).toHaveBeenCalledTimes(1)
    ;(manager.plugins as Map<string, unknown>).clear()
  })
})
