// 界面字号上下限跨端镜像护栏: 前端滑块 `GeneralSettings.vue` 的 UI_FONT_SCALE_MIN/MAX 与原生 clamp
// `FontScaleBridge.kt` 的 MIN_SCALE/MAX_SCALE 各写了一份 0.8 / 1.6,
// 只改一边就是"滑块允许选、原生又收回去"的静默不一致
import { describe, expect, it } from 'vitest'

const kotlinModules = import.meta.glob(
  '@/../src-tauri/gen/android/app/src/main/java/com/jdbewl/mercurial_player/*.kt',
  { query: '?raw', import: 'default', eager: true },
) as Record<string, string>
const vueModules = import.meta.glob('@/components/settings/GeneralSettings.vue', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

const pick = (modules: Record<string, string>, suffix: string): string => {
  const key = Object.keys(modules).find((k) => k.endsWith(suffix))
  expect(key, `未找到 ${suffix}`).toBeTruthy()
  return modules[key!]!
}

const kotlinSource = pick(kotlinModules, '/FontScaleBridge.kt')
const vueSource = pick(vueModules, '/GeneralSettings.vue')

/** 从源码里取一个 `const val NAME = <数字>f`（Kotlin） */
const kotlinConst = (name: string): number => {
  const match = new RegExp(`const val ${name}\\s*=\\s*([0-9.]+)f`).exec(kotlinSource)
  expect(match, `FontScaleBridge.kt 缺少 ${name}`).toBeTruthy()
  return Number(match![1])
}

/** 从源码里取一个 `const NAME = <数字>`（TS/Vue） */
const tsConst = (name: string): number => {
  const match = new RegExp(`const ${name}\\s*=\\s*([0-9.]+)`).exec(vueSource)
  expect(match, `GeneralSettings.vue 缺少 ${name}`).toBeTruthy()
  return Number(match![1])
}

describe('界面字号上下限跨端一致性', () => {
  it('前端滑块与原生 clamp 的上下限相同', () => {
    expect(tsConst('UI_FONT_SCALE_MIN')).toBe(kotlinConst('MIN_SCALE'))
    expect(tsConst('UI_FONT_SCALE_MAX')).toBe(kotlinConst('MAX_SCALE'))
  })

  it('区间合法且包含 100%', () => {
    const min = kotlinConst('MIN_SCALE')
    const max = kotlinConst('MAX_SCALE')
    expect(min).toBeLessThan(max)
    expect(min).toBeLessThanOrEqual(1)
    expect(max).toBeGreaterThanOrEqual(1)
  })
})
