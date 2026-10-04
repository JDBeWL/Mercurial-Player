// 音频扩展名白名单跨端镜像护栏: 同一处判定写在三边 - Rust `filesystem.rs` 的 AUDIO_EXTENSIONS、
// Kotlin `SafBridge.kt` 的 AUDIO_EXTS、前端 `fileUtils.ts` 的 audioExtensions,
// 任一边漂移都会扫不到文件或认了播不了, 所以用会红的用例钉住一致性
import { describe, expect, it } from 'vitest'

const rustModules = import.meta.glob('@/../src-tauri/src/media/filesystem.rs', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>
const kotlinModules = import.meta.glob(
  '@/../src-tauri/gen/android/app/src/main/java/com/jdbewl/mercurial_player/SafBridge.kt',
  { query: '?raw', import: 'default', eager: true },
) as Record<string, string>
const tsModules = import.meta.glob('@/utils/fileUtils.ts', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const pick = (modules: Record<string, string>, suffix: string): string => {
  const key = Object.keys(modules).find((k) => k.endsWith(suffix))
  expect(key, `未找到 ${suffix}`).toBeTruthy()
  return modules[key!]!
}

const rustSource = pick(rustModules, '/media/filesystem.rs')
const kotlinSource = pick(kotlinModules, '/SafBridge.kt')
const tsSource = pick(tsModules, '/fileUtils.ts')

/**
 * 取一段声明里的全部带引号字面量: 三份声明都是跨行书写的, 先按 "起始标记 -> 结束括号" 切出区间再收集,
 * 避免和同文件里其他字符串数组串味
 */
const quotedLiterals = (source: string, pattern: RegExp, label: string): string[] => {
  const match = pattern.exec(source)
  expect(match, `${label} 的声明没找到（改了名字就要同步本用例）`).toBeTruthy()
  return [...match![1]!.matchAll(/["']([a-z0-9]+)["']/g)].map((m) => m[1]!)
}

const sorted = (values: string[]): string[] => [...values].sort()

const rustExtensions = sorted(
  quotedLiterals(
    rustSource,
    /AUDIO_EXTENSIONS:\s*&\[&str\]\s*=\s*&\[([\s\S]*?)\]/,
    'Rust AUDIO_EXTENSIONS',
  ),
)
const kotlinExtensions = sorted(
  quotedLiterals(kotlinSource, /AUDIO_EXTS\s*=\s*setOf\(([\s\S]*?)\)/, 'Kotlin AUDIO_EXTS'),
)
const tsExtensions = sorted(
  quotedLiterals(tsSource, /const audioExtensions\s*=\s*\[([\s\S]*?)\]/, 'TS audioExtensions'),
)

describe('音频扩展名白名单跨端一致性', () => {
  it('三份白名单要素齐全（正则没失配）', () => {
    expect(rustExtensions.length).toBeGreaterThan(0)
    expect(kotlinExtensions.length).toBeGreaterThan(0)
    expect(tsExtensions.length).toBeGreaterThan(0)
  })

  it('Rust 与 Kotlin 一致', () => {
    expect(kotlinExtensions).toEqual(rustExtensions)
  })

  it('前端与 Rust 一致', () => {
    expect(tsExtensions).toEqual(rustExtensions)
  })

  it('只收解码器真能解开的格式，且不含 wma', () => {
    // symphonia 带 mp3/flac/vorbis/aac/alac/pcm/adpcm 解码器与
    // riff(wav/aiff)/ogg/isomp4/caf/mkv 解复用器，没有 opus / ape / wma
    for (const unsupported of ['wma', 'opus', 'ape']) {
      expect(rustExtensions, `解码器不支持 ${unsupported}，白名单里不该有它`).not.toContain(
        unsupported,
      )
    }
    for (const supported of ['mp3', 'flac', 'wav', 'ogg', 'm4a', 'aac', 'aiff', 'aif', 'caf']) {
      expect(rustExtensions, `解码器支持 ${supported}，白名单里应有它`).toContain(supported)
    }
  })
})
