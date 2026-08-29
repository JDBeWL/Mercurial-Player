/**
 * 通用函数工具:防抖等
 */

/**
 * 防抖函数(带取消功能)。
 * 连续调用时只执行最后一次;返回的函数带 `cancel()` 用于清理(如组件卸载时)。
 */
export interface DebouncedFunction<T extends (...args: unknown[]) => unknown> {
  (...args: Parameters<T>): void
  cancel: () => void
}

export function debounce<T extends (...args: unknown[]) => unknown>(
  func: T,
  wait: number,
): DebouncedFunction<T> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const debounced = function (this: unknown, ...args: Parameters<T>) {
    clearTimeout(timeout)
    timeout = setTimeout(() => func.apply(this, args), wait)
  } as DebouncedFunction<T>
  debounced.cancel = () => clearTimeout(timeout)
  return debounced
}
