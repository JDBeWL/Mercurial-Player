<template>
  <div class="equalizer-settings">
    <div class="content-header">
      <h3>{{ $t('config.equalizer') || 'EQ 均衡器' }}</h3>
      <div class="header-actions">
        <button @click="resetEq" class="icon-button" :title="$t('config.reset') || '重置'">
          <span class="material-symbols-rounded">restart_alt</span>
        </button>
      </div>
    </div>

    <!-- EQ 开关 -->
    <div class="eq-toggle" @click="toggleEnabled">
      <div class="toggle-info">
        <span class="material-symbols-rounded">equalizer</span>
        <span class="toggle-label">{{ $t('config.enableEq') || '启用均衡器' }}</span>
      </div>
      <div class="switch" :class="{ active: enabled }">
        <div class="switch-track"></div>
        <div class="switch-handle"></div>
      </div>
    </div>

    <!-- 预设选择 -->
    <div class="preset-section">
      <label class="section-label">{{ $t('config.eqPreset') || '预设' }}</label>
      <div class="preset-chips">
        <button
          v-for="preset in presets"
          :key="preset.name"
          class="preset-chip"
          :class="{ active: currentPreset === preset.name }"
          @click="applyPreset(preset)"
        >
          {{ getPresetLabel(preset.name) }}
        </button>
      </div>
    </div>

    <!-- 前置增益 -->
    <div class="preamp-section">
      <div class="preamp-header">
        <label class="section-label">{{ $t('config.preamp') || '前置增益' }}</label>
        <span class="preamp-value">{{ preamp > 0 ? '+' : '' }}{{ preamp.toFixed(1) }} dB</span>
      </div>
      <div 
        class="slider horizontal"
        :class="{ disabled: !enabled, dragging: preampDragging }"
        ref="preampSlider"
        @mousedown="startPreampDrag"
        @click="handlePreampClick"
      >
        <div class="slider-track"></div>
        <div class="slider-fill" :style="{ width: `${preampPercent}%` }"></div>
        <div class="slider-thumb" :style="{ left: `${preampPercent}%` }"></div>
      </div>
    </div>

    <!-- 频段滑块 -->
    <div class="bands-section">
      <label class="section-label">{{ $t('config.eqBands') || '频段调节' }}</label>
      <div class="bands-container">
        <div v-for="(band, index) in bands" :key="index" class="band-control">
          <div class="band-value">{{ gains[index] > 0 ? '+' : '' }}{{ gains[index].toFixed(1) }}</div>
          <div 
            class="slider vertical"
            :class="{ disabled: !enabled, dragging: bandDragging === index }"
            :ref="el => bandSliders[index] = el"
            @mousedown="(e) => startBandDrag(e, index)"
            @click="(e) => handleBandClick(e, index)"
          >
            <div class="slider-track"></div>
            <div class="slider-fill" :style="{ height: `${getBandPercent(index)}%` }"></div>
            <div class="slider-thumb" :style="{ bottom: `${getBandPercent(index)}%` }"></div>
          </div>
          <div class="band-label">{{ band.label }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';

// 状态
const enabled = ref(false);
const preamp = ref(0);
const gains = ref([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
const bands = ref([]);
const presets = ref([]);
const currentPreset = ref('Flat');

// 滑块引用
const preampSlider = ref(null);
const bandSliders = ref([]);

// 拖拽状态
const preampDragging = ref(false);
const bandDragging = ref(-1);

// 增益范围
const MIN_GAIN = -8;
const MAX_GAIN = 8;

// 预设名称映射
const presetLabels = {
  'Bass Boost': '低音增强',
  'Treble Boost': '高音增强',
  'Vocal': '人声',
  'Rock': '摇滚',
  'Pop': '流行',
  'Jazz': '爵士',
  'Classical': '古典',
  'Electronic': '电子',
  'Acoustic': '原声'
};

const getPresetLabel = (name) => presetLabels[name] || name;

// 计算前置增益百分比 (0-100)
const preampPercent = computed(() => {
  return ((preamp.value - MIN_GAIN) / (MAX_GAIN - MIN_GAIN)) * 100;
});

// 计算频段增益百分比
const getBandPercent = (index) => {
  return ((gains.value[index] - MIN_GAIN) / (MAX_GAIN - MIN_GAIN)) * 100;
};

// 加载 EQ 设置
const loadSettings = async () => {
  try {
    const [bandsData, settings, presetsData] = await Promise.all([
      invoke('get_eq_bands'),
      invoke('get_eq_settings'),
      invoke('get_eq_presets')
    ]);
    
    bands.value = bandsData;
    enabled.value = settings.enabled;
    preamp.value = settings.preamp;
    gains.value = settings.gains;
    presets.value = presetsData;
    
    detectCurrentPreset();
  } catch (error) {
    console.error('Failed to load EQ settings:', error);
  }
};

// 检测当前预设
const detectCurrentPreset = () => {
  for (const preset of presets.value) {
    const match = preset.gains.every((g, i) => Math.abs(g - gains.value[i]) < 0.1);
    if (match) {
      currentPreset.value = preset.name;
      return;
    }
  }
  currentPreset.value = '';
};

// 切换启用状态
const toggleEnabled = async () => {
  try {
    await invoke('set_eq_enabled', { enabled: !enabled.value });
    enabled.value = !enabled.value;
  } catch (error) {
    console.error('Failed to toggle EQ:', error);
  }
};

// 前置增益滑块处理
const handlePreampClick = (e) => {
  if (!enabled.value || !preampSlider.value) return;
  updatePreampFromEvent(e);
};

const startPreampDrag = (e) => {
  if (!enabled.value) return;
  preampDragging.value = true;
  updatePreampFromEvent(e);
  
  document.addEventListener('mousemove', onPreampDrag);
  document.addEventListener('mouseup', stopPreampDrag);
};

const onPreampDrag = (e) => {
  if (!preampDragging.value) return;
  updatePreampFromEvent(e);
};

const stopPreampDrag = () => {
  preampDragging.value = false;
  document.removeEventListener('mousemove', onPreampDrag);
  document.removeEventListener('mouseup', stopPreampDrag);
};

const updatePreampFromEvent = async (e) => {
  if (!preampSlider.value) return;
  
  const rect = preampSlider.value.getBoundingClientRect();
  const percent = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
  const newValue = MIN_GAIN + percent * (MAX_GAIN - MIN_GAIN);
  const roundedValue = Math.round(newValue * 2) / 2; // 四舍五入到 0.5
  
  try {
    await invoke('set_eq_preamp', { preamp: roundedValue });
    preamp.value = roundedValue;
  } catch (error) {
    console.error('Failed to set preamp:', error);
  }
};

// 频段滑块处理
const handleBandClick = (e, index) => {
  if (!enabled.value) return;
  updateBandFromEvent(e, index);
};

const startBandDrag = (e, index) => {
  if (!enabled.value) return;
  bandDragging.value = index;
  updateBandFromEvent(e, index);
  
  const onDrag = (ev) => {
    if (bandDragging.value !== index) return;
    updateBandFromEvent(ev, index);
  };
  
  const stopDrag = () => {
    bandDragging.value = -1;
    document.removeEventListener('mousemove', onDrag);
    document.removeEventListener('mouseup', stopDrag);
  };
  
  document.addEventListener('mousemove', onDrag);
  document.addEventListener('mouseup', stopDrag);
};

const updateBandFromEvent = async (e, index) => {
  const slider = bandSliders.value[index];
  if (!slider) return;
  
  const rect = slider.getBoundingClientRect();
  // 垂直滑块：从底部计算
  const percent = Math.max(0, Math.min(1, (rect.bottom - e.clientY) / rect.height));
  const newValue = MIN_GAIN + percent * (MAX_GAIN - MIN_GAIN);
  const roundedValue = Math.round(newValue * 2) / 2;
  
  try {
    await invoke('set_eq_band_gain', { band: index, gain: roundedValue });
    gains.value[index] = roundedValue;
    currentPreset.value = '';
  } catch (error) {
    console.error('Failed to set band gain:', error);
  }
};

// 应用预设
const applyPreset = async (preset) => {
  try {
    await invoke('apply_eq_preset', { presetName: preset.name });
    gains.value = [...preset.gains];
    currentPreset.value = preset.name;
  } catch (error) {
    console.error('Failed to apply preset:', error);
  }
};

// 重置 EQ
const resetEq = async () => {
  try {
    await invoke('reset_eq');
    await loadSettings();
  } catch (error) {
    console.error('Failed to reset EQ:', error);
  }
};

onMounted(() => {
  loadSettings();
});

onUnmounted(() => {
  // 清理可能残留的事件监听器
  document.removeEventListener('mousemove', onPreampDrag);
  document.removeEventListener('mouseup', stopPreampDrag);
});
</script>

<style scoped>
.equalizer-settings {
  max-width: 720px;
}

.content-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

.header-actions {
  display: flex;
  gap: 8px;
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: var(--md-sys-shape-corner-large, 12px);
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  transition: all 0.2s ease;
}

.icon-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
}

/* EQ 开关 */
.eq-toggle {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px;
  margin-bottom: 24px;
  border-radius: 12px;
  background-color: var(--md-sys-color-surface-container);
  cursor: pointer;
  transition: background-color 0.2s;
}

.eq-toggle:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.toggle-info {
  display: flex;
  align-items: center;
  gap: 12px;
}

.toggle-info .material-symbols-rounded {
  font-size: 24px;
  color: var(--md-sys-color-primary);
}

.toggle-label {
  font-size: 16px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

/* MD3 Switch */
.switch {
  position: relative;
  width: 52px;
  height: 32px;
  flex-shrink: 0;
}

.switch-track {
  position: absolute;
  width: 100%;
  height: 100%;
  background-color: var(--md-sys-color-surface-container-highest);
  border: 2px solid var(--md-sys-color-outline);
  border-radius: 16px;
  box-sizing: border-box;
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
  left: 22px;
  width: 24px;
  height: 24px;
  background-color: var(--md-sys-color-on-primary);
}

/* 预设部分 */
.preset-section {
  margin-bottom: 24px;
}

.section-label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
  margin-bottom: 12px;
}

.preset-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.preset-chip {
  padding: 8px 16px;
  border: 1px solid var(--md-sys-color-outline);
  border-radius: 8px;
  background: none;
  color: var(--md-sys-color-on-surface);
  font-size: 14px;
  cursor: pointer;
  transition: all 0.2s;
}

.preset-chip:hover {
  background-color: var(--md-sys-color-surface-container);
}

.preset-chip.active {
  background-color: var(--md-sys-color-secondary-container);
  border-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

/* 前置增益 */
.preamp-section {
  margin-bottom: 32px;
  padding: 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
}

.preamp-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
}

.preamp-header .section-label {
  margin-bottom: 0;
}

.preamp-value {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-primary);
  min-width: 60px;
  text-align: right;
}

/* MD3 Slider - 水平 */
.slider.horizontal {
  position: relative;
  width: 100%;
  height: 20px;
  cursor: pointer;
  display: flex;
  align-items: center;
}

.slider.horizontal .slider-track {
  position: absolute;
  left: 0;
  right: 0;
  height: 4px;
  background-color: var(--md-sys-color-surface-container-highest);
  border-radius: 2px;
}

.slider.horizontal .slider-fill {
  position: absolute;
  left: 0;
  height: 4px;
  background-color: var(--md-sys-color-primary);
  border-radius: 2px;
  transition: width 0.05s ease-out;
}

.slider.horizontal .slider-thumb {
  position: absolute;
  top: 50%;
  width: 20px;
  height: 20px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  transform: translate(-50%, -50%);
  transition: transform 0.1s ease, box-shadow 0.1s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

.slider.horizontal:hover .slider-thumb {
  transform: translate(-50%, -50%) scale(1.1);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3);
}

.slider.horizontal.dragging .slider-thumb {
  transform: translate(-50%, -50%) scale(1.15);
  box-shadow: 0 3px 8px rgba(0, 0, 0, 0.35);
}

.slider.disabled {
  opacity: 0.5;
  cursor: not-allowed;
  pointer-events: none;
}

/* 频段部分 */
.bands-section {
  margin-bottom: 24px;
}

.bands-container {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  padding: 24px 16px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 12px;
}

.band-control {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 40px;
}

.band-value {
  font-size: 11px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
  min-width: 36px;
  text-align: center;
}

.band-label {
  font-size: 11px;
  color: var(--md-sys-color-on-surface-variant);
  text-align: center;
}

/* MD3 Slider - 垂直 */
.slider.vertical {
  position: relative;
  width: 20px;
  height: 120px;
  cursor: pointer;
}

.slider.vertical .slider-track {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 4px;
  background-color: var(--md-sys-color-surface-container-highest);
  border-radius: 2px;
}

.slider.vertical .slider-fill {
  position: absolute;
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 4px;
  background-color: var(--md-sys-color-primary);
  border-radius: 2px;
  transition: height 0.05s ease-out;
}

.slider.vertical .slider-thumb {
  position: absolute;
  left: 50%;
  width: 16px;
  height: 16px;
  background-color: var(--md-sys-color-primary);
  border-radius: 50%;
  transform: translate(-50%, 50%);
  transition: transform 0.1s ease, box-shadow 0.1s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}

.slider.vertical:hover .slider-thumb {
  transform: translate(-50%, 50%) scale(1.15);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3);
}

.slider.vertical.dragging .slider-thumb {
  transform: translate(-50%, 50%) scale(1.2);
  box-shadow: 0 3px 8px rgba(0, 0, 0, 0.35);
}

/* 响应式 */
@media (max-width: 600px) {
  .bands-container {
    gap: 4px;
    padding: 16px 8px;
  }
  
  .band-control {
    min-width: 28px;
  }
  
  .slider.vertical {
    height: 100px;
    width: 20px;
  }
  
  .slider.vertical .slider-thumb {
    width: 14px;
    height: 14px;
  }
  
  .band-value,
  .band-label {
    font-size: 10px;
  }
  
  .preset-chips {
    gap: 6px;
  }
  
  .preset-chip {
    padding: 6px 12px;
    font-size: 12px;
  }
}
</style>
