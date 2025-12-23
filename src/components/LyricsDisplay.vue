<template>
    <div class="lyrics-wrapper" :class="`lyrics-style-${configStore.general.lyricsStyle}`">
        <div class="lyrics-display" ref="containerRef" @scroll="handleScroll" @mouseenter="isHovering = true" @mouseleave="isHovering = false">
            <div v-if="loading" class="loading">{{ $t('lyrics.loading') }}</div>
            
            <!-- 没有播放音乐时显示空闲状态 -->
            <div v-else-if="!hasCurrentTrack" class="no-lyrics idle-state">
                <span class="material-symbols-rounded idle-icon">music_note</span>
                <span>{{ $t('lyrics.noTrackPlaying') }}</span>
            </div>
            
            <!-- 有音乐但没有歌词 -->
            <div v-else-if="!lyrics.length" class="no-lyrics">
                <span>{{ $t('lyrics.notFound') }}</span>
                <button v-if="configStore.lyrics?.enableOnlineFetch || true" class="fetch-lyrics-btn" @click="handleFetchLyrics" :disabled="fetchingLyrics">
                    <span class="material-symbols-rounded">{{ fetchingLyrics ? 'hourglass_empty' : 'cloud_download' }}</span>
                    {{ fetchingLyrics ? $t('lyrics.fetching') : $t('lyrics.fetchOnline') }}
                </button>
            </div>

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
                        <div class="first-line karaoke-line"><span v-for="(word, idx) in line.words" :key="idx" class="karaoke-text"
                                :class="{ 'active': isWordActive(word) }" :style="getKaraokeStyle(word)">{{ word.text }}</span></div>
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
        
        <!-- 底部控制栏 - 只保留偏移控制 -->
        <div v-if="lyrics.length" class="lyrics-bottom-bar">
            <!-- 歌词偏移控制 -->
            <div class="lyrics-offset-control">
                <button class="offset-btn" @click="adjustOffset(-0.5)" :title="$t('lyrics.offsetDelay')">
                    <span class="material-symbols-rounded">remove</span>
                </button>
                <span class="offset-value" @click="resetOffset" :title="$t('lyrics.offsetReset')">
                    {{ formatOffset(playerStore.lyricsOffset) }}
                </span>
                <button class="offset-btn" @click="adjustOffset(0.5)" :title="$t('lyrics.offsetAdvance')">
                    <span class="material-symbols-rounded">add</span>
                </button>
            </div>
        </div>
    </div>
</template>

<script>
import { usePlayerStore } from '@/stores/player';
import { useConfigStore } from '@/stores/config';
import { nextTick, ref, watch, onMounted, onUnmounted, computed } from 'vue';
import { useLyrics } from '@/composables/useLyrics';

export default {
    name: "LyricsDisplay",
    setup() {
        const playerStore = usePlayerStore();
        const configStore = useConfigStore();
        const containerRef = ref(null);

        // 使用 composable
        const lyricsComposable = useLyrics();
        console.log('lyricsComposable:', lyricsComposable);
        console.log('fetchAndSaveLyrics:', lyricsComposable.fetchAndSaveLyrics);
        const { lyrics, loading, activeIndex, lyricsSource } = lyricsComposable;
        
        // 是否有当前播放的曲目
        const hasCurrentTrack = computed(() => !!playerStore.currentTrack);
        
        // 手动获取歌词状态
        const fetchingLyrics = ref(false);
        
        const handleFetchLyrics = async () => {
            console.log('handleFetchLyrics called, composable:', lyricsComposable);
            fetchingLyrics.value = true;
            try {
                if (typeof lyricsComposable.fetchAndSaveLyrics === 'function') {
                    await lyricsComposable.fetchAndSaveLyrics();
                } else {
                    console.error('fetchAndSaveLyrics is not a function:', lyricsComposable);
                }
            } finally {
                fetchingLyrics.value = false;
            }
        };

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

                const realTime = playerStore.currentTime;
                
                if (playerStore.isPlaying) {
                    // 播放中：基于帧间隔累加时间，并动态调整速度以消除漂移
                    let speed = 1.0;
                    const diff = visualTime.value - realTime; // 正值表示视觉领先，负值表示落后

                    if (Math.abs(diff) > 0.5) {
                        // 误差超过 0.5s，视为 Seek 或严重卡顿，直接硬同步
                        visualTime.value = realTime;
                    } else {
                        // P控制器：速度修正因子。diff 为正(快了)则减速，diff 为负(慢了)则加速
                        speed = 1.0 - diff; 
                        // 限制速度调整范围 [0.5, 1.5] 防止过度加速/减速
                        speed = Math.max(0.5, Math.min(1.5, speed));
                        visualTime.value += deltaTime * speed;
                    }
                } else {
                    // 暂停中：直接同步
                    visualTime.value = realTime;
                }

                rafId = requestAnimationFrame(animate);
            };
            rafId = requestAnimationFrame(animate);
        };

        // 监听真实时间跳变（如拖拽进度条），立即同步
        watch(() => playerStore.currentTime, (newTime) => {
            // 只有当偏差非常大（说明发生了Seek）时才立即硬同步
            // 小偏差交给 RAF 里的平滑算法处理，避免进度条抖动
            if (Math.abs(visualTime.value - newTime) > 0.5) {
                visualTime.value = newTime;
            }
        });

        // --- 样式计算逻辑 ---
        const isActive = (index) => index === activeIndex.value;

        const isWordActive = (word) => {
            // 应用歌词偏移
            const offset = playerStore.lyricsOffset || 0;
            const t = visualTime.value - offset;
            // 只有在时间范围内才算激活，并且考虑下一个单词的开始时间
            return t >= word.start && t < word.end;
        };

        // 计算卡拉OK单词的填充进度 (0% - 100%)
        const getKaraokeStyle = (word) => {
            // 应用歌词偏移
            const offset = playerStore.lyricsOffset || 0;
            const t = visualTime.value - offset;
            if (t >= word.end) return { '--progress': '100%' };
            if (t < word.start) return { '--progress': '0%' };

            // 确保进度计算精确，避免浮点数误差
            const duration = word.end - word.start;
            const elapsed = t - word.start;
            const progress = Math.min(100, Math.max(0, (elapsed / duration) * 100));
            return { '--progress': `${progress.toFixed(2)}%` };
        };

        // --- 滚动控制 ---
        const isAutoScrolling = ref(false); // 标记是否正在自动滚动
        const isHovering = ref(false);      // 标记鼠标是否悬停
        let scrollTimeout = null;

        const handleScroll = () => {
             // 如果是自动滚动触发的事件，忽略
             if (isAutoScrolling.value) return; 
             
             // 只有当鼠标悬停在歌词区域时，才认为是用户的主动滚动
             if (!isHovering.value) return;

             // 用户手动滚动
             isUserScroll.value = true;
             
             // 用户停止滚动 2.5s 后恢复自动跟随
             if (scrollTimeout) clearTimeout(scrollTimeout);
             scrollTimeout = setTimeout(() => {
                 isUserScroll.value = false;
             }, 2500);
        };

        const scrollToActiveLyric = (immediate = false, isUserClick = false, targetIndex = -1) => {
            if (!containerRef.value) return;
            
            const idx = targetIndex !== -1 ? targetIndex : activeIndex.value;
            // 如果索引无效或列表为空
            if (idx === -1 || !lyrics.value.length) return;

            const container = containerRef.value;
            // 直接通过索引查找元素，比 querySelector(".active") 更可靠
            const lyricElements = container.querySelectorAll('.lyrics');
            if (!lyricElements || !lyricElements[idx]) return;
            
            const activeEl = lyricElements[idx];

            nextTick(() => {
                const containerH = container.clientHeight;
                const elTop = activeEl.offsetTop;
                const elH = activeEl.clientHeight;
                let targetScroll;
                // 更加激进的滚动位置
                const offsetRatio = 0.5;
                targetScroll = elTop - (containerH * offsetRatio) + (elH / 2);

                targetScroll = Math.max(0, targetScroll);
                
                // 标记开始自动滚动，防止 handleScroll 误判
                isAutoScrolling.value = true;

                if (immediate || isUserClick) {
                    container.style.scrollBehavior = 'auto';
                    container.scrollTop = targetScroll;
                    requestAnimationFrame(() => {
                         container.style.scrollBehavior = 'smooth';
                         // 稍作延迟释放标志
                         setTimeout(() => isAutoScrolling.value = false, 100);
                    });
                } else {
                    container.style.scrollBehavior = 'smooth';
                    container.scrollTop = targetScroll;
                    setTimeout(() => isAutoScrolling.value = false, 500);
                }
            });
        };

        // 监听 activeIndex 变化以滚动
        watch(activeIndex, () => {
             // 只有在非用户滚动状态下才自动跟随
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
            
            // 点击跳转应打破用户滚动锁定，并强制执行
            isUserScroll.value = false;
            if (scrollTimeout) clearTimeout(scrollTimeout);

            await playerStore.seek(time);
            
            visualTime.value = time;
            const forceSync = () => { visualTime.value = playerStore.currentTime; };
            requestAnimationFrame(forceSync);
            requestAnimationFrame(() => requestAnimationFrame(forceSync));

            // 明确传入目标 index，確保即使 DOM class 更新滞后也能正确找到元素
            nextTick(() => scrollToActiveLyric(true, true, index));
        };

        // 保存 resize 处理函数引用，以便正确清理
        const handleResize = () => scrollToActiveLyric(true);

        // 歌词偏移控制
        const adjustOffset = (delta) => {
            playerStore.adjustLyricsOffset(delta);
        };
        
        const resetOffset = () => {
            playerStore.resetLyricsOffset();
        };
        
        const formatOffset = (offset) => {
            if (offset === 0) return '0s';
            const sign = offset > 0 ? '+' : '';
            return `${sign}${offset.toFixed(1)}s`;
        };

        onMounted(() => {
            startAnimationLoop();
            window.addEventListener("resize", handleResize);
        });

        onUnmounted(() => {
            if (rafId) cancelAnimationFrame(rafId);
            // 清理 scrollTimeout
            if (scrollTimeout) {
                clearTimeout(scrollTimeout);
                scrollTimeout = null;
            }
            // 清理 resize 事件监听器
            window.removeEventListener("resize", handleResize);
        });

        return {
            lyrics, loading, containerRef, configStore, lyricsSource, hasCurrentTrack, playerStore,
            isActive, isWordActive, getKaraokeStyle, handleLyricClick,
            handleScroll, isHovering, fetchingLyrics, handleFetchLyrics,
            adjustOffset, resetOffset, formatOffset
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
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: #888;
    font-size: 24px;
    gap: 16px;
}

.idle-state {
    color: var(--md-sys-color-on-surface-variant);
    opacity: 0.6;
}

.idle-icon {
    font-size: 64px;
    margin-bottom: 8px;
}

.fetch-lyrics-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px;
    background-color: var(--md-sys-color-primary);
    color: var(--md-sys-color-on-primary);
    border: none;
    border-radius: 24px;
    cursor: pointer;
    font-size: 14px;
    font-weight: 500;
    transition: all 0.2s ease;
}

.fetch-lyrics-btn:hover:not(:disabled) {
    background-color: var(--md-sys-color-primary-container);
    color: var(--md-sys-color-on-primary-container);
}

.fetch-lyrics-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
}

.fetch-lyrics-btn .material-symbols-rounded {
    font-size: 20px;
}

.lyrics-bottom-bar {
    position: absolute;
    bottom: 16px;
    left: 16px;
    right: 16px;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.3s ease;
}

.lyrics-wrapper:hover .lyrics-bottom-bar {
    opacity: 1;
}

.lyrics-offset-control {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px;
    background-color: var(--md-sys-color-surface-container);
    border-radius: 20px;
    pointer-events: auto;
}

.offset-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: 50%;
    background-color: transparent;
    color: var(--md-sys-color-on-surface-variant);
    cursor: pointer;
    transition: background-color 0.2s ease;
}

.offset-btn:hover {
    background-color: var(--md-sys-color-surface-container-highest);
}

.offset-btn .material-symbols-rounded {
    font-size: 16px;
}

.offset-value {
    min-width: 40px;
    text-align: center;
    font-size: 11px;
    font-weight: 500;
    color: var(--md-sys-color-on-surface-variant);
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 10px;
    transition: background-color 0.2s ease;
}

.offset-value:hover {
    background-color: var(--md-sys-color-surface-container-highest);
}

.lyrics-spacer-up {
    height: 30vh;
}

.lyrics-spacer-down {
    height: 45vh;
}
</style>