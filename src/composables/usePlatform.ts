import { computed, ref, type ComputedRef } from 'vue'
import { getPlatform } from '@/services/appService'
import logger from '@/utils/logger'

/** 运行平台（windows / macos / linux / android）。模块级单例：`get_platform` 一次运行里不
 *  会变，让每个组件各自 await 只会重复 IPC，还会带来"首帧未知"的闪烁。 */
const platformRef = ref<string | null>(null)
let pending: Promise<string> | null = null

function ensurePlatform(): Promise<string> {
  if (pending) return pending
  pending = getPlatform()
    .then((value) => {
      platformRef.value = value
      return value
    })
    .catch((err) => {
      // 取不到就当作未知：移动端专属降级不会生效，但界面仍然可用
      logger.warn('Failed to detect platform:', err)
      platformRef.value = 'unknown'
      return 'unknown'
    })
  return pending
}

export interface PlatformInfo {
  /** 平台标识；查询尚未返回时为 null */
  platform: ComputedRef<string | null>
  isAndroid: ComputedRef<boolean>
  isWindows: ComputedRef<boolean>
}

/** 平台信息。桌面专属功能（窗口控制按钮、拖拽标题栏、迷你模式、桌面歌词）在 Android 上没有
 *  意义，需按平台隐藏或降级。首次调用会自动发起一次平台查询。 */
export function usePlatform(): PlatformInfo {
  void ensurePlatform()

  return {
    platform: computed(() => platformRef.value),
    isAndroid: computed(() => platformRef.value === 'android'),
    isWindows: computed(() => platformRef.value === 'windows'),
  }
}
