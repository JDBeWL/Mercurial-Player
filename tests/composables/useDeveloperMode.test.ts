// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest'
import { useDeveloperMode } from '@/composables/useDeveloperMode'

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
