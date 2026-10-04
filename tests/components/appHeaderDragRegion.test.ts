// 顶栏拖拽区护栏: 窗口无边框, 顶栏是移动窗口的唯一手段; Tauri 2 从事件目标向上遍历时, 遇到第一个带
// data-tauri-drag-region 属性的元素就 return, 空值/"true" 会短路外层 header 的 "deep"
// (headless 实测可拖宽度占比: 全空值 11.1% -> 全 "deep" 74%, 且 0 个按钮被误判)
import { describe, expect, it } from 'vitest'

const modules = import.meta.glob('@/components/AppHeader.vue', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const key = Object.keys(modules).find((k) => k.endsWith('/AppHeader.vue'))
expect(key, '未找到 AppHeader.vue').toBeTruthy()
const source = modules[key!]!

/** 所有静态写法的 `data-tauri-drag-region` 取值（动态绑定的那条 `:` 前缀的不在此列） */
const staticValues = [...source.matchAll(/(?<!:)data-tauri-drag-region(?:="([^"]*)")?/g)].map(
  (m) => m[1] ?? null,
)

describe('顶栏拖拽区', () => {
  it('整条顶栏是 deep 拖拽区（Android 不标注）', () => {
    expect(source).toContain(`:data-tauri-drag-region="isAndroid ? null : 'deep'"`)
  })

  it('不存在空值 / "true" 标注——它会把祖先的 deep 短路掉，子孙全判成非拖拽区', () => {
    const offending = staticValues.filter((v) => v === null || v === '' || v === 'true')
    expect(offending, `发现空值拖拽标注：${JSON.stringify(offending)}`).toEqual([])
  })

  it('曲目信息块是 deep：曲名/艺术家文字本身也要能拖动', () => {
    expect(source).toContain('class="nav-center" data-tauri-drag-region="deep"')
  })

  it('溢出菜单浮层标 false：否则按住菜单内边距会把窗口拖走', () => {
    const menu = /class="overflow-menu"[\s\S]{0,120}?data-tauri-drag-region="false"/.exec(source)
    expect(menu).toBeTruthy()
  })

  it('交互控件保留显式 false（按钮的点击不受 deep 影响）', () => {
    const optOuts = staticValues.filter((v) => v === 'false').length
    // 4 个窗口控制 + 3 个左侧按钮 + ThemeSelector 包裹 + 溢出触发 + 溢出菜单容器 + 2 处文字
    expect(optOuts).toBeGreaterThanOrEqual(12)
  })

  it('曲名/艺术家的文字必须排除在拖拽区外，否则框选会被 preventDefault 吃掉', () => {
    // preventDefault 的实现见 drag.js 的 mousedown 分支
    expect(source).toMatch(
      /class="nav-track-title-text selectable"[\s\S]{0,80}?data-tauri-drag-region="false"/,
    )
    const artistTags = source.match(/class="nav-track-artist selectable"[^>]*>/g) ?? []
    const artistOptOuts =
      source.match(
        /class="nav-track-artist selectable"[\s\S]{0,120}?data-tauri-drag-region="false"/g,
      ) ?? []
    expect(artistTags.length).toBeGreaterThanOrEqual(2) // 真实艺术家 + 无曲目占位
    expect(artistOptOuts.length).toBe(artistTags.length)
  })

  it('文字带 selectable：触屏下 user-select 被 body 关掉，靠这个类才能选中', () => {
    expect(source).toContain('class="nav-track-title-text selectable"')
    expect(source).toContain('class="nav-track-artist selectable"')
  })
})
