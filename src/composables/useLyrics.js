import { ref, watch } from 'vue';
import { usePlayerStore } from '@/stores/player';
import { useConfigStore } from '@/stores/config';
import { FileUtils } from '@/utils/fileUtils';
import { neteaseApi } from '@/utils/neteaseApi';
import { invoke } from '@tauri-apps/api/core';
import logger from '@/utils/logger';

// 模块级别的在线歌词缓存，避免组件重新挂载时丢失
const onlineLyricsCache = new Map(); // key: trackPath, value: { lrc: string, parsed: array, source: string }

// 模块级别的共享状态，确保所有 useLyrics 实例共享同一个 lyricsSource
const sharedLyricsSource = ref('local');

// 让出主线程的辅助函数
const yieldToMain = () => new Promise(resolve => setTimeout(resolve, 0));

export function useLyrics() {
    const playerStore = usePlayerStore();
    const configStore = useConfigStore();
    const lyrics = ref([]);
    const loading = ref(false);
    const activeIndex = ref(-1);
    // 使用共享的 lyricsSource
    const lyricsSource = sharedLyricsSource;
    const onlineLyricsError = ref(null);

    // 异步 LRC 解析，分块处理避免阻塞主线程
    const parseLRC = async (lrcText) => {
        const lines = lrcText.split("\n");
        const pattern = /\[(\d{2}):(\d{2}):(\d{2})\]|\[(\d{2}):(\d{2})\.(\d{2,3})\]/g;
        const resultMap = {};
        const CHUNK_SIZE = 100; // 每 100 行让出一次主线程
        
        for (let i = 0; i < lines.length; i++) {
            // 分块让出主线程
            if (i > 0 && i % CHUNK_SIZE === 0) {
                await yieldToMain();
            }
            
            const line = lines[i];
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

    // 异步 ASS 解析，分块处理避免阻塞主线程
    const parseASS = async (assText) => {
        const lines = assText.split('\n');
        const dialogues = [];
        const toSeconds = (t) => {
            const [h, m, s] = t.split(':');
            return parseInt(h) * 3600 + parseInt(m) * 60 + parseFloat(s);
        };
        const CHUNK_SIZE = 100;
        
        for (let i = 0; i < lines.length; i++) {
            if (i > 0 && i % CHUNK_SIZE === 0) {
                await yieldToMain();
            }
            
            const line = lines[i];
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
            logger.debug('Fetching online lyrics for: ' + title + ' - ' + artist);
            const lyricsData = await neteaseApi.searchAndGetLyrics(title, artist, duration);
            if (!lyricsData || !lyricsData.lrc) {
                logger.debug('No online lyrics found');
                return null;
            }
            let lrcContent = lyricsData.lrc;
            if (configStore.lyrics?.preferTranslation && lyricsData.tlyric) {
                lrcContent = neteaseApi.mergeLyrics(lyricsData.lrc, lyricsData.tlyric);
            }
            return lrcContent;
        } catch (error) {
            logger.error('Failed to fetch online lyrics:', error);
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
            logger.info('Lyrics saved to: ' + lyricsPath);
            return true;
        } catch (error) {
            logger.error('Failed to save lyrics:', error);
            return false;
        }
    };

    const loadLyrics = async (trackPath) => {
        if (!trackPath) { 
            lyrics.value = []; 
            playerStore.lyrics = null;  // 同步到 store
            lyricsSource.value = 'local';
            onlineLyricsError.value = null;
            return; 
        }
        
        // 先检查缓存中是否有这首歌的在线歌词
        const cached = onlineLyricsCache.get(trackPath);
        if (cached) {
            logger.debug('Using cached online lyrics for:', trackPath);
            lyrics.value = cached.parsed;
            playerStore.lyrics = cached.parsed;  // 同步到 store
            lyricsSource.value = cached.source;
            loading.value = false;
            return;
        }
        
        loading.value = true;
        lyrics.value = [];
        playerStore.lyrics = null;  // 同步到 store
        lyricsSource.value = 'local';
        onlineLyricsError.value = null;
        try {
            const lyricsPath = await FileUtils.findLyricsFile(trackPath);
            if (lyricsPath) {
                const content = await FileUtils.readFile(lyricsPath);
                const ext = FileUtils.getFileExtension(lyricsPath);
                if (ext === 'lrc') lyrics.value = await parseLRC(content);
                else if (ext === 'ass') lyrics.value = await parseASS(content);
                playerStore.lyrics = lyrics.value;  // 同步到 store
                lyricsSource.value = 'local';
            } else if (configStore.lyrics?.enableOnlineFetch) {
                logger.debug('No local lyrics found, trying online fetch...');
                const track = playerStore.currentTrack;
                const onlineLrc = await fetchOnlineLyrics(track);
                if (onlineLrc) {
                    const parsed = await parseLRC(onlineLrc);
                    lyrics.value = parsed;
                    playerStore.lyrics = parsed;  // 同步到 store
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
            logger.error('Error loading lyrics:', e);
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
                const parsed = await parseLRC(onlineLrc);
                lyrics.value = parsed;
                playerStore.lyrics = parsed;  // 同步到 store
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
            logger.error('Error fetching lyrics:', e);
            onlineLyricsError.value = e.message;
            return false;
        } finally {
            loading.value = false;
        }
    };

    const stopWatchTrack = watch(() => playerStore.currentTrack?.path, loadLyrics, { immediate: true });

    // activeIndex 更新逻辑 - 使用节流避免高频更新
    // 这个值被 VisualizerPanel 等组件使用
    let lastActiveIndexUpdate = 0;
    const ACTIVE_INDEX_THROTTLE = 100; // 每 100ms 更新一次
    
    const stopWatchEffect = watch(
        () => playerStore.currentTime,
        (currentTime) => {
            if (!lyrics.value.length) {
                if (activeIndex.value !== -1) {
                    activeIndex.value = -1;
                    playerStore.currentLyricIndex = -1;
                }
                return;
            }
            
            // 节流：避免每次 currentTime 变化都计算
            const now = Date.now();
            if (now - lastActiveIndexUpdate < ACTIVE_INDEX_THROTTLE) return;
            lastActiveIndexUpdate = now;
            
            // 应用歌词偏移
            const offset = playerStore.lyricsOffset || 0;
            const adjustedTime = currentTime + 0.05 - offset;
            
            // 二分查找当前歌词索引
            let l = 0, r = lyrics.value.length - 1, idx = -1;
            while (l <= r) {
                const mid = (l + r) >> 1;
                if (lyrics.value[mid].time <= adjustedTime) {
                    idx = mid;
                    l = mid + 1;
                } else {
                    r = mid - 1;
                }
            }
            
            if (idx !== activeIndex.value) {
                activeIndex.value = idx;
                playerStore.currentLyricIndex = idx;
            }
        },
        { immediate: true }
    );

    // 清理函数
    const cleanup = () => {
        stopWatchTrack();
        stopWatchEffect();
    };

    return {
        lyrics,
        loading,
        activeIndex,
        lyricsSource,
        onlineLyricsError,
        fetchAndSaveLyrics,
        loadLyrics,
        cleanup
    };
}
