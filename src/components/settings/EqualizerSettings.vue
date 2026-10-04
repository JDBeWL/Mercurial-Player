<template>
  <div class="equalizer-settings">
    <div class="content-header">
      <h3>{{ $t('config.equalizer') }}</h3>
    </div>

    <div class="eq-toggle" @click="toggleEnabled">
      <div class="toggle-info">
        <span class="material-symbols-rounded">equalizer</span>
        <span class="toggle-label">{{ $t('config.enableEq') }}</span>
      </div>
      <SettingSwitch :model-value="enabled" />
    </div>

    <div class="preset-section">
      <label class="section-label">{{ $t('config.eqPreset') }}</label>
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

    <div class="preamp-section">
      <div class="preamp-header">
        <label class="section-label">{{ $t('config.preamp') }}</label>
        <span class="preamp-value">{{ preamp > 0 ? '+' : '' }}{{ preamp.toFixed(1) }} dB</span>
      </div>
      <div
        ref="preampSlider"
        class="slider horizontal"
        :class="{ disabled: !enabled, dragging: preampDragging }"
        @pointerdown="startPreampDrag"
        @click="handlePreampClick"
      >
        <div class="slider-track"></div>
        <div class="slider-fill" :style="{ width: `${preampPercent}%` }"></div>
        <div class="slider-thumb" :style="{ left: `${preampPercent}%` }"></div>
      </div>
    </div>

    <div class="bands-section">
      <!-- 重置抹平的就是这些滑块, 跟着标题行比挂页头显眼; 竖屏下不留只漂一个图标的空行 -->
      <div class="bands-header">
        <label class="section-label">{{ $t('config.eqBands') }}</label>
        <button class="text-button" @click="resetEq">
          <span class="material-symbols-rounded">restart_alt</span>
          {{ $t('config.reset') }}
        </button>
      </div>
      <div class="bands-container">
        <div v-for="(band, index) in bands" :key="index" class="band-control">
          <div class="band-value">
            {{ gains[index]! > 0 ? '+' : '' }}{{ gains[index]!.toFixed(1) }}
          </div>
          <div
            :ref="(el) => (bandSliders[index] = el as HTMLElement)"
            class="slider vertical"
            :class="{ disabled: !enabled, dragging: bandDragging && activeBandIndex === index }"
            @pointerdown="(e) => startBandDrag(e, index)"
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

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import logger from '../../utils/logger'
import SettingSwitch from './SettingSwitch.vue'
import { useDragValue } from '../../composables/useDragValue'
import {
  applyEqPreset,
  getEqBands,
  getEqPresets,
  getEqSettings,
  resetEq as resetEqCommand,
  setEqBandGain,
  setEqEnabled,
  setEqPreamp,
  type EqBandInfo,
  type EqPreset,
} from '../../services/eqService'

const enabled = ref<boolean>(false)
const preamp = ref<number>(0)
const gains = ref<number[]>([0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
const bands = ref<EqBandInfo[]>([])
const presets = ref<EqPreset[]>([])
const currentPreset = ref<string>('Flat')

const preampSlider = ref<HTMLElement | null>(null)
// 函数式 ref 的 el 是 Element | ComponentPublicInstance | null, 存 unknown[]
const bandSliders = ref<unknown[]>([])

// 前置增益与频段增益单位 dB
const MIN_GAIN = -8
const MAX_GAIN = 8

// 预设名走 i18n (key 见 config.eqPresetNames), 缺失时回退原名
const { t, te } = useI18n()

const getPresetLabel = (name: string): string => {
  const key = `config.eqPresetNames.${name}`
  return te(key) ? t(key) : name
}

const preampPercent = computed<number>(() => {
  return ((preamp.value - MIN_GAIN) / (MAX_GAIN - MIN_GAIN)) * 100
})

const getBandPercent = (index: number): number => {
  return ((gains.value[index]! - MIN_GAIN) / (MAX_GAIN - MIN_GAIN)) * 100
}

const loadSettings = async (): Promise<void> => {
  try {
    const [bandsData, settings, presetsData] = await Promise.all([
      getEqBands(),
      getEqSettings(),
      getEqPresets(),
    ])

    bands.value = bandsData
    enabled.value = settings.enabled
    preamp.value = settings.preamp
    gains.value = settings.gains
    presets.value = presetsData

    detectCurrentPreset()
  } catch (error) {
    logger.error('Failed to load EQ settings:', error)
  }
}

const detectCurrentPreset = (): void => {
  for (const preset of presets.value) {
    const match = preset.gains.every((g, i) => Math.abs(g - gains.value[i]!) < 0.1)
    if (match) {
      currentPreset.value = preset.name
      return
    }
  }
  currentPreset.value = ''
}

const toggleEnabled = async (): Promise<void> => {
  try {
    await setEqEnabled(!enabled.value)
    enabled.value = !enabled.value
  } catch (error) {
    logger.error('Failed to toggle EQ:', error)
  }
}

const updatePreampFromPercent = (percent: number): void => {
  const newValue = MIN_GAIN + percent * (MAX_GAIN - MIN_GAIN)
  const roundedValue = Math.round(newValue * 2) / 2 // 步进 0.5 dB

  // 值未跨过 0.5 dB 刻度时不做任何事
  if (preamp.value === roundedValue) return
  preamp.value = roundedValue
  void sendPreamp(roundedValue)
}

const preampDrag = useDragValue({
  getPercent: (event) => {
    if (!preampSlider.value) return 0
    const rect = preampSlider.value.getBoundingClientRect()
    return Math.max(0, Math.min(1, (event.clientX - rect.left) / rect.width))
  },
  onStart: (percent) => {
    if (!enabled.value) {
      return false
    }
    updatePreampFromPercent(percent)
    return undefined
  },
  onMove: updatePreampFromPercent,
})
const { isDragging: preampDragging, startDrag: startPreampDrag } = preampDrag

const handlePreampClick = (e: MouseEvent): void => {
  if (!enabled.value) return
  updatePreampFromPercent(preampDrag.getPercent(e))
}

// IPC 抑制: 拖拽中 mousemove 可达 60-125Hz, UI 立即更新保证滑块跟手;
// 每个滑块同一时刻最多一个在途 invoke, 在途期间的新值合并为最新值,
// 请求完成后若仍有变化则补发, 收敛到最终值

let preampSending = false
let preampPending = 0

const sendPreamp = async (value: number): Promise<void> => {
  if (preampSending) {
    preampPending = value
    return
  }
  preampSending = true
  preampPending = value
  try {
    for (;;) {
      const target = preampPending
      await setEqPreamp(target)
      if (preampPending === target) break
    }
  } catch (error) {
    logger.error('Failed to set preamp:', error)
  } finally {
    preampSending = false
  }
}

// 全部频段共用一个拖拽控制器, activeBandIndex 标识当前频段
const updateBandFromPercent = (index: number, percent: number): void => {
  const newValue = MIN_GAIN + percent * (MAX_GAIN - MIN_GAIN)
  const roundedValue = Math.round(newValue * 2) / 2

  // 同前置增益: 未跨过刻度不动作
  if (gains.value[index] === roundedValue) return
  gains.value[index] = roundedValue
  currentPreset.value = ''
  void sendBandGain(index, roundedValue)
}

const activeBandIndex = ref<number>(-1)

const bandDrag = useDragValue({
  getPercent: (event) => {
    const slider = bandSliders.value[activeBandIndex.value] as HTMLElement | null
    if (!slider) return 0
    const rect = slider.getBoundingClientRect()
    // 垂直滑块从底部起算
    return Math.max(0, Math.min(1, (rect.bottom - event.clientY) / rect.height))
  },
  onStart: (percent) => {
    if (!enabled.value || activeBandIndex.value === -1) {
      return false
    }
    updateBandFromPercent(activeBandIndex.value, percent)
    return undefined
  },
  onMove: (percent) => {
    if (activeBandIndex.value === -1) return
    updateBandFromPercent(activeBandIndex.value, percent)
  },
})
const { isDragging: bandDragging, startDrag: bandStartDrag } = bandDrag

const handleBandClick = (e: MouseEvent, index: number): void => {
  if (!enabled.value) return
  activeBandIndex.value = index
  updateBandFromPercent(index, bandDrag.getPercent(e))
}

const startBandDrag = (e: MouseEvent, index: number): void => {
  activeBandIndex.value = index
  bandStartDrag(e)
}

interface BandSendState {
  sent: number
  latest: number
}

const bandInFlight = new Map<number, BandSendState>()

const sendBandGain = async (index: number, value: number): Promise<void> => {
  const state = bandInFlight.get(index)
  if (state) {
    // 在途时只更新目标值, 见上方 IPC 抑制说明
    state.latest = value
    return
  }
  const entry: BandSendState = { sent: value, latest: value }
  bandInFlight.set(index, entry)
  try {
    for (;;) {
      await setEqBandGain(index, entry.sent)
      if (entry.latest === entry.sent) break
      entry.sent = entry.latest
    }
  } catch (error) {
    logger.error('Failed to set band gain:', error)
  } finally {
    bandInFlight.delete(index)
  }
}

const applyPreset = async (preset: EqPreset): Promise<void> => {
  try {
    await applyEqPreset(preset.name)
    gains.value = [...preset.gains]
    currentPreset.value = preset.name
  } catch (error) {
    logger.error('Failed to apply preset:', error)
  }
}

const resetEq = async (): Promise<void> => {
  try {
    await resetEqCommand()
    await loadSettings()
  } catch (error) {
    logger.error('Failed to reset EQ:', error)
  }
}

onMounted(() => {
  void loadSettings()
})
</script>

<style scoped>
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

@media (hover: hover) {
  .eq-toggle:hover {
    background-color: var(--md-sys-color-surface-container-high);
  }
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

@media (hover: hover) {
  .preset-chip:hover {
    background-color: var(--md-sys-color-surface-container);
  }
}

.preset-chip.active {
  background-color: var(--md-sys-color-secondary-container);
  border-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

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

.slider.horizontal {
  position: relative;
  width: 100%;
  height: 20px;
  cursor: pointer;
  display: flex;
  align-items: center;
  /* 不禁用默认手势会被当成横向滚动, 拖到一半就断 */
  touch-action: none;
}

.slider.horizontal .slider-track {
  position: absolute;
  left: 0;
  right: 0;
  height: 4px;
  background-color: var(--md-sys-color-surface-variant);
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
  transition:
    transform 0.1s ease,
    box-shadow 0.1s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  /* 覆盖全局 .slider-thumb 的 opacity: 0, 滑柄要默认可见而不是仅 hover 显示 */
  opacity: 1;
}

@media (hover: hover) {
  .slider.horizontal:hover .slider-thumb {
    transform: translate(-50%, -50%) scale(1.1);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3);
  }
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

.bands-section {
  margin-bottom: 24px;
}

.bands-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

/* 标签自带下边距会把按钮一起压低, 行高交给这一行统一 */
.bands-header .section-label {
  margin-bottom: 0;
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

.slider.vertical {
  position: relative;
  width: 20px;
  height: 120px;
  cursor: pointer;
  /* 竖直滑动默认是页面滚动, 必须显式接管 */
  touch-action: none;
}

.slider.vertical .slider-track {
  position: absolute;
  top: 0;
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 4px;
  /* 覆盖全局 .slider-track 的 height: 4px, 否则 top+bottom+height 过度约束使
     bottom 被忽略, 轨道只剩顶部 4px 不可见 */
  height: auto;
  background-color: var(--md-sys-color-surface-variant);
  border-radius: 2px;
}

.slider.vertical .slider-fill {
  position: absolute;
  bottom: 0;
  left: 50%;
  transform: translateX(-50%);
  width: 4px;
  background-color: var(--md-sys-color-primary);
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
  transition:
    transform 0.1s ease,
    box-shadow 0.1s ease;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
  /* 同水平滑块: 覆盖全局 opacity: 0 */
  opacity: 1;
}

@media (hover: hover) {
  .slider.vertical:hover .slider-thumb {
    transform: translate(-50%, 50%) scale(1.15);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3);
  }
}

.slider.vertical.dragging .slider-thumb {
  transform: translate(-50%, 50%) scale(1.2);
  box-shadow: 0 3px 8px rgba(0, 0, 0, 0.35);
}

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
