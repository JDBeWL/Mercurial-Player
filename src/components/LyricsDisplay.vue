<template>
    <div class="lyrics-wrapper" :class="`lyrics-style-${configStore.general.lyricsStyle}`">
        <div class="lyrics-display" ref="containerRef">
            <div v-if="loading" class="loading">{{ $t('lyrics.loading') }}</div>
            <div v-else-if="!lyrics.length" class="no-lyrics">{{ $t('lyrics.notFound') }}</div>

            <div v-else>
                <div class="lyrics-spacer-up"></div>

                <div class="lyrics" v-for="(line, index) in lyrics" :key="index" :class="{ active: isActive(index) }"
                    :style="{
                        // 根据配置的对齐方式，动态调整缩放锚点 (左/中/右)，防止放大时位移
                        '--align-origin': configStore.general.lyricsAlignment === 'right' ? 'right center' :
                            configStore.general.lyricsAlignment === 'center' ? 'center center' :
                                'left center',
                        // 应用用户配置的字体和对齐
                        textAlign: configStore.general.lyricsAlignment,
                        fontFamily: configStore.general.lyricsFontFamily
                    }" @click="handleLyricClick(line.time, index)">
                    <template v-if="line.karaoke && isActive(index)">
                        <div class="first-line karaoke-line">
                            <span v-for="(word, idx) in line.words" :key="idx" class="karaoke-text"
                                :class="{ 'active': isWordActive(word) }" :style="getKaraokeStyle(word)">
                                {{ word.text }}
                            </span>
                        </div>
                        <div class="last-line translation" v-if="line.texts[1]">{{ line.texts[1] }}</div>
                    </template>

                    <template v-else>
                        <div class="first-line">{{ line.texts[0] }}</div>
                        <div class="last-line translation" v-if="line.texts[1]">{{ line.texts[1] }}</div>
                    </template>
                </div>

                <div class="lyrics-spacer-down"></div>
            </div>
        </div>
    </div>
</template>

<script>
import { usePlayerStore } from '@/stores/player';
import { useConfigStore } from '@/stores/config';
import { nextTick, ref, watch, onMounted, onUnmounted } from 'vue';
import { useLyrics } from '@/composables/useLyrics';

export default {
    name: "LyricsDisplay",
    setup() {
        const playerStore = usePlayerStore();
        const configStore = useConfigStore();
        const containerRef = ref(null);

        // 使用 composable
        const { lyrics, loading, activeIndex } = useLyrics();

        // --- 视觉时间系统 ---
        const visualTime = ref(0);
        const isUserScroll = ref(false); // 标记用户是否正在交互
        let rafId = null;
        let lastFrameTime = 0;

        // 启动高频时间循环
        const startAnimationLoop = () => {
            const animate = (timestamp) => {
                if (!lastFrameTime) lastFrameTime = timestamp;
                const deltaTime = (timestamp - lastFrameTime) / 1000;
                lastFrameTime = timestamp;

                if (playerStore.isPlaying) {
                    // 播放中：基于帧间隔累加时间，实现平滑过渡
                    visualTime.value += deltaTime;
                } else {
                    // 暂停中：强制同步，防止漂移
                    visualTime.value = playerStore.currentTime;
                }

                // 漂移校正：如果视觉时间与真实时间误差过大，进行硬同步
                const diff = Math.abs(visualTime.value - playerStore.currentTime);
                if (diff > 0.75) {
                    visualTime.value = playerStore.currentTime;
                }

                rafId = requestAnimationFrame(animate);
            };
            rafId = requestAnimationFrame(animate);
        };

        // 监听真实时间跳变（如拖拽进度条），立即同步
        watch(() => playerStore.currentTime, (newTime) => {
            if (Math.abs(visualTime.value - newTime) > 0.1) {
                visualTime.value = newTime;
            }
        });

        // --- 样式计算逻辑 ---
        const isActive = (index) => index === activeIndex.value;

        const isWordActive = (word) => {
            const t = visualTime.value;
            return t >= word.start; // 只要开始了就算激活
        };

        // 计算卡拉OK单词的填充进度 (0% - 100%)
        const getKaraokeStyle = (word) => {
            const t = visualTime.value;
            if (t >= word.end) return { '--progress': '100%' };
            if (t < word.start) return { '--progress': '0%' };

            const progress = ((t - word.start) / (word.end - word.start)) * 100;
            return { '--progress': `${progress.toFixed(2)}%` };
        };

        // --- 滚动控制 ---
        const scrollToActiveLyric = (immediate = false, isUserClick = false) => {
            if (!containerRef.value || activeIndex.value === -1) return;

            const container = containerRef.value;
            const activeEl = container.querySelector(".lyrics.active");

            nextTick(() => {
                if (!activeEl) return;

                const containerH = container.clientHeight;
                const elTop = activeEl.offsetTop;
                const elH = activeEl.clientHeight;
                let targetScroll;
                const currentStyle = configStore.general.lyricsStyle || 'modern';

                if (isUserClick) {
                    targetScroll = elTop - (containerH / 2) + (elH / 2);
                } else {
                    if (currentStyle === 'classic') {
                        targetScroll = elTop - (containerH / 3.25) + (elH / 2);
                    } else {
                        targetScroll = elTop - (containerH * 0.25) + (elH / 2);
                    }
                }

                targetScroll = Math.max(0, targetScroll);

                if (immediate || isUserClick) {
                    container.style.scrollBehavior = 'auto';
                    container.scrollTop = targetScroll;
                    requestAnimationFrame(() => container.style.scrollBehavior = 'smooth');
                } else {
                    container.style.scrollBehavior = 'smooth';
                    container.scrollTop = targetScroll;
                }
            });
        };

        // 监听 activeIndex 变化以滚动
        watch(activeIndex, () => {
             if (!isUserScroll.value) {
                scrollToActiveLyric();
             }
        });

        // 歌词加载完成后滚动到当前位置
        watch(loading, (newVal) => {
            if (!newVal) {
                nextTick(() => scrollToActiveLyric(true));
            }
        });

        // 用户点击歌词跳转
        const handleLyricClick = async (time, index) => {
            if (time < 0) return;
            isUserScroll.value = true; // 暂停自动滚动
            
            // activeIndex 会由 composable 自动更新

            await playerStore.seek(time);
            visualTime.value = time; // 立即同步视觉时间

            nextTick(() => scrollToActiveLyric(true, true));
            setTimeout(() => isUserScroll.value = false, 600); // 600ms 后恢复自动滚动
        };

        onMounted(() => {
            startAnimationLoop();
            window.addEventListener("resize", () => scrollToActiveLyric(true));
        });

        onUnmounted(() => {
            if (rafId) cancelAnimationFrame(rafId);
            window.removeEventListener("resize", () => scrollToActiveLyric(true));
        });

        return {
            lyrics, loading, containerRef, configStore,
            isActive, isWordActive, getKaraokeStyle, handleLyricClick
        };
    }
};
</script>

<style scoped>
.lyrics-wrapper {
    width: 100%;
    height: 100%;
    position: relative;
    overflow: hidden;
}

.lyrics-display {
    height: 100%;
    padding: 0 32px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
    -ms-overflow-style: none;
    scroll-behavior: smooth;
}

.lyrics-display::-webkit-scrollbar {
    display: none;
}

.loading,
.no-lyrics {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #888;
    font-size: 24px;
}

.lyrics-spacer-up {
    height: 30vh;
}

.lyrics-spacer-down {
    height: 45vh;
}
</style>