/**
 * 通用对象工具:深拷贝、深比较
 */

/** JSON 安全的深拷贝(不保留 undefined/函数/Date 等,适合纯数据对象) */
export function deepClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

/** 深度比较两个对象是否相等 */
export function deepEqual(obj1: unknown, obj2: unknown): boolean {
  if (obj1 === obj2) return true
  if (typeof obj1 !== 'object' || typeof obj2 !== 'object') return false
  if (obj1 === null || obj2 === null) return false

  const keys1 = Object.keys(obj1 as object)
  const keys2 = Object.keys(obj2 as object)

  if (keys1.length !== keys2.length) return false

  for (const key of keys1) {
    if (!deepEqual((obj1 as Record<string, unknown>)[key], (obj2 as Record<string, unknown>)[key]))
      return false
  }
  return true
}
