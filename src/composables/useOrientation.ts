import { computed, onMounted, onUnmounted, readonly, ref, type ComputedRef, type Ref } from 'vue'

/** 方向只信 `(orientation: portrait)` 媒体查询, 与样式表同源 (自己比 innerWidth/innerHeight 在软键盘弹出时会误判); 单例 + 引用计数免得每个组件重复挂同一个监听 */
const portraitQuery = '(orientation: portrait)'

function readPortrait(): boolean {
  if (typeof window === 'undefined') return false
  if (typeof window.matchMedia === 'function') {
    return window.matchMedia(portraitQuery).matches
  }
  // 降级路径: 没有 matchMedia (老 WebView / 非浏览器环境) 时才直接比宽高
  return window.innerHeight > window.innerWidth
}

const isPortrait = ref(readPortrait())
let consumers = 0

function sync(): void {
  const next = readPortrait()
  if (next !== isPortrait.value) {
    isPortrait.value = next
  }
}

function attach(): void {
  // orientationchange 是老 WebView 上的主信号, resize 是它在现代实现里的替代品: 两个都挂, 同值短路吸收重复触发
  window.addEventListener('resize', sync)
  window.addEventListener('orientationchange', sync)
}

function detach(): void {
  window.removeEventListener('resize', sync)
  window.removeEventListener('orientationchange', sync)
}

export interface OrientationInfo {
  /** 是否竖屏, 横竖屏切换时响应式更新 */
  isPortrait: Readonly<Ref<boolean>>
  /** 是否横屏 (非竖屏即横屏, 含正方形窗口) */
  isLandscape: ComputedRef<boolean>
}

/** 屏幕方向信息, 判的是方向而非平台: 桌面把窗口拉成窄高同样算竖屏。
 *  竖屏的单面板形态 (封面/歌词二选一) 只在 Android 生效, 对应 CSS 都带 [data-mobile] 守卫。 */
export function useOrientation(): OrientationInfo {
  // 挂载前先同步一次: 模块加载与组件挂载之间方向可能已经变了
  sync()

  onMounted(() => {
    consumers += 1
    if (consumers === 1) attach()
    sync()
  })

  onUnmounted(() => {
    consumers = Math.max(0, consumers - 1)
    if (consumers === 0) detach()
  })

  return {
    isPortrait: readonly(isPortrait),
    isLandscape: computed(() => !isPortrait.value),
  }
}
