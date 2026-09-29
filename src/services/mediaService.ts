import { convertFileSrc, invoke } from '@tauri-apps/api/core'

// convertFileSrc 是纯路径转换工具（不发起 IPC），随媒体类调用一并从这里导出，
// 组件经此使用就不必直接 import @tauri-apps/api/core（存量文件里仍有不少直接 import）
export { convertFileSrc }

/** 获取曲目封面缓存路径（无封面时返回 null） */
export function getTrackCoverPath(path: string): Promise<string | null> {
  return invoke<string | null>('get_track_cover_path', { path })
}

/** 获取系统临时目录路径 */
export function getTempDir(): Promise<string> {
  return invoke<string>('get_temp_dir_command')
}

/** 获取元数据缓存统计信息 [条目数, 字节数] */
export function getMetadataCacheStats(): Promise<[number, number]> {
  return invoke<[number, number]>('get_metadata_cache_stats_command')
}

/** 清空元数据缓存 */
export function clearMetadataCache(): Promise<void> {
  return invoke<void>('clear_metadata_cache_command')
}

/** 按大小上限清理封面缓存，返回清理的文件数（未传上限时由后端使用默认值） */
export function cleanCoverCache(maxCacheSizeMb: number | undefined): Promise<number> {
  return invoke<number>('clean_cover_cache_command', { maxCacheSizeMb })
}

/** 设置封面缓存目录（null 表示恢复默认路径） */
export function setCoverCachePath(path: string | null): Promise<void> {
  return invoke<void>('set_cover_cache_path_command', { path })
}
