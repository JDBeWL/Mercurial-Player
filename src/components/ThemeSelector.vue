<template>
  <div class="theme-selector" :data-mobile="isAndroid ? 'true' : undefined">
    <button
      ref="toggleButtonRef"
      class="icon-button"
      :title="$t('nav.theme.switch')"
      @click="toggleColorPicker"
    >
      <span class="material-symbols-rounded">palette</span>
    </button>

    <!-- 挂到 body：面板在 .nav-bar 内会受那层 stacking context 限制（竖屏下 .nav-bar 是
         relative + z-index:100），音乐库 / 播放列表抽屉是 fixed + z-index:1000，一开就压住它 -->
    <Teleport to="body" :disabled="!isPortrait">
      <Transition name="picker-fade">
        <div v-if="showColorPicker" ref="colorPickerRef" class="color-picker" @click.stop>
          <div class="color-picker-header">
            <h3>{{ $t('themeSelector.chooseThemeColor') }}</h3>
            <button class="close-btn" @click="showColorPicker = false">
              <span class="material-symbols-rounded">close</span>
            </button>
          </div>

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

          <div class="color-preview">
            <div class="preview-swatch" :style="{ backgroundColor: themeStore.primaryColor }"></div>
            <div class="preview-info">
              <span class="preview-label">{{ $t('themeSelector.currentColor') }}</span>
              <span class="preview-hex">{{ themeStore.primaryColor }}</span>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useThemeStore } from '../stores/theme'
import { useConfigStore } from '../stores/config'
import { usePlatform } from '../composables/usePlatform'
import { useOrientation } from '../composables/useOrientation'
import logger from '../utils/logger'
import { colorPresets, type ColorPreset } from '../utils/themePresets'

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
// 藏触发按钮需要平台守卫：方向媒体查询在桌面窄高窗口也会命中（理由见下方 isPortrait 注释）
const { isAndroid } = usePlatform()
// 抽屉形态与 Teleport 只看方向：isAndroid 来自异步 IPC，取不到会兜底成 unknown -> false，
// 用它守卫就退回相对按钮的绝对定位形态，窄屏上向右溢出屏幕。
// 藏触发按钮反而必须带平台守卫：桌面窗口拉成窄高时它是第二个入口。
const { isPortrait } = useOrientation()
const showColorPicker = ref<boolean>(false)
const activeCategory = ref<string>('all')

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

const filteredColors = computed<ColorPreset[]>(() => {
  if (activeCategory.value === 'all') {
    return colorPresets
  }
  return colorPresets.filter((c) => c.category === activeCategory.value)
})

const toggleColorPicker = (): void => {
  showColorPicker.value = !showColorPicker.value
}

// 供外部打开面板：手机竖屏下触发按钮收进了 AppHeader 的溢出菜单，桌面仍走按钮 toggle
const open = (): void => {
  showColorPicker.value = true
}

// 点击外部关闭：靠模板 ref 判断 target 是否落在面板 / 触发按钮之外
const colorPickerRef = ref<HTMLElement | null>(null)
const toggleButtonRef = ref<HTMLElement | null>(null)

/** 必须监听 pointerdown 而不是 click：竖屏下面板由顶栏溢出菜单调 open() 打开，那次点击的 target
 *  既不在面板内也不在被隐藏的触发按钮内，监听 click 会在同一次点击的冒泡阶段把刚挂上的面板当成
 *  点了外面而关掉。pointerdown 早于 click，那时面板还没打开，关掉只是空操作。 */
const handleClickOutside = (event: PointerEvent): void => {
  const picker = colorPickerRef.value
  const button = toggleButtonRef.value
  const target = event.target as Node
  if (picker && !picker.contains(target) && button && !button.contains(target)) {
    showColorPicker.value = false
  }
}

onMounted(() => {
  document.addEventListener('pointerdown', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('pointerdown', handleClickOutside)
  if (previewRafId !== null) {
    cancelAnimationFrame(previewRafId)
    previewRafId = null
  }
})

// 应用主题色；开了自动保存才落盘到 user.json
const applyColor = async (color: string): Promise<void> => {
  void themeStore.setPrimaryColor(color)

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

// 拖动实时预览：rAF 把连续的 input 合并成每帧一次，且只应用主题不落盘；
// 持久化留到拖动结束的 @change，由 commitCustomColor 统一做
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
  // 只接受 #RRGGBB，非法输入忽略
  if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
    return applyColor(value)
  }
  return undefined
}

defineExpose({ open })
</script>

<style scoped>
.theme-selector {
  position: relative;
}

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

@media (hover: hover) {
  .close-btn:hover {
    background-color: var(--md-sys-color-surface-variant);
    color: var(--md-sys-color-on-surface);
  }
}

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

@media (hover: hover) {
  .category-chip:hover {
    background-color: var(--md-sys-color-surface-variant);
    border-color: var(--md-sys-color-outline);
  }
}

.category-chip.active {
  background: var(--gradient-primary, var(--md-sys-color-primary));
  border-color: transparent;
  color: var(--md-sys-color-on-primary);
}

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

@media (hover: hover) {
  .color-preset:hover {
    transform: scale(1.1);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
    z-index: 1;
  }
}

.color-preset.selected {
  transform: scale(1.05);
}

.check-icon {
  color: white;
  font-size: 20px;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.5);
}

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

.picker-fade-enter-active,
.picker-fade-leave-active {
  transition: all 0.25s cubic-bezier(0.4, 0, 0.2, 1);
}

.picker-fade-enter-from,
.picker-fade-leave-to {
  opacity: 0;
  transform: translateY(-10px) scale(0.95);
}

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

@media (hover: hover) {
  .color-presets::-webkit-scrollbar-thumb:hover {
    background: var(--md-sys-color-outline);
  }
}

/* 手机竖屏：触发按钮收进顶栏溢出菜单，面板改成贴底抽屉（相对按钮绝对定位在窄屏上会向右溢出）。
   守卫分两段（藏按钮带 data-mobile、抽屉只看方向）的理由见脚本里 isPortrait 注释。 */
@media (orientation: portrait) {
  .theme-selector[data-mobile='true'] {
    /* 盒子消失：不再占栅格列（面板已改 fixed，不再需要它当定位祖先） */
    display: contents;
  }

  .theme-selector[data-mobile='true'] > .icon-button {
    display: none;
  }

  .color-picker {
    position: fixed;
    top: auto;
    right: 0;
    bottom: 0;
    left: 0;
    /* 抽屉要盖住顶栏(100)与底部玻璃控制条，所以层级给到 1200 */
    z-index: 1200;
    width: auto;
    /* 先写 vh 再写 dvh：老 WebView(<108) 不认 dvh，只写后者会让 max-height 整条失效 */
    max-height: 86vh;
    max-height: 86dvh;
    translate: none;
    overflow-y: auto;
    overscroll-behavior: contain;
    border-radius: 20px 20px 0 0;
    padding: 20px 16px calc(16px + env(safe-area-inset-bottom, 0px));
  }

  /* 抽屉从下往上滑，与桌面端从按钮下方掉下来的缩放动画区分。位移只给 24px 而不是 100%：过渡被打断时
     元素会停在 enter-from 态，100% 会把整个抽屉停在屏幕外，24px 只是偏下一点、仍然完全可用。 */
  .picker-fade-enter-from,
  .picker-fade-leave-to {
    opacity: 0;
    transform: translateY(24px);
  }

  /* 分类标签由换行改成单行横滚：抽屉高度有限，换行会白吃两行 */
  .color-categories {
    flex-wrap: nowrap;
    overflow-x: auto;
    padding-bottom: 12px;
    scrollbar-width: none;
  }

  .color-categories::-webkit-scrollbar {
    display: none;
  }

  .category-chip {
    flex-shrink: 0;
  }

  .color-presets {
    max-height: 40vh;
  }
}
</style>
