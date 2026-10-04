// 顶栏曲目信息宽度约束护栏: 曲名/艺术家居中, 两侧是定宽按钮组, 中间列宽度须由两侧实际占宽动态推导;
// 边列下限给 0 时中间列会长到曲名 max-content, 把按钮组挤到标题底下 (实测边列 109px -> 208px / 184px)
import { describe, expect, it } from 'vitest'

const modules = import.meta.glob('@/components/AppHeader.vue', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const key = Object.keys(modules).find((k) => k.endsWith('/AppHeader.vue'))
expect(key, '未找到 AppHeader.vue').toBeTruthy()
const source = modules[key!]!

/** 取出第 `index` 个 `grid-template-columns` 的声明值（文件里共 3 条：横屏 / 窄屏两行 / 竖屏） */
const gridColumnsAt = (index: number): string => {
  const all = [...source.matchAll(/grid-template-columns:\s*([^;]+);/g)]
  const found = all[index]
  expect(found, `AppHeader.vue 缺少第 ${index + 1} 条 grid-template-columns`).toBeTruthy()
  // 折行书写，统一压成单行便于断言
  return found![1]!.replace(/\s+/g, ' ').trim()
}

/** 取出某个选择器的规则体（取第一个匹配，够用：这些选择器在文件里各只出现一次） */
const ruleBody = (selector: string): string => {
  const start = source.indexOf(`${selector} {`)
  expect(start, `AppHeader.vue 缺少 ${selector} 规则`).toBeGreaterThanOrEqual(0)
  const end = source.indexOf('}', start)
  return source.slice(start, end)
}

describe('顶栏曲目信息的宽度约束', () => {
  it('横屏：两侧列下限是 min-content，中间列由左右两侧实际占宽动态决定', () => {
    const cols = gridColumnsAt(0)
    // 两侧各一份 min-content 下限：按钮组是定宽内容，不允许被压到比自身窄
    expect(cols.match(/minmax\(min-content, 1fr\)/g)?.length).toBe(2)
    expect(cols).toContain('minmax(0, auto)')
    // 下限一旦回到 0 就会重现"标题压住按钮"
    expect(cols).not.toContain('minmax(0, 1fr)')
  })

  it('横屏：中间列的两侧必须是对称的 1fr（标题才能钉在窗口正中）', () => {
    const cols = gridColumnsAt(0)
    expect(cols.startsWith('minmax(min-content, 1fr)')).toBe(true)
    expect(cols.endsWith('minmax(min-content, 1fr)')).toBe(true)
  })

  it('竖屏：两侧用 auto（= 内容宽，各 40px 对称），中间列拿走剩余空间', () => {
    const cols = gridColumnsAt(2)
    expect(cols).toBe('auto minmax(0, 1fr) auto')
  })

  it('窄屏两行布局：第一行左列同样不能压到比按钮组窄', () => {
    const cols = gridColumnsAt(1)
    expect(cols.startsWith('minmax(min-content, 1fr)')).toBe(true)
    expect(cols).not.toBe('1fr auto')
  })

  it('.nav-center 必须是 border-box：否则 padding 会让它比栅格列宽出 16px', () => {
    const body = ruleBody('.nav-center')
    expect(body).toContain('box-sizing: border-box')
    // padding 两侧各 8px, 合计就是标题里的 16px 溢出量
    expect(body).toContain('padding: 0 8px')
    expect(body).toContain('max-width: 100%')
  })

  it('曲名与艺术家都带省略号收尾（宽度被限住后靠它表达"还有内容"）', () => {
    expect(source).toMatch(/\.nav-track-title-text \{[^}]*text-overflow: ellipsis/)
    expect(source).toMatch(/\.nav-track-artist \{[^}]*text-overflow: ellipsis/)
  })
})
