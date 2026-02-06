<template>
  <div ref="wrapperRef" class="md3-select-wrapper" :class="{ 'is-open': isOpen, 'is-focused': isFocused }">
    <div 
      ref="triggerRef"
      class="md3-select-trigger"
      @click="toggleDropdown"
      @focus="handleFocus"
      @keydown.esc.prevent="close"
      tabindex="0"
      :aria-expanded="isOpen"
      :aria-haspopup="true"
    >
      <span class="md3-select-value">{{ displayValue }}</span>
      <span class="md3-select-icon" :class="{ 'is-open': isOpen }">
        <span class="material-symbols-rounded">expand_more</span>
      </span>
    </div>
    <Transition name="dropdown">
      <div v-if="isOpen" class="md3-select-dropdown" @click.stop>
        <div class="md3-select-dropdown-scroll">
          <div 
            v-for="option in options" 
            :key="option.value"
            class="md3-select-option"
            :class="{ 'is-selected': modelValue === option.value }"
            @click="selectOption(option.value)"
          >
            <span class="md3-select-option-text">{{ option.label }}</span>
            <span v-if="modelValue === option.value" class="md3-select-option-check">
              <span class="material-symbols-rounded">check</span>
            </span>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'

const props = defineProps({
  modelValue: {
    type: [String, Number],
    required: true
  },
  options: {
    type: Array,
    required: true,
    validator: (options) => {
      return options.every(opt => opt.value !== undefined && opt.label !== undefined)
    }
  },
  placeholder: {
    type: String,
    default: ''
  }
})

const emit = defineEmits(['update:modelValue', 'change'])

const isOpen = ref(false)
const isFocused = ref(false)
const triggerRef = ref(null)
const wrapperRef = ref(null)
const instanceId = ref(
  (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function')
    ? crypto.randomUUID()
    : `md3sel_${Math.random().toString(36).slice(2)}`
)

const displayValue = computed(() => {
  const selectedOption = props.options.find(opt => opt.value === props.modelValue)
  return selectedOption ? selectedOption.label : props.placeholder
})

const toggleDropdown = () => {
  if (isOpen.value) {
    close()
    return
  }

  // 打开前先通知其它 MD3Select 关闭（常规交互：一次只展开一个）
  window.dispatchEvent(new CustomEvent('md3-select-open', { detail: { id: instanceId.value } }))
  isOpen.value = true
}

const selectOption = (value) => {
  emit('update:modelValue', value)
  emit('change', value)
  close()
}

const handleFocus = () => {
  isFocused.value = true
}

const close = () => {
  isOpen.value = false
  isFocused.value = false
}

const handleClickOutside = (event) => {
  // 如果点到了别的下拉菜单/页面任意区域：关闭当前下拉
  if (wrapperRef.value && !wrapperRef.value.contains(event.target)) {
    close()
    triggerRef.value?.blur?.()
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
  window.addEventListener('md3-select-open', handleOtherSelectOpen)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  window.removeEventListener('md3-select-open', handleOtherSelectOpen)
})

const handleOtherSelectOpen = (event) => {
  const otherId = event?.detail?.id
  if (otherId && otherId !== instanceId.value) {
    close()
  }
}
</script>

<style scoped>
.md3-select-wrapper {
  position: relative;
  min-width: 160px;
}

.md3-select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  padding-right: 40px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: var(--md-sys-shape-corner-small);
  background-color: var(--md-sys-color-surface-container-low);
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  font-weight: 400;
  font-family: 'Roboto', sans-serif;
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.2, 0, 0, 1);
  outline: none;
  box-shadow: none;
  min-height: 48px;
  box-sizing: border-box;
}

.md3-select-trigger:hover {
  border-color: var(--md-sys-color-on-surface);
  background-color: var(--md-sys-color-surface-container);
}

.md3-select-wrapper.is-focused .md3-select-trigger,
.md3-select-trigger:focus {
  border-color: var(--md-sys-color-primary);
  background-color: var(--md-sys-color-surface-container);
  box-shadow: 0 0 0 1px var(--md-sys-color-primary);
}

.md3-select-wrapper.is-open .md3-select-trigger {
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
  border-bottom-color: transparent;
  z-index: 1001;
  position: relative;
}

.md3-select-value {
  flex: 1;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.md3-select-icon {
  position: absolute;
  right: 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--md-sys-color-on-surface-variant);
  transition: transform 0.2s cubic-bezier(0.2, 0, 0, 1);
  pointer-events: none;
}

.md3-select-icon .material-symbols-rounded {
  font-size: 24px;
  font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24;
}

.md3-select-wrapper.is-open .md3-select-icon {
  transform: rotate(180deg);
  color: var(--md-sys-color-primary);
}

.md3-select-wrapper.is-focused .md3-select-icon {
  color: var(--md-sys-color-primary);
}

.md3-select-dropdown {
  position: absolute;
  top: calc(100% - 1px);
  left: 0;
  right: 0;
  z-index: 1000;
  background-color: var(--md-sys-color-surface);
  border: 1px solid var(--md-sys-color-outline);
  border-top: none;
  border-radius: 0 0 var(--md-sys-shape-corner-small) var(--md-sys-shape-corner-small);
  box-shadow: var(--md-sys-elevation-level2);
  /* 外层只负责圆角裁切：滚动交给内层，避免滚动条侵占边框/顶部拼接区 */
  overflow: hidden;
  padding-top: 1px;
}

.md3-select-dropdown-scroll {
  max-height: 240px;
  overflow-y: auto;
  background-color: var(--md-sys-color-surface-container-low);
}

.md3-select-option {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  min-height: 48px;
  box-sizing: border-box;
  cursor: pointer;
  transition: background-color 0.15s cubic-bezier(0.2, 0, 0, 1);
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  font-weight: 400;
  font-family: 'Roboto', sans-serif;
  position: relative;
  background-color: transparent;
}

.md3-select-option::before {
  content: '';
  position: absolute;
  inset: 0;
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  opacity: 0;
  transition: opacity 0.15s cubic-bezier(0.2, 0, 0, 1);
  pointer-events: none;
}

.md3-select-option:hover::before {
  opacity: 1;
}

.md3-select-option:active::before {
  opacity: 1;
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 12%, transparent);
}

.md3-select-option.is-selected {
  background-color: var(--md-sys-color-primary-container);
  color: var(--md-sys-color-on-primary-container);
}

.md3-select-option.is-selected::before {
  background-color: color-mix(in srgb, var(--md-sys-color-on-primary-container) 8%, transparent);
}

.md3-select-option.is-selected:active::before {
  background-color: color-mix(in srgb, var(--md-sys-color-on-primary-container) 12%, transparent);
}

.md3-select-option-text {
  flex: 1;
  text-align: left;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.md3-select-option-check {
  display: flex;
  align-items: center;
  justify-content: center;
  margin-left: 8px;
  color: var(--md-sys-color-primary);
  flex-shrink: 0;
}

.md3-select-option.is-selected .md3-select-option-check {
  color: var(--md-sys-color-on-primary-container);
}

.md3-select-option-check .material-symbols-rounded {
  font-size: 20px;
  font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 20;
}

/* 下拉动画 */
.dropdown-enter-active {
  transition: all 0.2s cubic-bezier(0.2, 0, 0, 1);
}

.dropdown-leave-active {
  transition: all 0.15s cubic-bezier(0.2, 0, 0, 1);
}

.dropdown-enter-from {
  opacity: 0;
  transform: translateY(-8px);
}

.dropdown-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

/* 滚动条样式 */
.md3-select-dropdown-scroll::-webkit-scrollbar {
  width: 8px;
}

.md3-select-dropdown-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.md3-select-dropdown-scroll::-webkit-scrollbar-thumb {
  background: var(--md-sys-color-outline-variant);
  border-radius: 4px;
}

.md3-select-dropdown-scroll::-webkit-scrollbar-thumb:hover {
  background: var(--md-sys-color-outline);
}
</style>

