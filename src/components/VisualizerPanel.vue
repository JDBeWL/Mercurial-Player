<template>
  <div class="visualizer-panel">
    <!-- 上方：音频波形可视化 -->
    <div class="visualizer-container" ref="visualizerContainer">
      <canvas ref="canvasRef"></canvas>
    </div>

    <!-- 下方：单行歌词显示 -->
    <div class="single-line-lyrics">
      <div v-if="currentLyric" class="lyric-content">
        <div :class="[
          'lyric-original',
          { 'has-translation': !!currentLyric.texts[1] },
          isLyricTypeASS ? 'lyric-original-ass' : 'lyric-original-lrc'
        ]">
          <template v-if="currentLyric.karaoke">
             <!-- 复用卡拉OK逻辑 -->
             <span v-for="(word, idx) in currentLyric.words" :key="idx" 
                   class="karaoke-word"
                   :style="getKaraokeStyle(word)">
               {{ word.text }}
             </span>
          </template>
          <template v-else>
            {{ currentLyric.texts[0] }}
          </template>
        </div>
        <div v-if="currentLyric.texts[1]" class="lyric-translation">
          {{ currentLyric.texts[1] }}
        </div>
      </div>
      <div v-else class="lyric-placeholder">
      </div>
    </div>
  </div>
</template>

<script>
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import { usePlayerStore } from '@/stores/player';
import { useLyrics } from '@/composables/useLyrics';
import { listen } from '@tauri-apps/api/event';
import logger from '@/utils/logger';

export default {
  name: 'VisualizerPanel',
  setup() {
    const playerStore = usePlayerStore();
    const { lyrics, activeIndex } = useLyrics();
    
    const canvasRef = ref(null);
    const visualizerContainer = ref(null);
    let animationId = null;
    let audioData = [];
    let smoothedAudioData = [];
    let lastUpdateTime = 0;
    let spectrumListener = null;

    // 平滑插值函数
    const smoothData = (currentData, targetData, smoothingFactor = 0.7) => {
      const result = new Array(128).fill(0);
      for (let i = 0; i < 128; i++) {
        const current = currentData[i] || 0;
        const target = targetData[i] || 0;
        result[i] = current * smoothingFactor + target * (1 - smoothingFactor);
      }
      return result;
    };

    // 当前歌词
    const currentLyric = computed(() => {
      if (activeIndex.value !== -1 && lyrics.value[activeIndex.value]) {
        return lyrics.value[activeIndex.value];
      }
      return null;
    });

    // 判断歌词类型（ASS/LRC）
    const isLyricTypeASS = computed(() => {
      return currentLyric.value && currentLyric.value.words && currentLyric.value.words.length > 0;
    });

    // --- 视觉时间 (用于卡拉OK) ---
    const visualTime = ref(0);
    let lastFrameTime = 0;
    let wasPlaying = false; // 追踪上一帧的播放状态

    // 监听时间跳变（seek 操作）
    watch(() => playerStore.currentTime, (newTime, oldTime) => {
        const jump = newTime - oldTime;
        if (Math.abs(jump) > 1.5 || jump < -0.1) {
            visualTime.value = newTime;
        }
    });

    // 应用歌词偏移
    const getKaraokeStyle = (word) => {
        const offset = playerStore.lyricsOffset || 0;
        const t = visualTime.value - offset;
        if (t >= word.end) return { '--progress': '100%', color: 'var(--md-sys-color-primary)' };
        if (t < word.start) return { '--progress': '0%', color: 'var(--md-sys-color-on-surface-variant)' };

        const progress = ((t - word.start) / (word.end - word.start)) * 100;
        return { 
            '--progress': `${progress.toFixed(2)}%`,
            backgroundImage: `linear-gradient(90deg, var(--md-sys-color-primary) ${progress}%, var(--md-sys-color-on-surface-variant) ${progress}%)`
        };
    };

    // --- 可视化绘制 ---
    const drawVisualizer = (timestamp) => {
      if (!canvasRef.value || !visualizerContainer.value) return;
      
      const canvas = canvasRef.value;
      const ctx = canvas.getContext('2d');
      const width = canvas.width;
      const height = canvas.height;
      
      // 清空画布
      ctx.clearRect(0, 0, width, height);
      
      // 频谱数据现在通过事件监听获取，不再需要频繁请求

      // 应用平滑处理
      if (audioData.length > 0) {
        smoothedAudioData = smoothData(smoothedAudioData, audioData, 0.85);
      }
      
      // 绘制频谱条
      const drawData = smoothedAudioData.length > 0 ? smoothedAudioData : audioData;

      // 如果没有数据或暂停，显示直线在底部
      if (!playerStore.isPlaying || drawData.length === 0) {
          ctx.beginPath();
          ctx.moveTo(0, height - 2);
          ctx.lineTo(width, height - 2);
          ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue('--md-sys-color-primary').trim() || '#6750a4';
          ctx.stroke();
          
          // 暂停时同步视觉时间
          visualTime.value = playerStore.currentTime;
          wasPlaying = false;
          
          animationId = requestAnimationFrame(drawVisualizer);
          return;
      }

      const bufferLength = drawData.length;
      const barWidth = (width / bufferLength) * 0.8; // 留出间隙
      const gap = (width / bufferLength) * 0.2;
      let x = 0;

      // 创建渐变
      const gradient = ctx.createLinearGradient(0, height, 0, 0); // 从底部向上
      const primaryColor = getComputedStyle(document.documentElement).getPropertyValue('--md-sys-color-primary').trim() || '#6750a4';
      
      gradient.addColorStop(0, primaryColor); // 底部深
      gradient.addColorStop(1, `${primaryColor}40`); // 顶部淡

      ctx.fillStyle = gradient;
      
      // 移除发光效果
      ctx.shadowBlur = 0;

      for (let i = 0; i < bufferLength; i++) {
        // 极小的底噪，确保最低限度的动画，同时避免过多抖动
        const baseNoise = 0.01 + (Math.random() * 0.01);
        const value = drawData[i] + baseNoise;
        // 使用对数缩放以增强微小变化的可视性
        let barHeight = Math.pow(value, 0.9) * height * 0.9; 
        
        // 限制最大高度，但尽量让它自然过渡
        if (barHeight > height) barHeight = height;
        
        // 降低最小高度，让微小的频率变化也能显示
        if (barHeight < 2) barHeight = 2;

        // 单侧绘制 (从底部向上)
        // 使用 roundRect 绘制圆角柱子 (如果浏览器支持)
        // 顶部圆角
        if (ctx.roundRect) {
            ctx.beginPath();
            ctx.roundRect(x, height - barHeight, barWidth, barHeight, [5, 5, 0, 0]);
            ctx.fill();
        } else {
            // 回退方案
            ctx.fillRect(x, height - barHeight, barWidth, barHeight);
        }

        x += barWidth + gap;
      }

      // 清除阴影
      ctx.shadowBlur = 0;

      
      // 更新视觉时间
      if (!lastFrameTime) lastFrameTime = timestamp;
      const deltaTime = Math.min((timestamp - lastFrameTime) / 1000, 0.1);
      lastFrameTime = timestamp;
      
      const realTime = playerStore.currentTime;
      const isPlaying = playerStore.isPlaying;
      
      // 检测暂停后恢复
      if (isPlaying && !wasPlaying) {
          lastFrameTime = timestamp;
      }
      wasPlaying = isPlaying;
      
      if (isPlaying) {
          // 播放中：基于帧间隔累加时间，并动态调整速度以消除漂移 (P控制器)
          const diff = visualTime.value - realTime;

          if (Math.abs(diff) > 0.5) {
              // 误差超过 0.5s，硬同步
              visualTime.value = realTime;
          } else if (Math.abs(diff) > 0.05) {
              // 误差在 0.05s ~ 0.5s 之间，平滑追赶
              const speed = 1.0 - diff * 2.0;
              const clampedSpeed = Math.max(0.7, Math.min(1.3, speed));
              visualTime.value += deltaTime * clampedSpeed;
          } else {
              // 误差很小，正常累加
              visualTime.value += deltaTime;
          }
      } else {
          // 暂停中：直接同步
          visualTime.value = realTime;
      }
      animationId = requestAnimationFrame(drawVisualizer);
    };

    const resizeCanvas = () => {
        if (canvasRef.value && visualizerContainer.value) {
            canvasRef.value.width = visualizerContainer.value.clientWidth;
            canvasRef.value.height = visualizerContainer.value.clientHeight;
        }
    };

    onMounted(async () => {
      window.addEventListener('resize', resizeCanvas);
      resizeCanvas();
      
      // 监听频谱更新事件
      try {
        spectrumListener = await listen('spectrum-update', (event) => {
          if (event.payload && event.payload.data) {
            const now = Date.now();
            // 限制更新频率为60fps
            if (now - lastUpdateTime >= 16) {
              audioData = event.payload.data;
              lastUpdateTime = now;
            }
          }
        });
      } catch (error) {
        logger.error('Failed to setup spectrum listener:', error);
      }
      
      animationId = requestAnimationFrame(drawVisualizer);
    });

    onUnmounted(async () => {
      window.removeEventListener('resize', resizeCanvas);
      if (animationId) cancelAnimationFrame(animationId);
      if (spectrumListener) {
        spectrumListener();
      }
    });

    return {
      canvasRef,
      visualizerContainer,
      currentLyric,
      isLyricTypeASS,
      getKaraokeStyle
    };
  }
}
</script>

<style scoped>
.visualizer-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  width: 100%;
  gap: 24px;
}

.visualizer-container {
  flex: 0 0 35%;
  margin-top: 4%;
  width: 100%;
  min-height: 100px;
  background-color: var(--md-sys-color-surface-container-high);
  border-radius: 0;
  overflow: hidden;
  position: relative;
}

canvas {
  width: 100%;
  height: 100%;
  display: block;
}

.single-line-lyrics {
  flex: 0;
  min-height: 100px;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: right;
  /* padding: 8px; */
}

.lyric-content {
  display: flex;
  flex-direction: column;
  /* gap: 4px; */
  width: 100%;
}

.lyric-original {
  font-size: 36px;
  font-weight: 700;
  color: var(--md-sys-color-primary);
  line-height: 1.3;
  transition: all 0.3s ease;
}

/* LRC字幕颜色 */
.lyric-original-lrc {
  color: var(--md-sys-color-primary);
}

/* ASS字幕颜色 */
.lyric-original-ass {
  color: var(--md-sys-color-primary);
}

.lyric-translation {
  font-size: 32px;
  color: var(--md-sys-color-primary);
  font-weight: 400;
  margin-top: 8px;
}

.karaoke-word {
    background-clip: text;
    -webkit-background-clip: text;
    color: transparent; 
}

.lyric-placeholder {
    color: var(--md-sys-color-outline);
    font-size: 24px;
}
</style>
