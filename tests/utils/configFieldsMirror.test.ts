// 配置字段前后端镜像护栏
//
// 前端 `save_config` 把整个 AppConfig 交给后端，后端用强类型结构体反序列化；serde 会静默
// 忽略未知字段，所以结构体漏字段就会在保存时丢弃、下次启动回默认值（`lyricProviderOrder` /
// `lyricProviderSettings` / `autoSelectBestLyrics` 就这样丢过）。这里逐字段比对两边定义。
import { describe, expect, it } from 'vitest'

// 两个 glob：前端类型在 src/ 下，后端结构体在 src-tauri/ 下（后者不在 src 内，需单独匹配）
const frontendModules = import.meta.glob('@/../src/types/*.ts', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>
const rustModules = import.meta.glob('@/../src-tauri/src/config/*.rs', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const pick = (modules: Record<string, string>, suffix: string): string => {
  const key = Object.keys(modules).find((k) => k.endsWith(suffix))
  expect(key, `未找到 ${suffix}`).toBeTruthy()
  return modules[key!]!
}

const typesSource = pick(frontendModules, '/src/types/index.ts')
const rustSource = pick(rustModules, '/src-tauri/src/config/manager.rs')

/** TS 接口字段（去掉注释，忽略方法签名） */
const tsFields = (name: string): string[] | null => {
  const match = new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`).exec(typesSource)
  if (!match) return null
  const body = match[1]!.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '')
  return [...body.matchAll(/^ {2}([A-Za-z_]\w*)\??\s*:/gm)].map((m) => m[1]!)
}

/** Rust 结构体字段（snake_case） */
const rustFields = (name: string): string[] | null => {
  const match = new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`).exec(rustSource)
  if (!match) return null
  return [...match[1]!.matchAll(/pub (\w+):/g)].map((m) => m[1]!)
}

/** snake_case → camelCase（Rust 侧统一 `#[serde(rename_all = "camelCase")]`） */
const toCamel = (snake: string): string => {
  const parts = snake.split('_')
  return (
    parts[0]! +
    parts
      .slice(1)
      .map((p) => p.charAt(0).toUpperCase() + p.slice(1))
      .join('')
  )
}

/** 前端类型 → 后端结构体（个别类型名不同，如 UIConfig ⇄ UiConfig） */
const SECTIONS: Array<[string, string]> = [
  ['AppConfig', 'AppConfig'],
  ['UIConfig', 'UiConfig'],
  ['VisualizerConfig', 'VisualizerConfig'],
  ['DirectoryScanConfig', 'DirectoryScanConfig'],
  ['TrackSnapshot', 'TrackSnapshot'],
  ['TitleExtractionConfig', 'TitleExtractionConfig'],
  ['PlaylistConfig', 'PlaylistConfig'],
  ['GeneralConfig', 'GeneralConfig'],
  ['AudioConfig', 'AudioConfig'],
  ['LyricsConfig', 'LyricsConfig'],
  ['ProviderLyricSetting', 'ProviderLyricSetting'],
  ['DesktopLyricsConfig', 'DesktopLyricsConfig'],
]

/** 允许后端不持久化的字段：UI 面板开关是临时的，保存前会显式复位，后端 UiConfig 不该存 */
const FRONTEND_ONLY: string[] = ['UIConfig.showSettings', 'UIConfig.showConfigPanel']

describe('配置字段前后端镜像', () => {
  it('前端声明的配置字段后端结构体都有（否则保存时会被静默丢弃）', () => {
    const missing: string[] = []

    for (const [tsName, rsName] of SECTIONS) {
      const front = tsFields(tsName)
      const back = rustFields(rsName)
      expect(front, `types/index.ts 缺少 interface ${tsName}`).not.toBeNull()
      expect(back, `manager.rs 缺少 struct ${rsName}`).not.toBeNull()

      const backCamel = new Set(back!.map(toCamel))
      for (const field of front!) {
        if (backCamel.has(field)) continue
        const path = `${tsName}.${field}`
        if (FRONTEND_ONLY.includes(path)) continue
        missing.push(`${path} ← 后端 struct ${rsName} 缺字段(或命名不一致)`)
      }
    }

    expect(missing).toEqual([])
  })

  it('歌词来源相关字段确实在后端结构体里', () => {
    const back = rustFields('LyricsConfig')!.map(toCamel)
    for (const field of ['autoSelectBestLyrics', 'lyricProviderOrder', 'lyricProviderSettings']) {
      expect(back, `LyricsConfig 缺少 ${field}`).toContain(field)
    }
  })
})
