/**
 * Shuffle 辅助模块:Knuth 洗牌序列的生成、校验与导航计算,全部为无副作用的纯函数。
 * 状态(_shuffleOrder / _shufflePosition / _shuffleHistory)仍由 player store 持有,这里只算不存。
 */

/**
 * Knuth 洗牌：从后往前遍历 [n-1..1]，与 [0..i] 随机位交换，O(n) 且 n! 排列等概率。
 * 当前曲目会被移到序列第 0 位作为起点。playlistLength 为 0 时返回空序列。
 */
export function generateShuffleOrder(
  playlistLength: number,
  currentIndex: number,
): { order: number[]; position: number } {
  const n = playlistLength
  if (n === 0) {
    return { order: [], position: -1 }
  }

  const order = Array.from({ length: n }, (_, i) => i)

  for (let i = n - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    const tmp = order[i]!
    order[i] = order[j]!
    order[j] = tmp
  }

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

/** 洗牌序列是否仍有效：长度与当前 playlist 一致(列表变化/重置即失效)且位置已定位 */
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
 * @returns `{ index, position, order }` - order 可能在重新洗牌后变化
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

/** 优先从历史栈弹出以真正回到上一首；栈空才退化为序列前移。 */
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

  const removedPos = order.indexOf(removedIndex)

  const newOrder = order
    .filter((idx) => idx !== removedIndex)
    .map((idx) => (idx > removedIndex ? idx - 1 : idx))

  // 被删条目在当前播放位置之前时,当前条目前移一位
  let newPosition = position
  if (removedPos !== -1 && removedPos < position) {
    newPosition--
  }

  const newHistory = history
    .filter((idx) => idx !== removedIndex)
    .map((idx) => (idx > removedIndex ? idx - 1 : idx))

  return { order: newOrder, position: newPosition, history: newHistory }
}
