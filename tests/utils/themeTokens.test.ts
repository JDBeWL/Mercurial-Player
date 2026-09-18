// 主题 token 完整性护栏：组件引用的 `var(--md-sys-color-*)` 若没有来源，声明会在运行时
// 静默失效（background-color 退回 transparent）。歌词候选选择器就因 surface-container-high
// 未被输出却写了深色兜底而踩过坑。
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { argbFromHex, themeFromSourceColor } from '@material/material-color-utilities'

/** 主题库实际输出的角色色 → CSS 变量名 */
const toKebab = (key: string): string => key.replace(/([a-z])([A-Z])/g, '$1-$2').toLowerCase()

// 与 theme store 同一调用方式(先 argbFromHex 再生成),确保这里枚举的正是应用真实拿到的 token
const themeScheme = themeFromSourceColor(argbFromHex('#6750a4')).schemes.light.toJSON()
const themeTokens = new Set(Object.keys(themeScheme).map((key) => `--md-sys-color-${toKebab(key)}`))

// style.css 原文。测试环境下 `.css` 导入会被 Vitest 替换成空模块（`?raw` 同样），故直接读文件
const styleCss = readFileSync('src/style.css', 'utf8')
const definedTokens = new Set(
  [...styleCss.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((match) => match[1]!),
)

/** src 下所有组件/脚本源码（取原文扫描 var() 引用）。.css 在测试里读不到内容，故不纳入。 */
const sources = import.meta.glob('@/../src/**/*.{vue,ts}', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const stripComments = (text: string): string => text.replace(/\/\*[\s\S]*?\*\//g, '')

/**
 * 本应用主题**不存在**的 MD3 容器层级角色（只输出 29 个基础角色色）。仍有 19 个文件按
 * MD3 语义引用它们，声明失效后表现为"透明"，而界面观感正是按透明设计的：补上真实颜色会
 * 让各区域拿到不同层级色而出现接缝（"统一补齐 7 个容器层级色"那版即如此，已回退）。
 * 这里登记为"已知缺失"：允许存在，但不允许新增引用（见棘轮断言）。
 */
const KNOWN_ABSENT_ROLES = new Set([
  '--md-sys-color-surface-container',
  '--md-sys-color-surface-container-low',
  '--md-sys-color-surface-container-high',
  '--md-sys-color-surface-container-highest',
  '--md-sys-color-surface-container-lowest',
  '--md-sys-color-surface-dim',
  '--md-sys-color-surface-bright',
])

/** 引用上述角色的文件数基线：只应随清理而下降 */
const KNOWN_ABSENT_FILE_BUDGET = 19

const usedTokens = new Map<string, string[]>()
for (const [file, raw] of Object.entries(sources)) {
  for (const match of stripComments(raw).matchAll(/var\(\s*(--md-sys-color-[a-z0-9-]+)/g)) {
    const token = match[1]!
    usedTokens.set(token, [...(usedTokens.get(token) ?? []), file.replace('/src/', 'src/')])
  }
}

describe('主题 token 完整性', () => {
  it('组件引用的每个 --md-sys-color-* 都有来源（或已登记的已知缺失角色）', () => {
    const missing = [...usedTokens.entries()]
      .filter(([token]) => !themeTokens.has(token) && !definedTokens.has(token))
      .filter(([token]) => !KNOWN_ABSENT_ROLES.has(token))
      .map(([token, files]) => `${token} ← ${files.join(', ')}`)

    expect(missing).toEqual([])
  })

  it('对已知缺失的容器层级色的引用只减不增', () => {
    const files = new Set<string>()
    for (const [token, tokenFiles] of usedTokens) {
      if (!KNOWN_ABSENT_ROLES.has(token)) continue
      for (const file of tokenFiles) files.add(file)
    }
    expect(
      files.size,
      '引用缺失的容器层级色的文件数不应超过基线；新增引用请改用 surface / surface-variant / hover-overlay 等真实存在的 token',
    ).toBeLessThanOrEqual(KNOWN_ABSENT_FILE_BUDGET)
  })

  it('容器层级色没有被就地补变量（补齐前需要先做视觉回归）', () => {
    for (const token of KNOWN_ABSENT_ROLES) {
      expect(
        definedTokens.has(token),
        `${token} 又被定义了。补齐容器层级色会让多个区域拿到不同底色而产生接缝，请先做一次跨界面视觉回归（App.css / Settings / SettingsNav 等），再更新本清单与断言`,
      ).toBe(false)
    }
  })
})
