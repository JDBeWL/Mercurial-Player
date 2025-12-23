import { ref, watch, watchEffect } from 'vue';
import { usePlayerStore } from '@/stores/player';
import { useConfigStore } from '@/stores/config';
import { FileUtils } from '@/utils/fileUtils';
import { neteaseApi } from '@/utils/neteaseApi';
import { invoke } from '@tauri-apps/api/core';

// 模块级别的在线歌词缓存，避免组件重新挂载时丢失
const onlineLyricsCache = new Map(); // key: trackPath, value: { lrc: string, parsed: array, source: string }

// 模块级别的共享状态，确保所有 useLyrics 实例共享同一个 lyricsSource
const sharedLyricsSource = ref('local');

export function useLyrics() {
    const playerStore = usePlayerStore();
    const configStore = useConfigStore();
    const lyrics = ref([]);
    const loading = ref(false);
    const activeIndex = ref(-1);
    // 使用共享的 lyricsSource
    const lyricsSource = sharedLyricsSource;
    const onlineLyricsError = ref(null);

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
            const key = d.startTime.toFixed(3) + '-' + d.endTime.toFixed(3);
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
                const karaokeTag = /{\\k[f]?(\d+)}([^{}]*)/g;
                let words = [];
                let accTime = group.startTime;
                let match;
                while ((match = karaokeTag.exec(text)) !== null) {
                    const duration = parseInt(match[1]) * 0.01;
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

    const fetchOnlineLyrics = async (track) => {
        if (!track) return null;
        try {
            const title = track.title || track.name || FileUtils.getFileNameWithoutExtension(track.path);
            const artist = track.artist || '';
            const duration = track.duration ? track.duration * 1000 : 0;
            console.log('Fetching online lyrics for: ' + title + ' - ' + artist);
            const lyricsData = await neteaseApi.searchAndGetLyrics(title, artist, duration);
            if (!lyricsData || !lyricsData.lrc) {
                console.log('No online lyrics found');
                return null;
            }
            let lrcContent = lyricsData.lrc;
            if (configStore.lyrics?.preferTranslation && lyricsData.tlyric) {
                lrcContent = neteaseApi.mergeLyrics(lyricsData.lrc, lyricsData.tlyric);
            }
            return lrcContent;
        } catch (error) {
            console.error('Failed to fetch online lyrics:', error);
            onlineLyricsError.value = error.message;
            return null;
        }
    };

    const saveLyricsToLocal = async (trackPath, lrcContent) => {
        if (!trackPath || !lrcContent) return false;
        try {
            const baseName = FileUtils.getFileNameWithoutExtension(trackPath);
            const directory = FileUtils.getDirectoryPath(trackPath);
            const lyricsPath = directory + '/' + baseName + '.lrc';
            await invoke('write_lyrics_file', { path: lyricsPath, content: lrcContent });
            console.log('Lyrics saved to: ' + lyricsPath);
            return true;
        } catch (error) {
            console.error('Failed to save lyrics:', error);
            return false;
        }
    };

    const loadLyrics = async (trackPath) => {
        if (!trackPath) { 
            lyrics.value = []; 
            lyricsSource.value = 'local';
            onlineLyricsError.value = null;
            return; 
        }
        
        // 先检查缓存中是否有这首歌的在线歌词
        const cached = onlineLyricsCache.get(trackPath);
        if (cached) {
            console.log('Using cached online lyrics for:', trackPath);
            lyrics.value = cached.parsed;
            lyricsSource.value = cached.source;
            loading.value = false;
            return;
        }
        
        loading.value = true;
        lyrics.value = [];
        lyricsSource.value = 'local';
        onlineLyricsError.value = null;
        try {
            const lyricsPath = await FileUtils.findLyricsFile(trackPath);
            if (lyricsPath) {
                const content = await FileUtils.readFile(lyricsPath);
                const ext = FileUtils.getFileExtension(lyricsPath);
                if (ext === 'lrc') lyrics.value = parseLRC(content);
                else if (ext === 'ass') lyrics.value = parseASS(content);
                lyricsSource.value = 'local';
            } else if (configStore.lyrics?.enableOnlineFetch) {
                console.log('No local lyrics found, trying online fetch...');
                const track = playerStore.currentTrack;
                const onlineLrc = await fetchOnlineLyrics(track);
                if (onlineLrc) {
                    const parsed = parseLRC(onlineLrc);
                    lyrics.value = parsed;
                    lyricsSource.value = 'online';
                    
                    // 缓存在线歌词
                    onlineLyricsCache.set(trackPath, {
                        lrc: onlineLrc,
                        parsed: parsed,
                        source: 'online'
                    });
                    
                    if (configStore.lyrics?.autoSaveOnlineLyrics) {
                        const saved = await saveLyricsToLocal(trackPath, onlineLrc);
                        if (saved) {
                            lyricsSource.value = 'local';
                            // 保存成功后从缓存中移除，下次会从本地加载
                            onlineLyricsCache.delete(trackPath);
                        }
                    }
                }
            }
        } catch (e) {
            console.error('Error loading lyrics:', e);
            onlineLyricsError.value = e.message;
        } finally {
            loading.value = false;
        }
    };

    const fetchAndSaveLyrics = async () => {
        const track = playerStore.currentTrack;
        if (!track) return false;
        loading.value = true;
        onlineLyricsError.value = null;
        try {
            const onlineLrc = await fetchOnlineLyrics(track);
            if (onlineLrc) {
                const parsed = parseLRC(onlineLrc);
                lyrics.value = parsed;
                lyricsSource.value = 'online';
                
                // 缓存在线歌词
                onlineLyricsCache.set(track.path, {
                    lrc: onlineLrc,
                    parsed: parsed,
                    source: 'online'
                });
                
                // 只有在启用自动保存时才保存到本地
                if (configStore.lyrics?.autoSaveOnlineLyrics) {
                    const saved = await saveLyricsToLocal(track.path, onlineLrc);
                    if (saved) {
                        lyricsSource.value = 'local';
                        // 保存成功后从缓存中移除
                        onlineLyricsCache.delete(track.path);
                    }
                }
                return true;
            }
            return false;
        } catch (e) {
            console.error('Error fetching lyrics:', e);
            onlineLyricsError.value = e.message;
            return false;
        } finally {
            loading.value = false;
        }
    };

    watch(() => playerStore.currentTrack?.path, loadLyrics, { immediate: true });

    watchEffect(() => {
        // 应用歌词偏移：正值表示歌词提前（时间减小），负值表示歌词延后（时间增大）
        const offset = playerStore.lyricsOffset || 0;
        const currentTime = playerStore.currentTime + 0.05 - offset;
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
        activeIndex,
        lyricsSource,
        onlineLyricsError,
        fetchAndSaveLyrics,
        loadLyrics
    };
}
