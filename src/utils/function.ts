/** 通用函数工具（防抖等） */

/** 防抖：连续调用只执行最后一次；返回带 `cancel()` 的包装，供卸载时清理。 */
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
