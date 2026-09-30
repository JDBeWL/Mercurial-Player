/**
 * Shuffle 逻辑辅助模块
 *
 * 从 player.ts 抽离的纯函数,负责 Knuth 洗牌序列的生成、校验和导航计算。
 * 状态 (_shuffleOrder / _shufflePosition / _shuffleHistory) 仍保留在 player store 中,
 * 本模块仅提供无副作用的状态计算逻辑,由 store 调用后赋值。
 */

/**
 * 用 Knuth (Fisher-Yates-Knuth) 算法生成洗牌顺序
 * 算法: 从后往前遍历 [n-1..1], 每次从 [0..i] 中随机取一个与 i 交换
 * 时间复杂度 O(n), 空间 O(n), 保证 n! 种排列等概率出现
 *
 * 以当前曲目为起点: 把当前 index 放到序列第 0 位,只对剩余 n-1 首洗牌
 *
 * @param playlistLength 播放列表长度
 * @param currentIndex 当前曲目索引 (-1 表示无)
 * @returns `{ order, position }`,playlistLength 为 0 时返回空序列
 */
export function generateShuffleOrder(
  playlistLength: number,
  currentIndex: number,
): { order: number[]; position: number } {
  const n = playlistLength
  if (n === 0) {
    return { order: [], position: -1 }
  }

  // 1. 生成 [0, 1, ..., n-1]
  const order = Array.from({ length: n }, (_, i) => i)

  // 2. Knuth shuffle: for i = n-1 downto 1, swap(order[i], order[rand(0..i)])
  for (let i = n - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    const tmp = order[i]!
    order[i] = order[j]!
    order[j] = tmp
  }

  // 3. 以当前曲目为起点:把 currentIndex 移到第 0 位
  if (currentIndex >= 0 && currentIndex < n) {
    const curPos = order.indexOf(currentIndex)
    if (curPos > 0) {
      const tmp = order[0]!
      order[0] = order[curPos]!
      order[curPos] = tmp
    }
  }

  return { order, position: 0 }
}

/**
 * 校验洗牌顺序是否仍然有效
 * 失效条件: 序列长度与当前 playlist 不一致 (playlist 变化/重置)
 */
export function isShuffleOrderValid(
  order: number[],
  position: number,
  playlistLength: number,
): boolean {
  return order.length === playlistLength && order.length > 0 && position >= 0
}

/**
 * 计算 nextTrack 在 shuffle 模式下的下一首索引
 *
 * @param order 当前洗牌序列
 * @param position 当前在序列中的位置
 * @param playlistLength 播放列表长度
 * @param currentIndex 当前曲目索引 (用于在序列失效时重新生成)
 * @returns `{ index, position, order }` — order 可能在重新洗牌后变化
 */
export function getNextShuffleIndex(
  order: number[],
  position: number,
  playlistLength: number,
  currentIndex: number,
): { index: number; position: number; order: number[] } {
  if (playlistLength <= 1) {
    return { index: 0, position, order }
  }

  // 懒生成:第一次或顺序失效时重新洗牌
  let currentOrder = order
  let currentPosition = position
  if (!isShuffleOrderValid(currentOrder, currentPosition, playlistLength)) {
    const result = generateShuffleOrder(playlistLength, currentIndex)
    currentOrder = result.order
    currentPosition = result.position
  }

  // 走到末尾:重新洗牌继续 (手动触发时不应停止)
  if (currentPosition >= currentOrder.length - 1) {
    const result = generateShuffleOrder(playlistLength, currentIndex)
    currentOrder = result.order
    currentPosition = 0
    return { index: currentOrder[0]!, position: currentPosition, order: currentOrder }
  }

  currentPosition++
  return { index: currentOrder[currentPosition]!, position: currentPosition, order: currentOrder }
}

/**
 * 计算 previousTrack 在 shuffle 模式下的上一首索引
 *
 * @param order 当前洗牌序列
 * @param position 当前在序列中的位置
 * @param history 历史栈
 * @param playlistLength 播放列表长度
 * @param currentIndex 当前曲目索引 (用于在序列失效时重新生成)
 * @returns `{ index, position, order, history }`
 */
export function getPreviousShuffleIndex(
  order: number[],
  position: number,
  history: number[],
  playlistLength: number,
  currentIndex: number,
): { index: number; position: number; order: number[]; history: number[] } {
  if (playlistLength <= 1) {
    return { index: 0, position, order, history }
  }

  const currentHistory = [...history]

  // 优先从历史栈弹出,真正回到上一首
  if (currentHistory.length > 0) {
    const prevIndex = currentHistory.pop()!
    let newPosition = position
    if (newPosition > 0) newPosition--
    return { index: prevIndex, position: newPosition, order, history: currentHistory }
  }

  // 历史栈空:走到洗牌序列上一首
  let currentOrder = order
  let currentPosition = position
  if (!isShuffleOrderValid(currentOrder, currentPosition, playlistLength)) {
    const result = generateShuffleOrder(playlistLength, currentIndex)
    currentOrder = result.order
    currentPosition = result.position
  }

  if (currentPosition > 0) {
    currentPosition--
    return {
      index: currentOrder[currentPosition]!,
      position: currentPosition,
      order: currentOrder,
      history: currentHistory,
    }
  }

  // 在起点之前:回绕到序列末尾
  currentPosition = currentOrder.length - 1
  return {
    index: currentOrder[currentPosition]!,
    position: currentPosition,
    order: currentOrder,
    history: currentHistory,
  }
}

/**
 * 在播放列表移除条目后,校正洗牌序列和历史栈
 *
 * @param order 当前洗牌序列
 * @param position 当前位置
 * @param history 历史栈
 * @param removedIndex 被移除的条目索引
 * @returns 校正后的 `{ order, position, history }`
 */
export function adjustShuffleAfterRemove(
  order: number[],
  position: number,
  history: number[],
  removedIndex: number,
): { order: number[]; position: number; history: number[] } {
  if (order.length === 0) {
    return { order: [], position, history: [...history] }
  }

  // 被删 index 在 _shuffleOrder 中的位置
  const removedPos = order.indexOf(removedIndex)

  // 从序列中移除该 index,并将大于该 index 的所有值减 1
  const newOrder = order
    .filter((idx) => idx !== removedIndex)
    .map((idx) => (idx > removedIndex ? idx - 1 : idx))

  // 校正 position:若被删条目位于当前播放位置之前,当前条目前移一位
  let newPosition = position
  if (removedPos !== -1 && removedPos < position) {
    newPosition--
  }

  // 校正历史栈:移除被删 index,并将大于该 index 的值减 1
  const newHistory = history
    .filter((idx) => idx !== removedIndex)
    .map((idx) => (idx > removedIndex ? idx - 1 : idx))

  return { order: newOrder, position: newPosition, history: newHistory }
}
