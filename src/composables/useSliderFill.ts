import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'

/**
 * 计算滑块填充百分比（0-100），用于 linear-gradient 填充样式。
 * @param min 滑块最小值
 * @param max 滑块最大值
 * @param value 当前值（支持 ref / getter / 普通值）
 */
export const useSliderFill = (
  min: number,
  max: number,
  value: MaybeRefOrGetter<number>,
): ComputedRef<number> => {
  return computed(() => ((toValue(value) - min) / (max - min)) * 100)
}
