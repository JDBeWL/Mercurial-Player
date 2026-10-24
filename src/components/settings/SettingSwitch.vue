<script setup lang="ts">
/** MD3 风格设置开关 / `button[role=switch]` + `v-model`；未监听 `update` 时点击仍冒泡 */
const props = defineProps<{
  modelValue: boolean
  /** 禁用态: 降透明度且不响应自身点击 */
  disabled?: boolean
  /** 读屏标签 (开关自身无可见文本时建议提供) */
  ariaLabel?: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
}>()

function toggle(): void {
  if (!props.disabled) {
    emit('update:modelValue', !props.modelValue)
  }
}
</script>

<template>
  <button
    type="button"
    class="switch"
    :class="{ active: modelValue }"
    role="switch"
    :aria-checked="modelValue"
    :aria-label="ariaLabel"
    :disabled="disabled"
    @click="toggle"
  >
    <span class="switch-track"></span>
    <span class="switch-handle"></span>
  </button>
</template>

<style scoped>
.switch {
  position: relative;
  width: 52px;
  height: 28px;
  flex-shrink: 0;
  cursor: pointer;
  border: none;
  padding: 0;
  background: none;
  display: block;
}

.switch:disabled {
  opacity: 0.38;
  cursor: not-allowed;
}

.switch-track {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--md-sys-color-surface-container-highest);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 14px;
  transition: all 0.2s ease;
}

.switch.active .switch-track {
  background-color: var(--md-sys-color-primary);
  border-color: var(--md-sys-color-primary);
}

.switch-handle {
  position: absolute;
  top: 50%;
  left: 6px;
  transform: translateY(-50%);
  width: 16px;
  height: 16px;
  background-color: var(--md-sys-color-outline);
  border-radius: 50%;
  transition: all 0.2s ease;
}

.switch.active .switch-handle {
  left: 28px;
  width: 18px;
  height: 18px;
  background-color: var(--md-sys-color-on-primary);
}
</style>
