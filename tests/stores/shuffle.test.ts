import { describe, expect, it, vi, afterEach } from 'vitest'
import {
  generateShuffleOrder,
  isShuffleOrderValid,
  getNextShuffleIndex,
  getPreviousShuffleIndex,
  adjustShuffleAfterRemove,
} from '@/stores/shuffle'

describe('generateShuffleOrder', () => {
  it('returns an empty sequence for an empty playlist', () => {
    expect(generateShuffleOrder(0, -1)).toEqual({ order: [], position: -1 })
  })

  it('produces a permutation containing every index exactly once', () => {
    const { order, position } = generateShuffleOrder(50, -1)
    expect(order).toHaveLength(50)
    expect([...order].sort((a, b) => a - b)).toEqual(Array.from({ length: 50 }, (_, i) => i))
    expect(position).toBe(0)
  })

  it('puts the current track at the head of the sequence', () => {
    for (let i = 0; i < 20; i++) {
      const { order } = generateShuffleOrder(10, 7)
      expect(order[0]).toBe(7)
    }
  })

  it('handles a single-track playlist', () => {
    expect(generateShuffleOrder(1, 0)).toEqual({ order: [0], position: 0 })
  })

  it('ignores an out-of-range current index', () => {
    const { order } = generateShuffleOrder(5, 99)
    expect(order).toHaveLength(5)
    expect([...order].sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4])
  })
})

describe('isShuffleOrderValid', () => {
  it('accepts a matching order with a non-negative position', () => {
    expect(isShuffleOrderValid([0, 1, 2], 1, 3)).toBe(true)
  })

  it('rejects a length mismatch', () => {
    expect(isShuffleOrderValid([0, 1], 0, 3)).toBe(false)
  })

  it('rejects an empty order', () => {
    expect(isShuffleOrderValid([], 0, 0)).toBe(false)
  })

  it('rejects a negative position', () => {
    expect(isShuffleOrderValid([0, 1, 2], -1, 3)).toBe(false)
  })
})

describe('getNextShuffleIndex', () => {
  it('returns index 0 unchanged for a playlist of one or zero tracks', () => {
    expect(getNextShuffleIndex([], 0, 1, 0)).toEqual({ index: 0, position: 0, order: [] })
    expect(getNextShuffleIndex([], 0, 0, -1)).toEqual({ index: 0, position: 0, order: [] })
  })

  it('regenerates the order when it has become invalid', () => {
    const result = getNextShuffleIndex([0, 1], 0, 5, 3)
    expect(result.order).toHaveLength(5)
    expect(result.order[0]).toBe(3)
    expect(result.position).toBe(1)
    expect(result.index).toBe(result.order[1])
  })

  it('advances the position within a valid order', () => {
    const result = getNextShuffleIndex([4, 1, 9, 2], 0, 4, 4)
    expect(result.position).toBe(1)
    expect(result.index).toBe(1)
    expect(result.order).toEqual([4, 1, 9, 2])
  })

  it('reshuffles when reaching the end of the sequence', () => {
    const result = getNextShuffleIndex([4, 1, 9, 2], 3, 4, 2)
    expect(result.position).toBe(0)
    expect(result.order).toHaveLength(4)
    expect(result.order[0]).toBe(2)
    expect(result.index).toBe(2)
  })
})

describe('getPreviousShuffleIndex', () => {
  it('short-circuits for a playlist of one or zero tracks', () => {
    expect(getPreviousShuffleIndex([], 0, [3], 1, 0)).toEqual({
      index: 0,
      position: 0,
      order: [],
      history: [3],
    })
  })

  it('pops from the history stack first and does not mutate the input', () => {
    const history = [5, 8]
    const result = getPreviousShuffleIndex([0, 1, 2], 2, history, 3, 2)
    expect(result.index).toBe(8)
    expect(result.position).toBe(1)
    // 原数组未被修改
    expect(history).toEqual([5, 8])
    expect(result.history).toEqual([5])
  })

  it('does not decrement the position below zero when popping from history', () => {
    const result = getPreviousShuffleIndex([0, 1, 2], 0, [7], 3, 0)
    expect(result.index).toBe(7)
    expect(result.position).toBe(0)
  })

  it('regenerates the order when invalid and steps back through it', () => {
    const result = getPreviousShuffleIndex([], 0, [], 5, 4)
    expect(result.order).toHaveLength(5)
    expect(result.order[0]).toBe(4)
    // 位置 0 之前 => 回绕到序列末尾
    expect(result.position).toBe(4)
    expect(result.index).toBe(result.order[4])
  })

  it('steps back one position when not at the head of the sequence', () => {
    const result = getPreviousShuffleIndex([4, 1, 9, 2], 2, [], 4, 9)
    expect(result.position).toBe(1)
    expect(result.index).toBe(1)
    expect(result.order).toEqual([4, 1, 9, 2])
  })
})

describe('adjustShuffleAfterRemove', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('is a no-op for an empty order', () => {
    expect(adjustShuffleAfterRemove([], 2, [1, 2], 3)).toEqual({
      order: [],
      position: 2,
      history: [1, 2],
    })
  })

  it('removes the index and decrements greater indexes', () => {
    const result = adjustShuffleAfterRemove([0, 3, 1, 4], 0, [], 3)
    expect(result.order).toEqual([0, 1, 3])
  })

  it('shifts the position when the removed entry precedes the current one', () => {
    // 3 位于序列下标 1,当前位置 3 => 前移一位
    const result = adjustShuffleAfterRemove([0, 3, 1, 4], 3, [], 3)
    expect(result.position).toBe(2)
  })

  it('keeps the position when the removed entry follows the current one', () => {
    const result = adjustShuffleAfterRemove([3, 0, 1, 4], 0, [], 3)
    expect(result.position).toBe(0)
  })

  it('does not shift the position when the index is absent from the order', () => {
    const result = adjustShuffleAfterRemove([0, 1, 2], 2, [], 9)
    expect(result.position).toBe(2)
    expect(result.order).toEqual([0, 1, 2])
  })

  it('rewrites the history stack the same way', () => {
    const result = adjustShuffleAfterRemove([0, 1, 2], 0, [0, 2, 5], 2)
    expect(result.history).toEqual([0, 4])
  })
})
