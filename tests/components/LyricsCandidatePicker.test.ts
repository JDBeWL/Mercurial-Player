// @vitest-environment happy-dom
// 歌词候选选择器回归护栏：i18n 键必须在 zh/en 都存在（`common.cancel` 曾缺失、控制台刷
// "Not found"）；显示/隐藏时 Teleport 不应向 body 留下占位节点。
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import i18n from '@/i18n'
import LyricsCandidatePicker from '@/components/lyrics/LyricsCandidatePicker.vue'
// 通过 Vite 的 ?raw 读取组件源码,避免在测试里直接使用 node:fs
import pickerSource from '@/components/lyrics/LyricsCandidatePicker.vue?raw'
import zh from '@/locales/zh.json'
import en from '@/locales/en.json'

describe('LyricsCandidatePicker', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('组件用到的所有 i18n 键在中英文语言包里都存在', () => {
    const keys = [...pickerSource.matchAll(/\$t\('([^']+)'\)/g)].map((match) => match[1]!)
    expect(keys.length).toBeGreaterThan(0)

    for (const locale of [zh, en]) {
      const missing = keys.filter((key) => {
        const segments = key.split('.')
        let node: unknown = locale
        for (const segment of segments) {
          if (!node || typeof node !== 'object') return true
          node = (node as Record<string, unknown>)[segment]
        }
        return node === undefined
      })
      expect(missing).toEqual([])
    }
  })

  it('隐藏时不渲染到 body,显示时渲染弹窗且不再出现缺键告警', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const wrapper = mount(LyricsCandidatePicker, {
      props: { visible: false, loading: false, candidates: [] },
      global: { plugins: [i18n] },
    })
    await wrapper.vm.$nextTick()
    expect(document.body.querySelector('.picker-overlay')).toBeNull()

    await wrapper.setProps({ visible: true })
    await wrapper.vm.$nextTick()
    const overlay = document.body.querySelector('.picker-overlay')
    expect(overlay).not.toBeNull()
    expect(document.body.textContent).toContain(
      (zh.common as Record<string, string>).cancel as string,
    )

    const missingKeyWarnings = warnSpy.mock.calls
      .map((call) => String(call[0]))
      .filter((message) => message.includes('Not found'))
    expect(missingKeyWarnings).toEqual([])

    await wrapper.setProps({ visible: false })
    await wrapper.vm.$nextTick()
    expect(document.body.querySelector('.picker-overlay')).toBeNull()

    warnSpy.mockRestore()
    wrapper.unmount()
  })
})
