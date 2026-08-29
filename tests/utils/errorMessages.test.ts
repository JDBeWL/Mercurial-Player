import { describe, expect, it, vi } from 'vitest'
import { getErrorMessage, saveConfigSafely } from '@/utils/errorMessages'

describe('getErrorMessage', () => {
  it('extracts message from Error instances', () => {
    expect(getErrorMessage(new Error('boom'), 'fallback')).toBe('boom')
  })

  it('returns fallback for non-Error values', () => {
    expect(getErrorMessage('string error', 'fallback')).toBe('fallback')
    expect(getErrorMessage(null, 'fallback')).toBe('fallback')
    expect(getErrorMessage(undefined, 'fallback')).toBe('fallback')
  })
})

describe('saveConfigSafely', () => {
  it('delegates to saveConfigNow', async () => {
    const saveConfigNow = vi.fn().mockResolvedValue(undefined)
    await saveConfigSafely({ saveConfigNow })
    expect(saveConfigNow).toHaveBeenCalledTimes(1)
  })

  it('swallows save errors without throwing', async () => {
    const saveConfigNow = vi.fn().mockRejectedValue(new Error('disk full'))
    await expect(saveConfigSafely({ saveConfigNow })).resolves.toBeUndefined()
  })
})
