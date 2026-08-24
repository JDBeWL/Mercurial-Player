// Re-export mocks from setup
export {
  mockInvoke,
  mockOpen,
  mockStoreGet,
  mockStoreSet,
  mockStoreSave,
  mockShortcutRegister,
  mockShortcutUnregister,
  mockShortcutUnregisterAll,
  mockShortcutIsRegistered,
  mockWriteFile,
  mockTauriFetch,
  mockRelaunch,
} from '../setup'
import {
  mockInvoke,
  mockOpen,
  mockStoreGet,
  mockStoreSet,
  mockStoreSave,
  mockShortcutRegister,
  mockShortcutUnregister,
  mockShortcutUnregisterAll,
  mockShortcutIsRegistered,
  mockWriteFile,
  mockTauriFetch,
  mockRelaunch,
} from '../setup'

// Helper to reset all mocks
export function resetTauriMocks() {
  mockInvoke.mockReset()
  mockOpen.mockReset()
  mockStoreGet.mockReset()
  mockStoreSet.mockReset()
  mockStoreSave.mockReset()
  mockShortcutRegister.mockReset()
  mockShortcutUnregister.mockReset()
  mockShortcutUnregisterAll.mockReset()
  mockShortcutIsRegistered.mockReset()
  mockWriteFile.mockReset()
  mockTauriFetch.mockReset()
  mockRelaunch.mockReset()
}

// Helper to setup common invoke responses
export function setupInvokeMock(command: string, response: unknown) {
  mockInvoke.mockImplementation((cmd: string, _args?: unknown) => {
    if (cmd === command) {
      return Promise.resolve(response)
    }
    return Promise.reject(new Error(`Unknown command: ${cmd}`))
  })
}

// Helper to setup multiple invoke responses
export function setupInvokeMocks(responses: Record<string, unknown>) {
  mockInvoke.mockImplementation((cmd: string, args?: unknown) => {
    if (cmd in responses) {
      const response = responses[cmd]
      if (typeof response === 'function') {
        return Promise.resolve(response(args))
      }
      return Promise.resolve(response)
    }
    return Promise.reject(new Error(`Unknown command: ${cmd}`))
  })
}
