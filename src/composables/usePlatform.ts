import { computed, ref, type ComputedRef } from 'vue'
import { getPlatform } from '@/services/appService'
import logger from '@/utils/logger'

/** 运行平台标识: windows / macos / linux / android / unknown, 取自 Rust 侧 get_platform 命令 */
const platformRef = ref<string | null>(null)
let pending: Promise<string> | null = null

// get_platform 的结果一次运行里不会变, 故做模块级单例: 各自 await 只会重复 IPC, 还带来首帧未知的闪烁
function ensurePlatform(): Promise<string> {
  if (pending) return pending
  pending = getPlatform()
    .then((value) => {
      platformRef.value = value
      return value
    })
    .catch((err) => {
      // 查询失败当作 unknown: 移动端专属降级不会生效, 但界面仍然可用
      logger.warn('Failed to detect platform:', err)
      platformRef.value = 'unknown'
      return 'unknown'
    })
  return pending
}

export interface PlatformInfo {
  /** 平台标识, 查询尚未返回时为 null */
  platform: ComputedRef<string | null>
  isAndroid: ComputedRef<boolean>
  isWindows: ComputedRef<boolean>
}

/** 平台信息: 桌面专属功能 (窗口控制按钮、拖拽标题栏、迷你模式、桌面歌词) 在 Android 上要隐藏或降级; 首次调用即发起查询 */
export function usePlatform(): PlatformInfo {
  void ensurePlatform()

  return {
    platform: computed(() => platformRef.value),
    isAndroid: computed(() => platformRef.value === 'android'),
    isWindows: computed(() => platformRef.value === 'windows'),
  }
}
