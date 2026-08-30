<template>
  <div class="theme-selector">
    <button
      ref="toggleButtonRef"
      class="icon-button"
      :title="$t('nav.theme.switch')"
      @click="toggleColorPicker"
    >
      <span class="material-symbols-rounded">palette</span>
    </button>

    <Transition name="picker-fade">
      <div v-if="showColorPicker" ref="colorPickerRef" class="color-picker" @click.stop>
        <div class="color-picker-header">
          <h3>{{ $t('themeSelector.chooseThemeColor') }}</h3>
          <button class="close-btn" @click="showColorPicker = false">
            <span class="material-symbols-rounded">close</span>
          </button>
        </div>

        <!-- 色彩分类标签 -->
        <div class="color-categories">
          <button
            v-for="category in colorCategories"
            :key="category.id"
            class="category-chip"
            :class="{ active: activeCategory === category.id }"
            @click="activeCategory = category.id"
          >
            {{ category.name }}
          </button>
        </div>

        <!-- 颜色预设网格 -->
        <div class="color-presets">
          <div
            v-for="color in filteredColors"
            :key="color.hex"
            class="color-preset"
            :class="{ selected: themeStore.primaryColor === color.hex }"
            :style="{ backgroundColor: color.hex }"
            :title="colorName(color)"
            @click="selectColor(color.hex)"
          >
            <span
              v-if="themeStore.primaryColor === color.hex"
              class="check-icon material-symbols-rounded"
              >check</span
            >
          </div>
        </div>

        <!-- 自定义颜色 -->
        <div class="custom-color-section">
          <label for="custom-color">{{ $t('themeSelector.customColor') }}</label>
          <div class="custom-color-input">
            <input
              id="custom-color"
              type="color"
              :value="themeStore.primaryColor"
              @input="selectCustomColor"
              @change="commitCustomColor"
            />
            <input
              type="text"
              class="hex-input"
              :value="themeStore.primaryColor"
              placeholder="#000000"
              maxlength="7"
              @change="onHexInput"
            />
          </div>
        </div>

        <!-- 当前颜色预览 -->
        <div class="color-preview">
          <div class="preview-swatch" :style="{ backgroundColor: themeStore.primaryColor }"></div>
          <div class="preview-info">
            <span class="preview-label">{{ $t('themeSelector.currentColor') }}</span>
            <span class="preview-hex">{{ themeStore.primaryColor }}</span>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useThemeStore } from '../stores/theme'
import { useConfigStore } from '../stores/config'
import logger from '../utils/logger'
import { colorPresets, type ColorPreset } from '../utils/themePresets'

// 颜色分类类型
interface ColorCategory {
  id: string
  name: string
}

const { t, te } = useI18n()

// 颜色名走 i18n（key 为不含 # 的十六进制值），缺失时回退到预设名
const colorName = (color: ColorPreset): string => {
  const key = `themeSelector.colors.${color.hex.slice(1)}`
  return te(key) ? t(key) : color.name
}
const themeStore = useThemeStore()
const configStore = useConfigStore()
const showColorPicker = ref<boolean>(false)
const activeCategory = ref<string>('all')

// 颜色分类
const colorCategories = computed<ColorCategory[]>(() => [
  { id: 'all', name: t('themeSelector.category.all') },
  { id: 'blue', name: t('themeSelector.category.blue') },
  { id: 'purple', name: t('themeSelector.category.purple') },
  { id: 'pink', name: t('themeSelector.category.pink') },
  { id: 'red', name: t('themeSelector.category.red') },
  { id: 'orange', name: t('themeSelector.category.orange') },
  { id: 'green', name: t('themeSelector.category.green') },
  { id: 'monet', name: t('themeSelector.category.monet') },
  { id: 'neutral', name: t('themeSelector.category.neutral') },
])

// 颜色预设数据见 src/utils/themePresets.ts

// 根据分类筛选颜色
const filteredColors = computed<ColorPreset[]>(() => {
  if (activeCategory.value === 'all') {
    return colorPresets
  }
  return colorPresets.filter((c) => c.category === activeCategory.value)
})

const toggleColorPicker = (): void => {
  showColorPicker.value = !showColorPicker.value
}

// 点击外部关闭（通过模板 ref 定位面板与触发按钮）
const colorPickerRef = ref<HTMLElement | null>(null)
const toggleButtonRef = ref<HTMLElement | null>(null)

const handleClickOutside = (event: MouseEvent): void => {
  const picker = colorPickerRef.value
  const button = toggleButtonRef.value
  const target = event.target as Node
  if (picker && !picker.contains(target) && button && !button.contains(target)) {
    showColorPicker.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  if (previewRafId !== null) {
    cancelAnimationFrame(previewRafId)
    previewRafId = null
  }
})

// 应用主题色并按需自动保存配置到 user.json
const applyColor = async (color: string): Promise<void> => {
  themeStore.setPrimaryColor(color)

  if (configStore.general.autoSaveConfig) {
    try {
      await configStore.saveConfig()
      logger.debug('主题色已保存到 user.json')
    } catch (error) {
      logger.error('保存主题色到 user.json 失败:', error)
    }
  }
}

const selectColor = (color: string): Promise<void> => applyColor(color)

// 拖动实时预览:用 rAF 把连续 input 事件合并为每帧一次,且只做不落盘的主题应用
// 配置持久化在拖动结束；(@change)时由 commitCustomColor 统一完成
let previewRafId: number | null = null
let pendingPreviewColor = ''

const selectCustomColor = (event: Event): void => {
  pendingPreviewColor = (event.target as HTMLInputElement).value
  if (previewRafId !== null) return
  previewRafId = requestAnimationFrame(() => {
    previewRafId = null
    themeStore.setPrimaryColorLive(pendingPreviewColor)
  })
}

const commitCustomColor = (event: Event): void => {
  if (previewRafId !== null) {
    cancelAnimationFrame(previewRafId)
    previewRafId = null
  }
  void applyColor((event.target as HTMLInputElement).value)
}

const onHexInput = (event: Event): Promise<void> | undefined => {
  const value = (event.target as HTMLInputElement).value.trim()
  // 验证 HEX 颜色格式
  if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
    return applyColor(value)
  }
  return undefined
}
</script>

<style scoped>
.theme-selector {
  position: relative;
}

/* 颜色选择器面板 */
.color-picker {
  position: absolute;
  top: 60px;
  width: 360px;
  translate: -45%;
  background: var(--md-sys-color-surface);
  backdrop-filter: blur(var(--glass-blur, 12px));
  border-radius: var(--md-sys-shape-corner-large);
  box-shadow: var(--shadow-strong, var(--md-sys-elevation-level3));
  border: var(--glass-border, 1px solid rgba(255, 255, 255, 0.1));
  z-index: 1000;
  padding: 20px;
  overflow: hidden;
}

.color-picker::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--gradient-surface, none);
  pointer-events: none;
  z-index: -1;
}

.color-picker-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.color-picker-header h3 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

.close-btn {
  background: none;
  border: none;
  padding: 4px;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  border-radius: var(--md-sys-shape-corner-small);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: all 0.2s;
}

.close-btn:hover {
  background-color: var(--md-sys-color-surface-variant);
  color: var(--md-sys-color-on-surface);
}

/* 颜色分类标签 */
.color-categories {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid var(--md-sys-color-outline-variant);
}

.category-chip {
  padding: 6px 12px;
  border-radius: var(--md-sys-shape-corner-large);
  border: 1px solid var(--md-sys-color-outline-variant);
  background: transparent;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.category-chip:hover {
  background-color: var(--md-sys-color-surface-variant);
  border-color: var(--md-sys-color-outline);
}

.category-chip.active {
  background: var(--gradient-primary, var(--md-sys-color-primary));
  border-color: transparent;
  color: var(--md-sys-color-on-primary);
}

/* 颜色预设网格 */
.color-presets {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: 10px;
  margin-bottom: 20px;
  max-height: 200px;
  overflow-y: auto;
  padding: 4px;
}

.color-preset {
  width: 100%;
  aspect-ratio: 1;
  border-radius: var(--md-sys-shape-corner-medium);
  cursor: pointer;
  transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

.color-preset:hover {
  transform: scale(1.1);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  z-index: 1;
}

.color-preset.selected {
  transform: scale(1.05);
}

.check-icon {
  color: white;
  font-size: 20px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
}

/* 自定义颜色区域 */
.custom-color-section {
  margin-bottom: 16px;
}

.custom-color-section label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin-bottom: 8px;
}

.custom-color-input {
  display: flex;
  align-items: center;
  gap: 12px;
}

.custom-color-input input[type='color'] {
  width: 48px;
  height: 48px;
  border: none;
  border-radius: var(--md-sys-shape-corner-medium);
  cursor: pointer;
  padding: 0;
  background: transparent;
  overflow: hidden;
  appearance: none;
  -webkit-appearance: none;
}

.custom-color-input input[type='color']::-webkit-color-swatch-wrapper {
  padding: 0;
  margin: 0;
  border: none;
  background: transparent;
}

.custom-color-input input[type='color']::-webkit-color-swatch {
  border: none;
  border-radius: var(--md-sys-shape-corner-medium);
}

.hex-input {
  flex: 1;
  padding: 12px 16px;
  border: 1px solid var(--md-sys-color-outline-variant);
  border-radius: var(--md-sys-shape-corner-medium);
  background: var(--md-sys-color-surface-container);
  color: var(--md-sys-color-on-surface);
  font-family: inherit;
  font-size: 14px;
  transition: all 0.2s;
}

.hex-input:focus {
  outline: none;
  border-color: var(--md-sys-color-primary);
  box-shadow: 0 0 0 2px var(--primary-alpha-20, rgba(100, 181, 246, 0.2));
}

/* 颜色预览 */
.color-preview {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  background: var(--md-sys-color-surface-container);
  border-radius: var(--md-sys-shape-corner-medium);
}

.preview-swatch {
  width: 48px;
  height: 48px;
  border-radius: var(--md-sys-shape-corner-medium);
  box-shadow: var(--shadow-soft, 0 2px 8px rgba(0, 0, 0, 0.1));
}

.preview-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.preview-label {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.preview-hex {
  font-size: 16px;
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
}

/* 动画 */
.picker-fade-enter-active,
.picker-fade-leave-active {
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.picker-fade-enter-from,
.picker-fade-leave-to {
  opacity: 0;
  transform: translateY(-10px) scale(0.95);
}

/* 滚动条样式 */
.color-presets::-webkit-scrollbar {
  width: 6px;
}

.color-presets::-webkit-scrollbar-track {
  background: transparent;
}

.color-presets::-webkit-scrollbar-thumb {
  background: var(--md-sys-color-outline-variant);
  border-radius: 3px;
}

.color-presets::-webkit-scrollbar-thumb:hover {
  background: var(--md-sys-color-outline);
}
</style>
