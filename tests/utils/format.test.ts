import { describe, expect, it } from 'vitest'
import { formatBytes, formatKbMb, formatTime } from '@/utils/format'

describe('formatTime', () => {
  it('formats minutes and seconds', () => {
    expect(formatTime(65)).toBe('1:05')
    expect(formatTime(0)).toBe('0:00')
    expect(formatTime(599)).toBe('9:59')
  })

  it('pads seconds with leading zero', () => {
    expect(formatTime(61)).toBe('1:01')
  })

  it('formats hours as h:mm:ss', () => {
    expect(formatTime(3661)).toBe('1:01:01')
    expect(formatTime(7200)).toBe('2:00:00')
  })

  it('returns 0:00 for invalid input', () => {
    expect(formatTime(NaN)).toBe('0:00')
    expect(formatTime(Infinity)).toBe('0:00')
    expect(formatTime(-1)).toBe('0:00')
  })
})

describe('formatBytes', () => {
  it('formats bytes without decimals', () => {
    expect(formatBytes(512)).toBe('512 B')
  })

  it('formats KB with one decimal', () => {
    expect(formatBytes(1536)).toBe('1.5 KB')
  })

  it('rounds up to GB and stops at the last unit', () => {
    expect(formatBytes(1024 * 1024 * 100)).toBe('100 MB')
    expect(formatBytes(1024 ** 4)).toBe('1024 GB')
  })

  it('uses integer rounding for values >= 100', () => {
    expect(formatBytes(150 * 1024)).toBe('150 KB')
  })

  it('returns -- for invalid input', () => {
    expect(formatBytes(NaN)).toBe('--')
    expect(formatBytes(-5)).toBe('--')
  })
})

describe('formatKbMb', () => {
  it('formats KB below 1MB', () => {
    expect(formatKbMb(512 * 1024)).toBe('512.00 KB')
  })

  it('formats MB above 1MB', () => {
    expect(formatKbMb(2.5 * 1024 * 1024)).toBe('2.50 MB')
  })
})
