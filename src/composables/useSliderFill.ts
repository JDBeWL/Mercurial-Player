import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue'

export const useSliderFill = (
  min: number,
  max: number,
  value: MaybeRefOrGetter<number>,
): ComputedRef<number> => {
  return computed(() => ((toValue(value) - min) / (max - min)) * 100)
}
