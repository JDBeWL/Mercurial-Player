import { invoke } from '@tauri-apps/api/core'

/** 获取当前平台标识（windows / macos / linux） */
export function getPlatform(): Promise<string> {
  return invoke<string>('get_platform')
}

/** 获取系统字体列表 */
export function getSystemFonts(): Promise<string[]> {
  return invoke<string[]>('get_system_fonts')
}

/** 获取字体缓存统计信息 */
export function getFontCacheStats(): Promise<{ extractCacheBytes: number }> {
  return invoke<{ extractCacheBytes: number }>('get_font_cache_stats')
}

/** 清空字体缓存，返回清理后的缓存统计 */
export function clearFontCaches(): Promise<{ extractCacheBytes: number }> {
  return invoke<{ extractCacheBytes: number }>('clear_font_caches')
}

/** 获取屏幕刷新率（Hz），取窗口当前所在显示器，跨屏后返回值跟随变化 */
export function getScreenRefreshRate(): Promise<number> {
  return invoke<number>('get_screen_refresh_rate')
}

/** 获取窗口所在显示器的刷新率挡位（当前值 + 当前分辨率支持的全部挡位） */
export function getDisplayRefreshRates(): Promise<{
  current: number
  available: number[]
}> {
  return invoke('get_display_refresh_rates')
}

/** 设置可视化目标帧率 */
export function setTargetFps(fps: number): Promise<void> {
  return invoke<void>('set_target_fps', { fps })
}

/** 使用系统默认浏览器打开外部 URL（后端有 HTTPS 白名单校验） */
export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>('open_external_url', { url })
}

/** 解析便携化数据文件完整路径（主程序同级 data/ 下,如 config.json、library-cache.json） */
export function resolveDataFile(file: string): Promise<string> {
  return invoke<string>('resolve_data_file', { file })
}

/** 获取系统基础信息（OS/架构/系统音乐目录等，开发者面板用） */
export function getSystemInfo(): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('get_system_info')
}

/** 清除上次播放会话记录（开发者排障用） */
export function clearLastSession(): Promise<void> {
  return invoke<void>('clear_last_session')
}

/** 立即将内存中的元数据缓存写入磁盘 */
export function flushMetadataCache(): Promise<void> {
  return invoke<void>('flush_metadata_cache_command')
}
