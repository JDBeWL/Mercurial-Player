// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { mockTauriFetch } from '../setup'
import { pluginManager, PluginState, PluginPermission } from '@/plugins/pluginManager'
import { createPluginAPI } from '@/plugins/pluginAPI'

// vitest 4 + happy-dom 环境不把 localStorage 暴露为全局,补一个内存实现。
// 插件存储代码通过全局 localStorage 读写,shim 必须在用例执行前就位。
if (typeof localStorage === 'undefined') {
  const backing = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => (backing.has(key) ? (backing.get(key) as string) : null),
    setItem: (key: string, value: string) => void backing.set(key, String(value)),
    removeItem: (key: string) => void backing.delete(key),
    clear: () => void backing.clear(),
    key: (index: number) => Array.from(backing.keys())[index] ?? null,
    get length() {
      return backing.size
    },
  })
}

/**
 * 插件管理器与插件 API 的安全边界测试:
 * 权限门禁 (最小权限)、存储隔离 (插件间不可互读)、生命周期状态机。
 */

describe('pluginManager - 生命周期', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    localStorage.clear()
    // 清空单例状态,避免用例间串扰
    for (const id of Array.from(pluginManager.plugins.keys())) {
      pluginManager.plugins.delete(id)
    }
  })

  it('register 缺少 id/name/main 时拒绝注册', async () => {
    await expect(
      pluginManager.register({ id: '', name: 'x', main: async () => ({}) }),
    ).rejects.toThrow('插件必须包含')
    await expect(
      pluginManager.register({ id: 'a', name: '', main: async () => ({}) }),
    ).rejects.toThrow('插件必须包含')
  })

  it('重复注册同一 id 会被拒绝', async () => {
    await pluginManager.register({ id: 'dup', name: 'A', main: async () => ({}) })
    await expect(
      pluginManager.register({ id: 'dup', name: 'B', main: async () => ({}) }),
    ).rejects.toThrow('已存在')
  })

  it('activate 执行插件 main 并进入 ACTIVE 状态', async () => {
    const activateFn = vi.fn()
    await pluginManager.register({
      id: 'lifecycle',
      name: 'L',
      permissions: [],
      main: async () => ({ activate: activateFn }),
    })
    await pluginManager.activate('lifecycle')
    const plugin = pluginManager.plugins.get('lifecycle')
    expect(plugin?.state).toBe(PluginState.ACTIVE)
    expect(activateFn).toHaveBeenCalled()
  })

  it('main 抛错时插件进入 ERROR 状态并记录错误信息', async () => {
    await pluginManager.register({
      id: 'boom',
      name: 'B',
      permissions: [],
      main: async () => {
        throw new Error('crash in main')
      },
    })
    await expect(pluginManager.activate('boom')).rejects.toThrow('crash in main')
    const plugin = pluginManager.plugins.get('boom')
    expect(plugin?.state).toBe(PluginState.ERROR)
    expect(plugin?.error).toBe('crash in main')
  })

  it('deactivate 调用插件 deactivate 并执行沙箱清理', async () => {
    const deactivateFn = vi.fn()
    await pluginManager.register({
      id: 'cleanup',
      name: 'C',
      permissions: [],
      main: async () => ({ deactivate: deactivateFn }),
    })
    await pluginManager.activate('cleanup')
    await pluginManager.deactivate('cleanup')
    expect(deactivateFn).toHaveBeenCalled()
    expect(pluginManager.plugins.get('cleanup')?.state).toBe(PluginState.INACTIVE)
  })
})

describe('pluginAPI - 权限门禁 (最小权限原则)', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('无 storage 权限时存储操作被拒绝', () => {
    const api = createPluginAPI('no-storage-plugin', [], pluginManager)
    expect(() => api.storage.get('k')).toThrow('没有 storage 权限')
    expect(() => api.storage.set('k', 1)).toThrow('没有 storage 权限')
    expect(() => api.storage.remove('k')).toThrow('没有 storage 权限')
    expect(() => api.storage.getAll()).toThrow('没有 storage 权限')
  })

  it('无 player:read 权限时读取播放器状态被拒绝', () => {
    const api = createPluginAPI('no-read-plugin', [], pluginManager)
    expect(() => api.player.getState()).toThrow('没有 player:read 权限')
  })

  it('无 network 权限时网络请求被拒绝', async () => {
    const api = createPluginAPI('no-net-plugin', [], pluginManager)
    await expect(api.network.fetch('https://example.com')).rejects.toThrow('没有 network 权限')
    expect(mockTauriFetch).not.toHaveBeenCalled()
  })

  it('network.fetch 只允许 HTTPS', async () => {
    const api = createPluginAPI('net-plugin', [PluginPermission.NETWORK], pluginManager)
    await expect(api.network.fetch('http://example.com')).rejects.toThrow('只允许 HTTPS')
    await expect(api.network.fetch('ftp://example.com')).rejects.toThrow('只允许 HTTPS')
  })

  it('network.fetch 走受控通道并添加插件标记头', async () => {
    mockTauriFetch.mockResolvedValueOnce({ url: 'https://example.com', ok: true })
    const api = createPluginAPI('net-plugin', [PluginPermission.NETWORK], pluginManager)
    const res = await api.network.fetch('https://example.com', { headers: { 'X-A': '1' } })
    expect(res.ok).toBe(true)
    expect(mockTauriFetch).toHaveBeenCalledWith(
      'https://example.com',
      expect.objectContaining({
        headers: expect.objectContaining({ 'X-Plugin-Request': 'true', 'X-A': '1' }),
        redirect: 'manual',
      }),
    )
  })

  it('network.fetch 请求头不暴露具体插件 ID', async () => {
    mockTauriFetch.mockResolvedValueOnce({ url: 'https://example.com' })
    const api = createPluginAPI('sneaky-plugin', [PluginPermission.NETWORK], pluginManager)
    await api.network.fetch('https://example.com')
    const [, init] = mockTauriFetch.mock.calls[0] as [string, { headers: Record<string, string> }]
    expect(JSON.stringify(init.headers)).not.toContain('sneaky-plugin')
  })
})

describe('pluginAPI - 存储隔离', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
    localStorage.clear()
  })

  it('不同插件的同名键互不可见', () => {
    const apiA = createPluginAPI('plugin-a', [PluginPermission.STORAGE], pluginManager)
    const apiB = createPluginAPI('plugin-b', [PluginPermission.STORAGE], pluginManager)

    apiA.storage.set('data', 'from-a')
    apiA.storage.set('shared', { secret: 'a' })
    apiB.storage.set('data', 'from-b')

    expect(apiA.storage.get('data')).toBe('from-a')
    expect(apiB.storage.get('data')).toBe('from-b')
    // B 的 getAll 不包含 A 的键
    expect(Object.keys(apiB.storage.getAll())).not.toContain('shared')
    expect(apiB.storage.get('shared')).toBeNull()
  })

  it('存储持久化到以插件 id 命名的独立 localStorage 键', async () => {
    const api = createPluginAPI('persist-plugin', [PluginPermission.STORAGE], pluginManager)
    api.storage.set('k', 'v')
    // safeSave 是异步防抖写入,等待微任务/定时器 flush
    await new Promise((resolve) => setTimeout(resolve, 1500))
    const raw = localStorage.getItem('mercurial-plugin-storage-persist-plugin')
    expect(raw).not.toBeNull()
    expect(JSON.parse(raw as string)).toMatchObject({ k: 'v' })
  })

  it('remove 删除指定键且不影响其他键', () => {
    const api = createPluginAPI('remove-plugin', [PluginPermission.STORAGE], pluginManager)
    api.storage.set('a', 1)
    api.storage.set('b', 2)
    api.storage.remove('a')
    expect(api.storage.get('a')).toBeNull()
    expect(api.storage.get('b')).toBe(2)
  })
})
