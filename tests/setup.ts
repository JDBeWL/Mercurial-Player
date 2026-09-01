import { vi } from 'vitest'
import logger from '@/utils/logger'

// 测试环境无 Tauri 后端,关闭日志落盘:否则每个 invoke('write_log')
// 都会抛 "window is not defined" 并打印一次告警,淹没真实失败信息
logger.setFileEnabled(false)

// Mock Tauri core API
export const mockInvoke = vi.fn()
// 把本地路径转成 WebView 可加载的 asset 协议 URL
export const mockConvertFileSrc = vi.fn((path: string) => `asset://localhost/${encodeURI(path)}`)

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
  convertFileSrc: mockConvertFileSrc,
}))

// Mock Tauri dialog plugin
export const mockOpen = vi.fn()
export const mockSave = vi.fn()

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: mockOpen,
  save: mockSave,
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
export const mockReadFile = vi.fn()

vi.mock('@tauri-apps/plugin-fs', () => ({
  writeFile: mockWriteFile,
  readFile: mockReadFile,
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
