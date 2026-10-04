// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { createPluginStorage, PLUGIN_STORAGE_PREFIX } from '@/plugins/pluginStorage'

// vitest 4 + happy-dom 不把 localStorage 暴露为全局, 而插件存储代码经全局 localStorage 读写,
// shim 必须在用例执行前就位 (与 pluginManager.test.ts 同样的处理)
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
 * P1-3 回归: flush/cleanup 若是普通数据键, 展开快照会带上函数 -> 宿主 postMessage 结构化克隆抛
 * DataCloneError 使整份状态镜像被静默丢弃, 且插件能用 api.storage.set('flush', 1) 覆盖生命周期方法
 */
describe('pluginStorage - 生命周期方法隔离 (P1-3 回归)', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('flush/cleanup 不作为数据键出现 (spread / Object.keys 不可见)', () => {
    const storage = createPluginStorage('iso-1')
    storage.foo = 'bar'

    const spread = { ...storage }
    expect(Object.keys(spread).sort()).toEqual(['foo'])
    expect('flush' in spread).toBe(false)
    expect('cleanup' in spread).toBe(false)
    expect(Object.keys(storage)).not.toContain('flush')
    expect(Object.keys(storage)).not.toContain('cleanup')
  })

  it('数据快照可结构化克隆 (镜像推送不再抛 DataCloneError)', () => {
    const storage = createPluginStorage('iso-2')
    storage.counter = 5
    storage.name = 'hello'

    // postMessage 语义:任何不可克隆的值都会让整条镜像消息失败
    expect(() => structuredClone({ ...storage })).not.toThrow()
  })

  it('flush/cleanup 仍可作为方法调用', async () => {
    const storage = createPluginStorage('iso-3')
    storage.counter = 1

    await expect(storage.flush()).resolves.toBeUndefined()
    expect(localStorage.getItem(PLUGIN_STORAGE_PREFIX + 'iso-3')).toContain('"counter"')
    expect(() => storage.cleanup()).not.toThrow()
  })

  it('flush 后再修改会重新进入防抖保存', async () => {
    const storage = createPluginStorage('iso-4')
    storage.counter = 1
    await storage.flush()
    storage.counter = 2
    // 防抖 300ms
    await new Promise((resolve) => setTimeout(resolve, 350))
    expect(localStorage.getItem(PLUGIN_STORAGE_PREFIX + 'iso-4')).toContain('"counter":2')
  })

  it('生命周期方法不可被数据写入或删除覆盖', () => {
    const storage = createPluginStorage('iso-5')
    const mutable = storage as unknown as Record<string, unknown>

    // 严格模式下 set/deleteProperty 陷阱返回 false 会抛 TypeError
    expect(() => {
      mutable.flush = 1
    }).toThrow(TypeError)
    expect(() => {
      mutable.cleanup = 1
    }).toThrow(TypeError)
    expect(() => {
      delete mutable.flush
    }).toThrow(TypeError)

    expect(typeof storage.flush).toBe('function')
    expect(typeof storage.cleanup).toBe('function')

    mutable.normal = 'ok'
    expect(storage.normal).toBe('ok')
    delete mutable.normal
    expect('normal' in storage).toBe(false)
  })
})
