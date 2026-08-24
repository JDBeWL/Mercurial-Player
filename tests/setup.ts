import { vi } from 'vitest'

// Mock Tauri core API
export const mockInvoke = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}))

// Mock Tauri dialog plugin
export const mockOpen = vi.fn()

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: mockOpen,
}))

// Mock Tauri store plugin
// load() 返回一个模拟的 Store 对象,支持 get/set/save
export const mockStoreGet = vi.fn()
export const mockStoreSet = vi.fn()
export const mockStoreSave = vi.fn()

vi.mock('@tauri-apps/plugin-store', () => ({
  load: vi.fn().mockResolvedValue({
    get: mockStoreGet,
    set: mockStoreSet,
    save: mockStoreSave,
  }),
}))

// Mock Tauri global-shortcut plugin
export const mockShortcutRegister = vi.fn()
export const mockShortcutUnregister = vi.fn()
export const mockShortcutUnregisterAll = vi.fn()
export const mockShortcutIsRegistered = vi.fn()

vi.mock('@tauri-apps/plugin-global-shortcut', () => ({
  register: mockShortcutRegister,
  unregister: mockShortcutUnregister,
  unregisterAll: mockShortcutUnregisterAll,
  isRegistered: mockShortcutIsRegistered,
}))

// Mock Tauri fs plugin
export const mockWriteFile = vi.fn()

vi.mock('@tauri-apps/plugin-fs', () => ({
  writeFile: mockWriteFile,
}))

// Mock Tauri http plugin
export const mockTauriFetch = vi.fn()

vi.mock('@tauri-apps/plugin-http', () => ({
  fetch: mockTauriFetch,
}))

// Mock Tauri process plugin
export const mockRelaunch = vi.fn()

vi.mock('@tauri-apps/plugin-process', () => ({
  relaunch: mockRelaunch,
}))

// Mock Tauri opener plugin (当前未直接使用,但为完整性预置)
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(),
  openPath: vi.fn(),
}))
