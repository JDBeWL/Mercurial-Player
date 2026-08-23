// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import MD3Select from '../../src/components/MD3Select.vue'

describe('MD3Select 分组选项', () => {
  it('渲染分组标题且不可点击', async () => {
    const wrapper = mount(MD3Select, {
      props: {
        modelValue: 'a',
        options: [
          { label: '组1', options: [{ value: 'a', label: '选项A' }] },
          { label: '组2', options: [{ value: 'b', label: '选项B' }] },
        ],
      },
    })
    await wrapper.find('.md3-select-trigger').trigger('click')
    const groups = wrapper.findAll('.md3-select-group-label')
    expect(groups).toHaveLength(2)
    expect(groups[0].text()).toBe('组1')
    expect(groups[0].attributes('class')).not.toContain('md3-select-option')
    await groups[0].trigger('click')
    expect(wrapper.emitted('update:modelValue')).toBeUndefined()
  })

  it('点击分组内选项可以正常选择', async () => {
    const wrapper = mount(MD3Select, {
      props: {
        modelValue: 'a',
        options: [
          { label: '组1', options: [{ value: 'a', label: '选项A' }] },
          { label: '组2', options: [{ value: 'b', label: '选项B' }, { value: 'c', label: '选项C' }] },
        ],
      },
    })
    await wrapper.find('.md3-select-trigger').trigger('click')
    const options = wrapper.findAll('.md3-select-option')
    expect(options).toHaveLength(3)
    await options[2].trigger('click')
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual(['c'])
    expect(wrapper.emitted('change')?.[0]).toEqual(['c'])
  })

  it('选中分组内选项时触发器显示对应 label', () => {
    const wrapper = mount(MD3Select, {
      props: {
        modelValue: 'b',
        options: [
          { label: '组1', options: [{ value: 'a', label: '选项A' }] },
          { label: '组2', options: [{ value: 'b', label: '选项B' }] },
        ],
      },
    })
    expect(wrapper.find('.md3-select-value').text()).toBe('选项B')
  })

  it('平铺选项仍然可用（向后兼容）', async () => {
    const wrapper = mount(MD3Select, {
      props: { modelValue: 1, options: [{ value: 1, label: '一' }, { value: 2, label: '二' }] },
    })
    await wrapper.find('.md3-select-trigger').trigger('click')
    const options = wrapper.findAll('.md3-select-option')
    expect(options).toHaveLength(2)
    await options[1].trigger('click')
    expect(wrapper.emitted('update:modelValue')?.[0]).toEqual([2])
  })
})
