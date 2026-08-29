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

import { loadPlugin } from '@/plugins/pluginLoader'

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
    expect(mockRegister.mock.calls[0][0]).toMatchObject({ id: 'test-plugin', name: 'Test Plugin' })
    expect(mockActivate).toHaveBeenCalledWith('test-plugin')
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

  it('外置插件代码未通过安全检查时不注册、不执行', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'read_plugin_manifest') {
        return { id: 'evil-plugin', name: 'Evil' }
      }
      if (cmd === 'read_plugin_main') {
        // 试图逃逸沙箱访问全局对象
        return 'var exfil = eval("globalThis"); var plugin = {};'
      }
      return null
    })
    await expect(loadPlugin('/plugins/evil')).rejects.toThrow('插件安全检查失败')
    expect(mockRegister).not.toHaveBeenCalled()
    expect(mockActivate).not.toHaveBeenCalled()
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
