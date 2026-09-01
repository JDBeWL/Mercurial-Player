// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Node 26+ 在全局内置了懒加载的 localStorage getter(未启用 --localstorage-file 时
// 读取为 undefined),happy-dom 不会覆盖内置全局;补一个内存实现保证行为一致。
// 被测模块在模块顶层读取 localStorage,shim 必须先于模块导入就位,故用动态导入。
if (typeof localStorage === 'undefined') {
  const backing = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => (backing.has(key) ? (backing.get(key) as string) : null),
    setItem: (key: string, value: string) => void backing.set(key, String(value)),
    removeItem: (key: string) => void backing.delete(key),
    clear: () => void backing.clear(),
  })
}

const { useDeveloperMode } = await import('@/composables/useDeveloperMode')

const STORAGE_KEY = 'mercurial-player.developer-mode'

describe('useDeveloperMode', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('defaults to off when nothing is stored', () => {
    // 模块级状态在首次导入时读取 localStorage,这里重置为已知值
    const { developerMode, setDeveloperMode } = useDeveloperMode()
    setDeveloperMode(false)
    expect(developerMode.value).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('persists an enabled flag to localStorage', () => {
    const { developerMode, setDeveloperMode } = useDeveloperMode()

    setDeveloperMode(true)

    expect(developerMode.value).toBe(true)
    expect(localStorage.getItem(STORAGE_KEY)).toBe('true')
  })

  it('removes the flag again when disabled', () => {
    const { developerMode, setDeveloperMode } = useDeveloperMode()
    setDeveloperMode(true)

    setDeveloperMode(false)

    expect(developerMode.value).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('shares one state across every call site', () => {
    const first = useDeveloperMode()
    const second = useDeveloperMode()

    first.setDeveloperMode(true)

    expect(second.developerMode.value).toBe(true)
    second.setDeveloperMode(false)
    expect(first.developerMode.value).toBe(false)
  })
})
