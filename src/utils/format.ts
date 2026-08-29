/**
 * 通用格式化工具（纯函数，无副作用）
 */

/**
 * 格式化时间（秒 → "m:ss"，超过 1 小时 → "h:mm:ss"）
 * 非法输入（0 / NaN / Infinity 等）返回 "0:00"
 */
export const formatTime = (seconds: number): string => {
  if (!seconds || isNaN(seconds) || !isFinite(seconds)) return '0:00'

  const hours = Math.floor(seconds / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  const remainingSeconds = Math.floor(seconds % 60)

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(remainingSeconds).padStart(2, '0')}`
  }
  return `${minutes}:${String(remainingSeconds).padStart(2, '0')}`
}

/**
 * 格式化字节数为带单位的可读文本（如 12.5 MB）
 * 非法输入（非有限值 / 负数）返回 "--"
 */
export const formatBytes = (bytes: number): string => {
  if (!Number.isFinite(bytes) || bytes < 0) return '--'
  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`
}

/**
 * 格式化缓存占用字节数（KB / MB 两档，保留两位小数，单位带空格）
 */
export const formatKbMb = (bytes: number): string => {
  return bytes > 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(2)} MB`
    : `${(bytes / 1024).toFixed(2)} KB`
}
