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
import { nextTick, ref, watch, onMounted, onUnmounted, watchEffect } from 'vue';
import { FileUtils } from '@/utils/fileUtils';

export default {
    name: "LyricsDisplay",
    setup() {
        const playerStore = usePlayerStore();
        const configStore = useConfigStore();

        // --- 基础状态 ---
        const lyrics = ref([]);
        const loading = ref(false);
        const containerRef = ref(null);
        const activeIndex = ref(-1);

        // --- 视觉时间系统 (60FPS 动画核心) ---
        // 相比 store.currentTime 更新频率更高，解决卡拉OK过渡卡顿问题
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

                // 漂移校正：如果视觉时间与真实时间误差过大(>0.25s)，进行硬同步
                const diff = Math.abs(visualTime.value - playerStore.currentTime);
                if (diff > 0.25) {
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
                        // 这里应该是计算出来的这里不是很稳定
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

        // --- 歌词解析器 (LRC) ---
        const parseLRC = (lrcText) => {
            const lines = lrcText.split("\n");
            const pattern = /\[(\d{2}):(\d{2}):(\d{2})\]|\[(\d{2}):(\d{2})\.(\d{2,3})\]/g;
            const resultMap = {};

            for (const line of lines) {
                const timestamps = [];
                let match;
                while ((match = pattern.exec(line)) !== null) {
                    let time;
                    if (match[1] !== undefined) {
                        time = parseInt(match[1]) * 60 + parseInt(match[2]) + parseInt(match[3]) / 100;
                    } else {
                        time = parseInt(match[4]) * 60 + parseInt(match[5]) + parseInt(match[6].padEnd(3, "0")) / 1000;
                    }
                    timestamps.push({ time, index: match.index });
                }

                if (timestamps.length < 1) continue;
                const text = line.replace(pattern, "").trim();
                if (!text) continue;

                const startTime = timestamps[0].time;
                resultMap[startTime] = resultMap[startTime] || { time: startTime, texts: [], karaoke: null };

                // 如果一行有多个时间戳，视为简单卡拉OK
                if (timestamps.length > 1) {
                    resultMap[startTime].karaoke = {
                        fullText: text,
                        timings: timestamps.slice(1).map((s, i) => ({ time: s.time, position: i + 1 }))
                    };
                }
                resultMap[startTime].texts.push(text);
            }
            return Object.values(resultMap).sort((a, b) => a.time - b.time);
        };

        // --- 歌词解析器 (ASS) ---
        const parseASS = (assText) => {
            const lines = assText.split('\n');
            const dialogues = [];
            const toSeconds = (t) => {
                const [h, m, s] = t.split(':');
                return parseInt(h) * 3600 + parseInt(m) * 60 + parseFloat(s);
            };
            for (const line of lines) {
                if (!line.startsWith('Dialogue:')) continue;
                const parts = line.split(',');
                if (parts.length < 10) continue;
                const start = parts[1].trim();
                const end = parts[2].trim();
                const style = parts[3].trim();
                const text = parts.slice(9).join(',').trim();
                dialogues.push({ startTime: toSeconds(start), endTime: toSeconds(end), style, text });
            }
            const groupedMap = new Map();
            dialogues.forEach(d => {
                const key = `${d.startTime.toFixed(3)}-${d.endTime.toFixed(3)}`;
                if (!groupedMap.has(key)) {
                    groupedMap.set(key, { startTime: d.startTime, endTime: d.endTime, texts: { orig: '', ts: '' }, karaoke: null });
                }
                const group = groupedMap.get(key);
                if (d.style === 'orig') group.texts.orig = d.text;
                if (d.style === 'ts') group.texts.ts = d.text;
            });
            const result = [];
            groupedMap.forEach(group => {
                const parseKaraoke = (text) => {
                    const karaokeTag = /{\\k[f]?(\d+)}([^{}]+)/g;
                    let words = [];
                    let accTime = group.startTime;
                    let match;
                    while ((match = karaokeTag.exec(text)) !== null) {
                        const duration = match[0].includes('\\kf') ? parseInt(match[1]) * 0.01 : parseInt(match[1]) * 0.1;
                        words.push({ text: match[2], start: accTime, end: accTime + duration });
                        accTime += duration;
                    }
                    return words;
                };
                const enWords = parseKaraoke(group.texts.orig);
                result.push({
                    time: group.startTime,
                    texts: [group.texts.orig.replace(/{.*?}/g, ''), group.texts.ts.replace(/{.*?}/g, '')],
                    words: enWords,
                    karaoke: enWords.length > 0
                });
            });
            return result.sort((a, b) => a.time - b.time);
        };

        // --- 核心逻辑：加载与同步 ---
        const loadLyrics = async (trackPath) => {
            if (!trackPath) { lyrics.value = []; return; }
            loading.value = true;
            lyrics.value = [];
            try {
                // 查找并读取歌词文件
                const lyricsPath = await FileUtils.findLyricsFile(trackPath);
                if (lyricsPath) {
                    const content = await FileUtils.readFile(lyricsPath);
                    const ext = FileUtils.getFileExtension(lyricsPath);
                    if (ext === 'lrc') lyrics.value = parseLRC(content);
                    else if (ext === 'ass') lyrics.value = parseASS(content); // 注意恢复你的 parseASS 实现
                }
            } catch (e) {
                console.error(e);
            } finally {
                loading.value = false;
                nextTick(() => scrollToActiveLyric(true));
            }
        };

        // 歌曲切换监听
        watch(() => playerStore.currentTrack?.path, loadLyrics, { immediate: true });

        // 播放进度监听 (使用二分查找优化性能)
        watchEffect(() => {
            if (isUserScroll.value) return;
            const currentTime = playerStore.currentTime + 0.2; // 0.2s 提前量，优化观感

            let l = 0, r = lyrics.value.length - 1, idx = -1;
            while (l <= r) {
                const mid = (l + r) >> 1;
                if (lyrics.value[mid].time <= currentTime) {
                    idx = mid;
                    l = mid + 1;
                } else {
                    r = mid - 1;
                }
            }

            if (idx !== activeIndex.value) {
                activeIndex.value = idx;
                scrollToActiveLyric();
            }
        });

        // 用户点击歌词跳转
        const handleLyricClick = async (time, index) => {
            if (time < 0) return;
            isUserScroll.value = true; // 暂停自动滚动
            activeIndex.value = index;

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