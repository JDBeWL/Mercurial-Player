<template>
  <div class="visualizer-panel">
    <!-- 上方：音频波形可视化 -->
    <div class="visualizer-container" ref="visualizerContainer">
      <canvas ref="canvasRef"></canvas>
    </div>

    <!-- 下方：单行歌词显示 -->
    <div class="single-line-lyrics">
      <div v-if="currentLyric" class="lyric-content">
        <div class="lyric-original" :class="{ 'has-translation': !!currentLyric.texts[1] }">
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
        {{ $t('player.musicName') }} <!-- 暂时显示应用名或等待提示 -->
      </div>
    </div>
  </div>
</template>

<script>
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import { usePlayerStore } from '@/stores/player';
import { useLyrics } from '@/composables/useLyrics';
import { invoke } from '@tauri-apps/api/core';

export default {
  name: 'VisualizerPanel',
  setup() {
    const playerStore = usePlayerStore();
    const { lyrics, activeIndex } = useLyrics();
    
    const canvasRef = ref(null);
    const visualizerContainer = ref(null);
    let animationId = null;
    let audioData = [];
    let isFetching = false;

    // 当前歌词
    const currentLyric = computed(() => {
      if (activeIndex.value !== -1 && lyrics.value[activeIndex.value]) {
        return lyrics.value[activeIndex.value];
      }
      return null;
    });

    // --- 视觉时间 (用于卡拉OK) ---
    // 这里简单复用 LyricsDisplay 的逻辑，或者简化
    const visualTime = ref(0);
    let lastFrameTime = 0;

    const getKaraokeStyle = (word) => {
        const t = visualTime.value;
        if (t >= word.end) return { '--progress': '100%', color: 'var(--md-sys-color-primary)' };
        if (t < word.start) return { '--progress': '0%', color: 'var(--md-sys-color-on-surface-variant)' };

        const progress = ((t - word.start) / (word.end - word.start)) * 100;
        // 使用 CSS 变量实现渐变填充效果需要更复杂的 CSS，这里简化为颜色变化或简单的进度
        // 为了效果好，我们可以使用 background-clip: text 配合 linear-gradient
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
      
      // 获取频谱数据
      if (!isFetching && playerStore.isPlaying) {
          isFetching = true;
          invoke('get_spectrum_data').then((data) => {
              if (data && data.length > 0) {
                  audioData = data;
              }
              isFetching = false;
          }).catch((e) => {
              console.error('Failed to get spectrum data:', e);
              isFetching = false;
          });
      }

      // 如果没有数据或暂停，显示直线
      if (!playerStore.isPlaying || audioData.length === 0) {
          ctx.beginPath();
          ctx.moveTo(0, height / 2);
          ctx.lineTo(width, height / 2);
          ctx.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue('--md-sys-color-primary').trim() || '#6750a4';
          ctx.stroke();
          
          // 更新视觉时间
          if (!lastFrameTime) lastFrameTime = timestamp;
          const deltaTime = (timestamp - lastFrameTime) / 1000;
          lastFrameTime = timestamp;
          visualTime.value = playerStore.currentTime;
          
          animationId = requestAnimationFrame(drawVisualizer);
          return;
      }



      // 绘制频谱条
      const bufferLength = audioData.length;
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
        const value = audioData[i];
        // 调整缩放系数，避免频繁顶格
        // 之前是 2.0，稍微降低一点，或者使用非线性缩放
        let barHeight = value * height * 1.5; 
        
        // 限制最大高度，但尽量让它自然过渡
        if (barHeight > height) barHeight = height;
        
        // 最小高度
        if (barHeight < 4) barHeight = 4; // 稍微高一点以便显示圆角

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
      const deltaTime = (timestamp - lastFrameTime) / 1000;
      lastFrameTime = timestamp;
      
      if (playerStore.isPlaying) {
          visualTime.value += deltaTime;
      } else {
          visualTime.value = playerStore.currentTime;
      }
      
      // 同步校正
      if (Math.abs(visualTime.value - playerStore.currentTime) > 0.25) {
          visualTime.value = playerStore.currentTime;
      }

      animationId = requestAnimationFrame(drawVisualizer);
    };

    const resizeCanvas = () => {
        if (canvasRef.value && visualizerContainer.value) {
            canvasRef.value.width = visualizerContainer.value.clientWidth;
            canvasRef.value.height = visualizerContainer.value.clientHeight;
        }
    };

    onMounted(() => {
      window.addEventListener('resize', resizeCanvas);
      resizeCanvas();
      animationId = requestAnimationFrame(drawVisualizer);
    });

    onUnmounted(() => {
      window.removeEventListener('resize', resizeCanvas);
      if (animationId) cancelAnimationFrame(animationId);
    });

    return {
      canvasRef,
      visualizerContainer,
      currentLyric,
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
  border-radius: 0; /* 直角 */
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
  color: var(--md-sys-color-on-surface);
  line-height: 1.3;
  transition: all 0.3s ease;
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
