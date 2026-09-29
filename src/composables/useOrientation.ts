import { computed, onMounted, onUnmounted, readonly, ref, type ComputedRef, type Ref } from 'vue'

/** 方向只认 `(orientation: portrait)` 这一个来源，与样式表用的媒体查询完全同源，不会出现
 *  JS 与 CSS 两套判定打架；自己比 innerWidth/innerHeight 在软键盘弹出时会误判。做成模块级
 *  单例 + 引用计数，避免每个组件重复挂同一个监听。 */
const portraitQuery = '(orientation: portrait)'

function readPortrait(): boolean {
  if (typeof window === 'undefined') return false
  if (typeof window.matchMedia === 'function') {
    return window.matchMedia(portraitQuery).matches
  }
  // 极端降级：没有 matchMedia（老 WebView / 非浏览器环境）时退回直接比宽高
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
  // orientationchange 在老 WebView 上是主信号，resize 是它在现代实现里的替代品：
  // 两个都挂，任一先到都能把状态纠正过来（重复触发由 sync 的同值短路吸收）
  window.addEventListener('resize', sync)
  window.addEventListener('orientationchange', sync)
}

function detach(): void {
  window.removeEventListener('resize', sync)
  window.removeEventListener('orientationchange', sync)
}

export interface OrientationInfo {
  /** 是否竖屏；横竖屏切换时响应式更新 */
  isPortrait: Readonly<Ref<boolean>>
  /** 是否横屏（非竖屏即横屏，含正方形窗口） */
  isLandscape: ComputedRef<boolean>
}

/** 屏幕方向信息。这是**方向**判定而非平台判定：桌面把窗口拉成窄高同样算竖屏。
 *  竖屏对应的单面板形态（封面/歌词二选一）只在 Android 上生效，CSS 规则都带 [data-mobile] 守卫。 */
export function useOrientation(): OrientationInfo {
  // 挂载前先同步一次：模块首次加载与组件挂载之间方向可能已经变了
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
