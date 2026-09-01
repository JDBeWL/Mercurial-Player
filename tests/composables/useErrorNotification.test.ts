import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type ErrorListener = (
  error: { severity: string },
  options: { showToUser: boolean; userMessage?: string },
) => void

const errorApi = vi.hoisted(() => ({
  onError: vi.fn(),
  getUserFriendlyMessage: vi.fn(),
}))

vi.mock('@/utils/errorHandler', () => ({
  default: {
    onError: errorApi.onError,
    getUserFriendlyMessage: errorApi.getUserFriendlyMessage,
  },
  AppError: class AppError {},
  ErrorSeverity: {
    LOW: 'low',
    MEDIUM: 'medium',
    HIGH: 'high',
    CRITICAL: 'critical',
  },
}))

const { useErrorNotification } = await import('@/composables/useErrorNotification')

const MAX_NOTIFICATIONS = 5

let listener: ErrorListener | null = null
let unsubscribed = false

beforeEach(() => {
  vi.clearAllMocks()
  vi.useFakeTimers()
  listener = null
  unsubscribed = false

  errorApi.onError.mockImplementation((cb: ErrorListener) => {
    listener = cb
    return () => {
      unsubscribed = true
    }
  })
  errorApi.getUserFriendlyMessage.mockReturnValue('friendly message')
})

afterEach(() => {
  const { clearErrors, unsubscribe } = useErrorNotification()
  clearErrors()
  unsubscribe()
  vi.useRealTimers()
})

const messages = () => useErrorNotification().errorNotifications.value.map((n) => n.message)

describe('useErrorNotification > showError', () => {
  it('queues a notification with error defaults', () => {
    const { showError, errorNotifications } = useErrorNotification()

    const id = showError('boom')

    expect(errorNotifications.value).toHaveLength(1)
    expect(errorNotifications.value[0]).toMatchObject({
      id,
      message: 'boom',
      severity: 'error',
      duration: 5000,
    })
  })

  it('honours a custom severity and duration', () => {
    const { showError, errorNotifications } = useErrorNotification()

    showError('careful', 'warning', 1234)

    expect(errorNotifications.value[0]).toMatchObject({
      message: 'careful',
      severity: 'warning',
      duration: 1234,
    })
  })

  it('auto-dismisses after the duration', () => {
    const { showError, errorNotifications } = useErrorNotification()
    showError('temporary', 'info', 1000)
    expect(errorNotifications.value).toHaveLength(1)

    vi.advanceTimersByTime(999)
    expect(errorNotifications.value).toHaveLength(1)

    vi.advanceTimersByTime(1)
    expect(errorNotifications.value).toHaveLength(0)
  })

  it('keeps the notification forever when the duration is zero', () => {
    const { showError, errorNotifications } = useErrorNotification()

    showError('sticky', 'error', 0)
    vi.advanceTimersByTime(60_000)

    expect(errorNotifications.value).toHaveLength(1)
  })

  it('gives every notification a distinct id', () => {
    const { showError, errorNotifications } = useErrorNotification()

    const ids = [showError('a'), showError('b'), showError('c')]

    expect(new Set(ids).size).toBe(3)
    expect(errorNotifications.value.map((n) => n.id)).toEqual(ids)
  })

  it('caps the queue at five and drops the oldest', () => {
    const { showError, errorNotifications } = useErrorNotification()

    for (let i = 0; i < MAX_NOTIFICATIONS + 3; i++) showError(`msg ${i}`, 'error', 0)

    expect(errorNotifications.value).toHaveLength(MAX_NOTIFICATIONS)
    expect(messages()).toEqual(['msg 3', 'msg 4', 'msg 5', 'msg 6', 'msg 7'])
  })

  it('cancels the timer of a notification dropped by the cap', () => {
    const { showError } = useErrorNotification()

    // 第 1 条设为 1s 后自动关闭,随后被大量通知挤出队列
    showError('doomed', 'error', 1000)
    for (let i = 0; i < MAX_NOTIFICATIONS + 1; i++) showError(`msg ${i}`, 'error', 1000)

    const timerCountBefore = vi.getTimerCount()
    vi.advanceTimersByTime(1000)

    // 被挤出的那条定时器必须已清理,否则会误删同名通知
    expect(vi.getTimerCount()).toBeLessThan(timerCountBefore)
    expect(messages()).not.toContain('doomed')
  })
})

describe('useErrorNotification > removal', () => {
  it('removes a notification by id', () => {
    const { showError, removeError, errorNotifications } = useErrorNotification()
    const id = showError('gone', 'error', 0)

    removeError(id)

    expect(errorNotifications.value).toHaveLength(0)
  })

  it('cancels the auto-dismiss timer when removed early', () => {
    const { showError, removeError, errorNotifications } = useErrorNotification()
    const id = showError('gone', 'error', 1000)

    removeError(id)
    const timersAfterRemoval = vi.getTimerCount()
    vi.advanceTimersByTime(2000)

    expect(vi.getTimerCount()).toBe(timersAfterRemoval)
    expect(errorNotifications.value).toHaveLength(0)
  })

  it('ignores an unknown id', () => {
    const { showError, removeError, errorNotifications } = useErrorNotification()
    showError('stays', 'error', 0)

    removeError(123456)

    expect(errorNotifications.value).toHaveLength(1)
  })

  it('clears every notification and timer', () => {
    const { showError, clearErrors, errorNotifications } = useErrorNotification()
    showError('a', 'error', 1000)
    showError('b', 'error', 2000)
    showError('c', 'error', 3000)

    clearErrors()

    expect(errorNotifications.value).toHaveLength(0)
    expect(vi.getTimerCount()).toBe(0)
  })
})

describe('useErrorNotification > showSuccess', () => {
  it('uses the success severity and a shorter default duration', () => {
    const { showSuccess, errorNotifications } = useErrorNotification()

    showSuccess('saved')

    expect(errorNotifications.value[0]).toMatchObject({
      message: 'saved',
      severity: 'success',
      duration: 3000,
    })
  })

  it('merges the optional title into the message', () => {
    const { showSuccess, errorNotifications } = useErrorNotification()

    showSuccess('all good', 'Settings')

    expect(errorNotifications.value[0]?.message).toBe('Settings: all good')
  })

  it('accepts a custom duration', () => {
    const { showSuccess, errorNotifications } = useErrorNotification()

    showSuccess('quick', undefined, 500)

    expect(errorNotifications.value[0]?.duration).toBe(500)
  })

  it('auto-dismisses like any other notification', () => {
    const { showSuccess, errorNotifications } = useErrorNotification()

    showSuccess('bye', undefined, 1000)
    vi.advanceTimersByTime(1000)

    expect(errorNotifications.value).toHaveLength(0)
  })
})

describe('useErrorNotification > errorHandler bridge', () => {
  it('registers the bridge listener only once', () => {
    useErrorNotification()
    useErrorNotification()
    useErrorNotification()

    expect(errorApi.onError).toHaveBeenCalledTimes(1)
  })

  it('shows a notification when the error asks for it', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'high' }, { showToUser: true })

    expect(errorNotifications.value).toHaveLength(1)
    expect(errorNotifications.value[0]?.message).toBe('friendly message')
  })

  it('stays quiet when showToUser is false', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'high' }, { showToUser: false })

    expect(errorNotifications.value).toHaveLength(0)
  })

  it('prefers the caller-supplied user message', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'high' }, { showToUser: true, userMessage: 'custom text' })

    expect(errorNotifications.value[0]?.message).toBe('custom text')
    expect(errorApi.getUserFriendlyMessage).not.toHaveBeenCalled()
  })

  it('maps critical and high severity to error', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'critical' }, { showToUser: true, userMessage: 'a' })
    listener?.({ severity: 'high' }, { showToUser: true, userMessage: 'b' })

    expect(errorNotifications.value.map((n) => n.severity)).toEqual(['error', 'error'])
  })

  it('maps medium severity to warning', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'medium' }, { showToUser: true, userMessage: 'a' })

    expect(errorNotifications.value[0]?.severity).toBe('warning')
  })

  it('maps low severity to info', () => {
    const { errorNotifications } = useErrorNotification()

    listener?.({ severity: 'low' }, { showToUser: true, userMessage: 'a' })

    expect(errorNotifications.value[0]?.severity).toBe('info')
  })

  it('unsubscribes the bridge listener on demand', () => {
    const { unsubscribe } = useErrorNotification()

    unsubscribe()

    expect(unsubscribed).toBe(true)
  })

  it('re-registers the bridge after unsubscribing', () => {
    const first = useErrorNotification()
    first.unsubscribe()

    useErrorNotification()

    expect(errorApi.onError).toHaveBeenCalledTimes(2)
  })

  it('is safe to unsubscribe twice', () => {
    const { unsubscribe } = useErrorNotification()
    unsubscribe()

    expect(() => unsubscribe()).not.toThrow()
  })

  it('routes errors through the newly registered listener after re-subscribing', () => {
    const first = useErrorNotification()
    first.unsubscribe()

    const second = useErrorNotification()
    listener?.({ severity: 'low' }, { showToUser: true, userMessage: 'via new bridge' })

    expect(second.errorNotifications.value).toHaveLength(1)
    expect(second.errorNotifications.value[0]?.message).toBe('via new bridge')
  })
})
