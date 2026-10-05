// @vitest-environment happy-dom
// 歌词候选选择器回归护栏：i18n 键必须在 zh/en 都存在（`common.cancel` 曾缺失、控制台刷
// "Not found"）；显示/隐藏时 Teleport 不应向 body 留下占位节点。
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import i18n from '@/i18n'
import LyricsCandidatePicker from '@/components/lyrics/LyricsCandidatePicker.vue'
import type { LyricCandidate } from '@/services/lyrics'
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

  // 逐字候选的 bundle 是 ASS，纯文本预览看上去和普通候选一样；预览必须按字渲染才能挑得出来
  it('逐字候选的预览按字渲染并标出逐字', async () => {
    const ass = [
      '[Script Info]',
      'ScriptType: v4.00+',
      '',
      '[Events]',
      'Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text',
      'Dialogue: 0,00:00:24.81,00:00:27.96,orig,,0,0,0,,{\\kf63}这{\\kf30}一{\\kf39}路',
    ].join('\n')
    const candidates: LyricCandidate[] = [
      {
        id: '1',
        title: 'Pick',
        artist: '',
        album: '',
        duration_ms: 0,
        bundle: { lrc: '[00:24.81]这一路', karaoke: ass },
        provider: 'netease',
        method: 'webapi',
      },
    ]

    const wrapper = mount(LyricsCandidatePicker, {
      props: { visible: true, loading: false, candidates, track: null },
      global: { plugins: [i18n] },
    })
    // ASS 解析是 async 的，flushPromises 走完微任务
    await flushPromises()

    const units = document.body.querySelectorAll('.word-unit')
    expect(units).toHaveLength(3)
    expect(units[0]!.textContent).toBe('这')
    expect(document.body.querySelector('.row-time')?.textContent).toBe('[00:24.81]')
    expect(document.body.textContent).toContain(
      (zh.lyrics as Record<string, string>).wordSynced as string,
    )
    // 文本类型选择器对逐字候选同样有效（kind 会裁掉 ASS 里不要的 ts/roma 行），所以必须在
    expect(document.body.querySelector('.md3-select-wrapper')).not.toBeNull()

    await wrapper.setProps({ visible: false })
    wrapper.unmount()
  })

  it('隐藏时不渲染到 body,显示时渲染弹窗且不再出现缺键告警', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const wrapper = mount(LyricsCandidatePicker, {
      props: { visible: false, loading: false, candidates: [], track: null },
      global: { plugins: [i18n] },
    })
    await wrapper.vm.$nextTick()
    expect(document.body.querySelector('.md3-dialog-overlay')).toBeNull()

    await wrapper.setProps({ visible: true })
    await wrapper.vm.$nextTick()
    const overlay = document.body.querySelector('.md3-dialog-overlay')
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
    expect(document.body.querySelector('.md3-dialog-overlay')).toBeNull()

    warnSpy.mockRestore()
    wrapper.unmount()
  })
})
