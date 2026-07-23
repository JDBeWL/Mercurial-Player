// Re-export mocks from setup
export { mockInvoke, mockOpen } from '../setup'
import { mockInvoke, mockOpen } from '../setup'

// Helper to reset all mocks
export function resetTauriMocks() {
  mockInvoke.mockReset()
  mockOpen.mockReset()
}

// Helper to setup common invoke responses
export function setupInvokeMock(command: string, response: any) {
  mockInvoke.mockImplementation((cmd: string, _args?: any) => {
    if (cmd === command) {
      return Promise.resolve(response)
    }
    return Promise.reject(new Error(`Unknown command: ${cmd}`))
  })
}

// Helper to setup multiple invoke responses
export function setupInvokeMocks(responses: Record<string, any>) {
  mockInvoke.mockImplementation((cmd: string, args?: any) => {
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
