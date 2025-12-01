import { ref, watch, watchEffect, nextTick } from 'vue';
import { usePlayerStore } from '@/stores/player';
import { FileUtils } from '@/utils/fileUtils';

export function useLyrics() {
    const playerStore = usePlayerStore();
    const lyrics = ref([]);
    const loading = ref(false);
    const activeIndex = ref(-1);

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
                else if (ext === 'ass') lyrics.value = parseASS(content);
            }
        } catch (e) {
            console.error(e);
        } finally {
            loading.value = false;
        }
    };

    // 歌曲切换监听
    watch(() => playerStore.currentTrack?.path, loadLyrics, { immediate: true });

    // 播放进度监听 (使用二分查找优化性能)
    watchEffect(() => {
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
        }
    });

    return {
        lyrics,
        loading,
        activeIndex
    };
}
