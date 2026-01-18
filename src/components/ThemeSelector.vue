<template>
  <div class="theme-selector">
    <button class="icon-button" @click="toggleColorPicker">
      <span class="material-symbols-rounded">palette</span>
    </button>
    
    <Transition name="picker-fade">
      <div class="color-picker" v-if="showColorPicker" @click.stop>
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
            :title="color.name"
            @click="selectColor(color.hex)"
          >
            <span v-if="themeStore.primaryColor === color.hex" class="check-icon material-symbols-rounded">check</span>
          </div>
        </div>
        
        <!-- 自定义颜色 -->
        <div class="custom-color-section">
          <label for="custom-color">{{ $t('themeSelector.customColor') }}</label>
          <div class="custom-color-input">
            <input 
              type="color" 
              id="custom-color" 
              :value="themeStore.primaryColor" 
              @input="selectCustomColor"
            />
            <input 
              type="text" 
              class="hex-input"
              :value="themeStore.primaryColor"
              @change="onHexInput"
              placeholder="#000000"
              maxlength="7"
            />
          </div>
        </div>
        
        <!-- 当前颜色预览 -->
        <div class="color-preview">
          <div class="preview-swatch" :style="{ backgroundColor: themeStore.primaryColor }"></div>
          <div class="preview-info">
            <span class="preview-label">当前主题色</span>
            <span class="preview-hex">{{ themeStore.primaryColor }}</span>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useThemeStore } from '../stores/theme'
import { useConfigStore } from '../stores/config'
import logger from '../utils/logger'

const themeStore = useThemeStore()
const configStore = useConfigStore()
const showColorPicker = ref(false)
const activeCategory = ref('all')

// 颜色分类
const colorCategories = [
  { id: 'all', name: '全部' },
  { id: 'blue', name: '蓝色系' },
  { id: 'purple', name: '紫色系' },
  { id: 'pink', name: '粉色系' },
  { id: 'red', name: '红色系' },
  { id: 'orange', name: '橙色系' },
  { id: 'green', name: '绿色系' },
  { id: 'neutral', name: '中性色' },
]

// 精选颜色预设（带名称和分类）- 使用用户直观可见的颜色
const colorPresets = [
  // 蓝色系 - 清新、专业、信任
  { hex: '#64B5F6', name: '天空蓝', category: 'blue' },
  { hex: '#42A5F5', name: '晴空蓝', category: 'blue' },
  { hex: '#0288D1', name: '月村手毬', category: 'blue' },
  { hex: '#2196F3', name: '经典蓝', category: 'blue' },
  { hex: '#1E88E5', name: '海洋蓝', category: 'blue' },
  { hex: '#1976D2', name: '深海蓝', category: 'blue' },
  { hex: '#0097A7', name: '青绿蓝', category: 'blue' },
  { hex: '#00ACC1', name: '水鸭蓝', category: 'blue' },
  { hex: '#00BCD4', name: '青色', category: 'blue' },
  { hex: '#26C6DA', name: '浅青色', category: 'blue' },
  
  // 紫色系 - 神秘、优雅、创意
  { hex: '#7C4DFF', name: '电光紫', category: 'purple' },
  { hex: '#651FFF', name: '深紫', category: 'purple' },
  { hex: '#6200EA', name: '靛紫', category: 'purple' },
  { hex: '#7E57C2', name: '薰衣草紫', category: 'purple' },
  { hex: '#673AB7', name: '经典紫', category: 'purple' },
  { hex: '#5E35B1', name: '深薰衣草', category: 'purple' },
  { hex: '#512DA8', name: '皇家紫', category: 'purple' },
  { hex: '#536DFE', name: '靛蓝紫', category: 'purple' },
  { hex: '#3D5AFE', name: '明亮靛蓝', category: 'purple' },
  { hex: '#304FFE', name: '深靛蓝', category: 'purple' },
  
  // 粉色系 - 浪漫、温柔、活力
  { hex: '#E67EA5', name: '杏山和纱', category: 'pink' },
  { hex: '#F48FB1', name: '樱花粉', category: 'pink' },
  { hex: '#F06292', name: '玫瑰粉', category: 'pink' },
  { hex: '#EC407A', name: '亮粉', category: 'pink' },
  { hex: '#E91E63', name: '经典粉', category: 'pink' },
  { hex: '#D81B60', name: '深玫瑰', category: 'pink' },
  { hex: '#C2185B', name: '酒红粉', category: 'pink' },
  { hex: '#AD1457', name: '深酒红', category: 'pink' },
  { hex: '#FF4081', name: '霓虹粉', category: 'pink' },
  { hex: '#F50057', name: '亮玫红', category: 'pink' },
  { hex: '#E040FB', name: '紫粉', category: 'pink' },
  
  // 红色系 - 热情、活力、警示
  { hex: '#EF5350', name: '珊瑚红', category: 'red' },
  { hex: '#F44336', name: '经典红', category: 'red' },
  { hex: '#E53935', name: '鲜红', category: 'red' },
  { hex: '#D32F2F', name: '深红', category: 'red' },
  { hex: '#C62828', name: '暗红', category: 'red' },
  { hex: '#B71C1C', name: '酒红', category: 'red' },
  { hex: '#FF5252', name: '亮红', category: 'red' },
  { hex: '#FF1744', name: '霓虹红', category: 'red' },
  { hex: '#D50000', name: '纯红', category: 'red' },
  
  // 橙色系 - 温暖、活泼、创意
  { hex: '#FF7043', name: '珊瑚橙', category: 'orange' },
  { hex: '#FF5722', name: '深橙', category: 'orange' },
  { hex: '#F4511E', name: '烈焰橙', category: 'orange' },
  { hex: '#E64A19', name: '暗橙', category: 'orange' },
  { hex: '#FF9800', name: '经典橙', category: 'orange' },
  { hex: '#FB8C00', name: '南瓜橙', category: 'orange' },
  { hex: '#F57C00', name: '深南瓜', category: 'orange' },
  { hex: '#FFA726', name: '杏橙', category: 'orange' },
  { hex: '#FFB300', name: '琥珀', category: 'orange' },
  { hex: '#FFC107', name: '金黄', category: 'orange' },
  
  // 绿色系 - 自然、健康、成长
  { hex: '#66BB6A', name: '草绿', category: 'green' },
  { hex: '#4CAF50', name: '经典绿', category: 'green' },
  { hex: '#43A047', name: '森林绿', category: 'green' },
  { hex: '#388E3C', name: '深森林', category: 'green' },
  { hex: '#2E7D32', name: '暗绿', category: 'green' },
  { hex: '#00E676', name: '霓虹绿', category: 'green' },
  { hex: '#00C853', name: '亮绿', category: 'green' },
  { hex: '#009688', name: '青绿', category: 'green' },
  { hex: '#00897B', name: '深青绿', category: 'green' },
  { hex: '#26A69A', name: '薄荷绿', category: 'green' },
  
  // 中性色 - 稳重、专业、简约
  { hex: '#2C2C2C', name: '鬼方佳代子', category: 'neutral' },
  { hex: '#78909C', name: '蓝灰', category: 'neutral' },
  { hex: '#607D8B', name: '深蓝灰', category: 'neutral' },
  { hex: '#546E7A', name: '暗蓝灰', category: 'neutral' },
  { hex: '#455A64', name: '炭灰', category: 'neutral' },
  { hex: '#37474F', name: '深炭灰', category: 'neutral' },
  { hex: '#795548', name: '棕色', category: 'neutral' },
  { hex: '#6D4C41', name: '深棕', category: 'neutral' },
  { hex: '#5D4037', name: '咖啡棕', category: 'neutral' },
  { hex: '#8D6E63', name: '浅棕', category: 'neutral' },
  { hex: '#9E9E9E', name: '中灰', category: 'neutral' },
]

// 根据分类筛选颜色
const filteredColors = computed(() => {
  if (activeCategory.value === 'all') {
    return colorPresets
  }
  return colorPresets.filter(c => c.category === activeCategory.value)
})

const toggleColorPicker = () => {
  showColorPicker.value = !showColorPicker.value
}

// 点击外部关闭
const handleClickOutside = (event) => {
  const picker = document.querySelector('.color-picker')
  const button = document.querySelector('.theme-selector .icon-button')
  if (picker && !picker.contains(event.target) && !button.contains(event.target)) {
    showColorPicker.value = false
  }
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
})

const selectColor = async (color) => {
  themeStore.setPrimaryColor(color)
  
  // 自动保存配置到 user.json
  if (configStore.general.autoSaveConfig) {
    try {
      await configStore.saveConfig()
      logger.debug('主题色已保存到 user.json')
    } catch (error) {
      logger.error('保存主题色到 user.json 失败:', error)
    }
  }
}

const selectCustomColor = async (event) => {
  themeStore.setPrimaryColor(event.target.value)
  
  // 自动保存配置到 user.json
  if (configStore.general.autoSaveConfig) {
    try {
      await configStore.saveConfig()
      logger.debug('主题色已保存到 user.json')
    } catch (error) {
      logger.error('保存主题色到 user.json 失败:', error)
    }
  }
}

const onHexInput = async (event) => {
  const value = event.target.value.trim()
  // 验证 HEX 颜色格式
  if (/^#[0-9A-Fa-f]{6}$/.test(value)) {
    themeStore.setPrimaryColor(value)
    
    if (configStore.general.autoSaveConfig) {
      try {
        await configStore.saveConfig()
        logger.debug('主题色已保存到 user.json')
      } catch (error) {
        logger.error('保存主题色到 user.json 失败:', error)
      }
    }
  }
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

.custom-color-input input[type="color"] {
  width: 48px;
  height: 48px;
  border: none;
  border-radius: var(--md-sys-shape-corner-medium);
  cursor: pointer;
  padding: 0;
  overflow: hidden;
}

.custom-color-input input[type="color"]::-webkit-color-swatch-wrapper {
  padding: 0;
}

.custom-color-input input[type="color"]::-webkit-color-swatch {
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