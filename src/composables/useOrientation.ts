import { computed, onMounted, onUnmounted, readonly, ref, type ComputedRef, type Ref } from 'vue'

/**
 * 屏幕方向判定（竖屏 = 宽度 < 高度）。
 *
 * 只认 `(orientation: portrait)` 这一个来源：它与样式表里用的媒体查询**完全同源**，
 * 不会出现"JS 认为竖屏、CSS 认为横屏"两套判定打架、布局与交互对不上的情况。
 * （用 innerWidth/innerHeight 自己比大小也可以，但那等于把方向语义抄了两遍，
 * 且软键盘弹出时 innerHeight 会缩水、存在误判成横屏的风险。）
 *
 * 做成模块级单例 + 引用计数：一次运行里方向是全局状态，
 * 每个组件各自挂监听只会让同一个 resize 被重复处理。
 */
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

/**
 * 屏幕方向信息。
 *
 * 竖屏下播放主界面会切成"单面板"形态（封面 / 歌词二选一占满上部区域），
 * 但横屏与桌面窗口仍沿用左封面 + 右歌词的双栏布局，所以这是**方向**而非平台判定，
 * 桌面把窗口拉成窄高同样会走竖屏形态。
 */
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
