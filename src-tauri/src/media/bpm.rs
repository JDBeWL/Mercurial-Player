//! BPM（速度）识别：标签优先，缺标签时解码音频做节拍检测。
//!
//! 解码 8s 起的 60s 单声道 → 对数压缩频谱通量 → 白化 → 自相关枚举候选 → DP 跟踪 → 打分选层；首窗无结论顺序回退 28s / 48s，不做多窗投票。

use crate::android::saf;
use crate::error::AppError;
use crate::security::is_sensitive_path;
use lofty::prelude::TaggedFileExt;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use symphonia::core::audio::{Channels, GenericAudioBufferRef};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use super::metadata::cache::{cache_dir, get_file_modified_time, patch_cached_bpm};
use super::metadata::extractor::open_tagged_file;

/// 支持的速度下限（BPM）
pub const BPM_MIN: f64 = 60.0;
/// 支持的速度上限（BPM）：必须与 `TEMPO_MIN_PERIOD_SECS` 自洽（0.25s ↔ 240 BPM），
/// 再高就是在断言自相关表里不存在的速度；调低会让高速段只能以 ×1/2 或 ×2/3 出现。
pub const BPM_MAX: f64 = 240.0;

/// 落盘的 BPM 分析结果文件名（与 metadata-cache.json 同目录）
const BPM_CACHE_FILENAME: &str = "bpm-cache.json";
/// 缓存格式版本。检测算法变更时必须递增，否则旧条目（含大量负缓存）会一直生效。
const BPM_CACHE_VERSION: u32 = 12;

/// 开始收集采样前跳过的时长（秒）：避开开头的静音与长淡入；`pub` 供 `examples/bpm_diagnose.rs` 取默认值。
pub const ANALYSIS_SKIP_SECS: f64 = 8.0;
/// 参与分析的时长（秒）：太短拍点覆盖不足以估八度，太长会跨进变奏段落打断拍点链；
/// 60 秒的精确命中与跨起点一致性优于 40 秒。`pub` 供回归 runner 当默认值。
pub const ANALYSIS_WINDOW_SECS: f64 = 60.0;

/// 窗口数。生产路径只顺序回退（首窗有结论即用），`analyze_consensus` 才真的投票。
const ANALYSIS_VOTE_WINDOWS: usize = 3;
/// 相邻窗口起点间隔（秒）：8s / 28s / 48s；必须非 0，否则各窗内容重合，投出来的是同一个错误的三份副本。
const ANALYSIS_VOTE_STEP_SECS: f64 = 20.0;
/// 分析采样率上限（Hz）：更高的源率先抗混叠低通再整数倍抽取。
///
/// `FFT_WINDOW` 固定时 bin 宽随采样率线性增长，96 kHz 下最低可分析频率会抬到 94 Hz，
/// 把底鼓基频（50–80 Hz）整段滤掉。
const ANALYSIS_SAMPLE_RATE: u32 = 48_000;

/// 频谱分析窗长（采样数，2 的幂供 FFT）：23ms @44.1k，足以分辨 200BPM 的鼓点冲击
const FFT_WINDOW: usize = 1024;
/// 频谱分析的帧移（采样数）：窗长的一半，保证瞬态不会两次落在相邻帧的边界上
const FFT_HOP: usize = FFT_WINDOW / 2;

/// 源采样率对应的抽取倍数（1 = 不抽取）。Hi-Res 都是 44.1k / 48k 的整数倍。
fn decimation_factor(sample_rate: u32) -> usize {
    if sample_rate <= ANALYSIS_SAMPLE_RATE {
        return 1;
    }
    let factor = (sample_rate + ANALYSIS_SAMPLE_RATE / 2) / ANALYSIS_SAMPLE_RATE;
    factor.max(1) as usize
}

/// 实际参与分析的采样率（源率过高时是抽取后的）；`onset_envelope` 与 `frame_rate` 必须共用它，否则包络帧率会整体算错。
fn analysis_rate(sample_rate: u32) -> u32 {
    sample_rate / decimation_factor(sample_rate) as u32
}

/// 包络帧率（帧/秒）：帧移 `FFT_HOP` 个采样一帧，按实际分析率计算
fn frame_rate(sample_rate: u32) -> f64 {
    f64::from(analysis_rate(sample_rate)) / FFT_HOP as f64
}

/// 把过高的采样率抗混叠低通后整数倍抽取，等于 `ANALYSIS_SAMPLE_RATE` 以下时原样返回。
/// 只做整数倍抽取，无插值误差；低通不能省：8 kHz 以上的能量会折叠回低频段，凭空造出鼓点。
/// 截止频率取目标奈奎斯特的 0.45 倍，二阶巴特沃斯。
fn resample_for_analysis(mono: &[f32], sample_rate: u32) -> (std::borrow::Cow<'_, [f32]>, u32) {
    use std::borrow::Cow;
    let factor = decimation_factor(sample_rate);
    let target = analysis_rate(sample_rate);
    if factor <= 1 || mono.is_empty() {
        return (Cow::Borrowed(mono), sample_rate);
    }

    let cutoff = f64::from(target) * 0.5 * 0.45;
    let dt = 1.0 / f64::from(sample_rate);
    let rc = 1.0 / (std::f64::consts::TAU * cutoff);
    let alpha = (dt / (rc + dt)) as f32;

    let mut stage1 = 0.0_f32;
    let mut stage2 = 0.0_f32;
    let mut out = Vec::with_capacity(mono.len() / factor + 1);
    for (index, sample) in mono.iter().enumerate() {
        stage1 += alpha * (*sample - stage1);
        stage2 += alpha * (stage1 - stage2);
        if index % factor == 0 {
            out.push(stage2);
        }
    }
    (Cow::Owned(out), target)
}
/// 通量谱的下限频率（Hz）：滤掉直流与次声抖动
const FLUX_MIN_HZ: f32 = 30.0;
/// 通量谱的上限频率（Hz）：鼓点能量都在 8k 以内，再往上是齿音与噪声
const FLUX_MAX_HZ: f32 = 8_000.0;
/// 对数压缩系数：压缩响度动态范围，避免安静段落的鼓点被响段人声淹没
const FLUX_LOG_SCALE: f32 = 1_000.0;
/// 自适应白化的滑动均值窗长（秒）：太长抹不掉段落动态，太短会把拍点本身减掉
const WHITEN_SECS: f64 = 0.4;
/// 包络至少要有多少秒才做分析：更短的样本里统计噪声会压过周期性
const MIN_ENVELOPE_SECS: f64 = 4.0;

/// 自相关搜索的最短拍长（秒）：0.25s = 240 BPM。比 `BPM_MIN` 宽，才能看见倍频峰
const TEMPO_MIN_PERIOD_SECS: f64 = 0.25;
/// 自相关搜索的最长拍长（秒）：2.0s = 30 BPM，同理
const TEMPO_MAX_PERIOD_SECS: f64 = 2.0;

/// 动态规划转移代价的刚度：代价 `-tightness·ln(滞后/目标拍长)²`，偏离 5% 几乎免费、偏离一倍则远高于
/// 起音强度，所以单次跟踪不会自己跳八度——八度只能由候选枚举决定。`pub` 供回归 runner 扫描。
pub const DP_TIGHTNESS: f64 = 30.0;
/// 判定两个速度是同一假设的最大相对偏离（候选去重与共识折层都用它）。
const TEMPO_DEDUPE_RATIO: f64 = 0.02;
/// 候选速度的保留阈值（相对最强候选的相关值）：低于它的峰不参与枚举，避免噪声假设浪费算力
const CANDIDATE_RELATIVE_FLOOR: f64 = 0.30;
/// 最多枚举多少个自相关峰（每个再展开成 3 个倍频）
const MAX_TEMPO_SEEDS: usize = 3;

/// 速度先验的倍频宽（以 2 为底的对数单位）：1.2 时 60 BPM 的权重约为 120 的 0.81，过窄（0.8）会压掉快速曲目。
const TEMPO_PRIOR_OCTAVES: f64 = 1.2;
/// 速度先验的中心（BPM）：多数流行/舞曲落在这附近
const TEMPO_PRIOR_CENTER: f64 = 120.0;

/// 可信度下限：胜出假设的拍点平均起音强度（包络峰值归一化为 1）。0.06 落在真实音乐
/// （5% 分位 0.083）与噪声/持续音（≤0.05）之间；自相关信噪比在两簇间重叠，不能用来做这个判断。
const MIN_CONFIDENCE: f64 = 0.06;

/// 单条批量分析结果：`bpm` 为 `None` 表示这首确实没测出（不是失败，前端不必重试）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmAnalysisResult {
    pub path: String,
    pub bpm: Option<f64>,
}

/// 节拍检测结果
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatEstimate {
    /// 每分钟拍数
    pub bpm: f64,
    /// 可信度（拍点平均起音强度），供调用方决定要不要采信
    pub confidence: f64,
}

/// 解析标签里的 BPM 文本：兼容 `"128"`、`"128.5"`、`"128 BPM"`、`"BPM:96"`、`"120-128"`（取中值），越界或解析不出返回 `None`。
#[must_use]
pub fn parse_bpm_text(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    // 负号开头的 BPM 无意义，只可能是垃圾标签（"-128" 不该被读成 128）
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    // 区间写法取中点；只有两侧都能解析成数字才当区间，否则回落到"取第一个数字"
    if let Some((left, right)) = trimmed.split_once('-') {
        if let (Some(a), Some(b)) = (parse_bpm_number(left), parse_bpm_number(right)) {
            let mid = a + (b - a) / 2.0;
            return is_plausible_bpm(mid).then(|| round_to_tenth(mid));
        }
    }
    let value = parse_bpm_number(trimmed)?;
    is_plausible_bpm(value).then(|| round_to_tenth(value))
}

/// 抠出文本里的第一个数字（允许小数，允许前后带单位/字母）
fn parse_bpm_number(text: &str) -> Option<f64> {
    let start = text.char_indices().find(|(_, c)| c.is_ascii_digit())?.0;
    let rest = &text[start..];
    let end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && *c != '.')
        .map_or(rest.len(), |(idx, _)| idx);
    rest[..end].trim_end_matches('.').parse::<f64>().ok()
}

/// BPM 是否落在可展示的合理区间（0 与越界值一律视为无效标签）
fn is_plausible_bpm(value: f64) -> bool {
    value.is_finite() && (30.0..=300.0).contains(&value)
}

/// 一位小数取整，避免把 `128.0000000001` 这种东西写进缓存或界面
fn round_to_tenth(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// 读取文件标签里的 BPM（ID3 `TBPM` / Vorbis `BPM` / MP4 `tmpo` / APE `BPM`），没有则 `None`。
/// 只认 `ItemKey::Bpm` 与 `ItemKey::IntegerBpm`：lofty 已把各格式的键映射好。
#[must_use]
pub fn read_tagged_bpm(path: &str) -> Option<f64> {
    let tagged = open_tagged_file(path).ok()?;
    tagged.primary_tag().and_then(read_tagged_bpm_from_tag)
}

/// 从已打开的标签里取 BPM（元数据提取时标签已在内存里，不必为了 BPM 再开一次文件）
#[must_use]
pub fn read_tagged_bpm_from_tag(tag: &lofty::tag::Tag) -> Option<f64> {
    for key in [ItemKey::Bpm, ItemKey::IntegerBpm] {
        if let Some(text) = tag.get_string(key) {
            if let Some(bpm) = parse_bpm_text(text) {
                return Some(round_to_tenth(bpm));
            }
        }
    }
    None
}

/// 只查分析缓存，不做任何解码；文件已改动或没分析过时返回 `None`。
/// 元数据提取路径会在扫描上千首时随便调用，不能碰音频解码（与 `resolve` 的分工）。
#[must_use]
pub fn cached_value(path: &str) -> Option<f64> {
    match lookup_cache(path) {
        CacheLookup::Analyzed(bpm) => bpm,
        CacheLookup::Unknown => None,
    }
}

/// 分析一首曲目的 BPM：标签 > 节拍模型 > 本模块缓存 > 现算。
/// 返回 `None` 表示"确实没有可用结果"，这个结论也会写进缓存（负缓存），避免反复解码。
/// 节拍模型排在旧缓存之前：旧缓存可能存着已知错误的结果，先返回它会让模型分析白跑。
pub fn resolve(path: &str) -> Option<f64> {
    if is_sensitive_path(path) && !saf::is_content_uri(path) {
        return None;
    }
    // 标签不落缓存：改了就应立刻生效，而读标签只走一次 Probe::open
    if let Some(tagged) = read_tagged_bpm(path) {
        return Some(tagged);
    }
    // 神经网络整曲分析的结果：它有小节线这一维信息，层级判断比单窗可靠
    if let Some(value) = super::beat_cache::cached(path).and_then(|track| track.bpm) {
        return Some(value);
    }
    // 最后才看单窗算法的历史结论（含负缓存），避免重复解码
    if let CacheLookup::Analyzed(known) = lookup_cache(path) {
        return known;
    }
    let detected = detect_from_audio(path).map(|e| e.bpm);
    record_cache(path, detected);
    detected
}

/// 对一首曲目做实际的音频分析（不含标签与缓存），失败时返回 `None`。
/// 首窗出结论就用；前奏是环境音/念白/静音的曲目顺延到 28s / 48s，不要改成多窗投票（`analyze_consensus` 仅是诊断口径）。
fn detect_from_audio(path: &str) -> Option<BeatEstimate> {
    for index in 0..ANALYSIS_VOTE_WINDOWS {
        let skip = ANALYSIS_SKIP_SECS + index as f64 * ANALYSIS_VOTE_STEP_SECS;
        if let Some(estimate) = analyze_window(path, skip) {
            if index > 0 {
                log::debug!("BPM: {skip}s 窗口才测出节拍（前 {index} 个窗口无结论）");
            }
            return Some(estimate);
        }
    }
    log::debug!("BPM 分析未测出明显节拍 {path}（已尝试全部回退窗口）");
    None
}

/// 在指定起点解码并分析一个窗口；测不出（解码失败或低于可信度阈值）返回 `None`
fn analyze_window(path: &str, skip_secs: f64) -> Option<BeatEstimate> {
    let analysis = match decode_analysis_window_with(path, skip_secs, ANALYSIS_WINDOW_SECS) {
        Ok(analysis) => analysis,
        Err(e) => {
            log::debug!("BPM 分析解码失败 {path}（起点 {skip_secs}s）: {e}");
            return None;
        }
    };
    let frame_rate = frame_rate(analysis.sample_rate);
    let envelope = onset_envelope(&analysis.mono, analysis.sample_rate);
    let estimate = estimate_from_envelope(&envelope, frame_rate);
    if let Some(estimate) = estimate {
        log::debug!(
            "BPM 分析结果 {path}（起点 {skip_secs}s）: {} (可信度 {:.2})",
            estimate.bpm,
            estimate.confidence
        );
    }
    estimate
}

/// 生产口径的分析入口（单窗口）：不读标签、不碰缓存，诊断工具用它拿"用户实际会看到的值"。
#[must_use]
pub fn analyze_production(path: &str) -> Option<BeatEstimate> {
    detect_from_audio(path)
}

/// 三窗口共识，仅诊断用，不要接回 `resolve`：两个窗口同时锁错层级时多数票毫无作用，只保留它做并排比较。
#[must_use]
pub fn analyze_consensus(path: &str) -> Option<BeatEstimate> {
    let mut readings: Vec<BeatEstimate> = Vec::new();
    for index in 0..ANALYSIS_VOTE_WINDOWS {
        let skip = ANALYSIS_SKIP_SECS + index as f64 * ANALYSIS_VOTE_STEP_SECS;
        let Ok(analysis) = decode_analysis_window_with(path, skip, ANALYSIS_WINDOW_SECS) else {
            continue;
        };
        let frame_rate = frame_rate(analysis.sample_rate);
        let envelope = onset_envelope(&analysis.mono, analysis.sample_rate);
        if let Some(estimate) = estimate_from_envelope(&envelope, frame_rate) {
            readings.push(estimate);
        }
    }
    match readings.len() {
        0 => None,
        1 => readings.first().copied(),
        _ => Some(consensus(&readings)),
    }
}

/// 多窗口共识：把每个读数折到候选的八度上投票，取票数最多的层级，再在胜出层级上取几何平均。
/// 不能统一折到某个固定区间（会把真实的高速曲目整体压成一半）；平票时先看谁离 `TEMPO_PRIOR_CENTER` 近，再看可信度。
fn consensus(readings: &[BeatEstimate]) -> BeatEstimate {
    let tolerance = TEMPO_DEDUPE_RATIO;
    let mut best_votes = 0_usize;
    let mut best_distance = f64::INFINITY;
    let mut best_confidence = f64::NEG_INFINITY;
    let mut best_folded: Vec<f64> = Vec::new();

    for candidate in readings {
        if candidate.bpm <= 0.0 {
            continue;
        }
        let mut folded: Vec<(f64, f64)> = Vec::new();
        for reading in readings {
            if reading.bpm <= 0.0 {
                continue;
            }
            let octaves = (reading.bpm / candidate.bpm).log2().round();
            let value = reading.bpm / 2f64.powf(octaves);
            if ((value - candidate.bpm) / candidate.bpm).abs() <= tolerance {
                folded.push((value, reading.confidence));
            }
        }
        if folded.is_empty() {
            continue;
        }
        let confidence = folded
            .iter()
            .map(|&(_, confidence)| confidence)
            .fold(0.0, f64::max);
        let distance = (candidate.bpm / TEMPO_PRIOR_CENTER).log2().abs();

        let better = folded.len() > best_votes
            || (folded.len() == best_votes && distance < best_distance - 1e-9)
            || (folded.len() == best_votes
                && (distance - best_distance).abs() <= 1e-9
                && confidence > best_confidence);
        if better {
            best_votes = folded.len();
            best_distance = distance;
            best_confidence = confidence;
            best_folded = folded.iter().map(|&(value, _)| value).collect();
        }
    }

    if best_folded.is_empty() {
        // 没有任何两个读数能对上：退回第一个，行为与单窗一致
        return readings[0];
    }
    let count = best_folded.len() as f64;
    let mean_log = best_folded.iter().map(|value| value.ln()).sum::<f64>() / count;
    BeatEstimate {
        bpm: round_to_tenth(mean_log.exp()),
        confidence: best_confidence,
    }
}

/// 频谱通量 → 起音强度包络，并做自适应白化：逐帧 FFT，只累积幅度增大的方向（持续音不产生通量，鼓点这类瞬态才会）。
/// Hi-Res 先抽取，对数压缩避免响段淹没安静段，白化减去 `WHITEN_SECS` 的滑动均值以去掉段落响度偏置。
fn onset_envelope(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    let (resampled, sample_rate) = resample_for_analysis(mono, sample_rate);
    let mono: &[f32] = &resampled;
    if mono.len() < FFT_WINDOW {
        return Vec::new();
    }

    let bin_hz = f64::from(sample_rate) / FFT_WINDOW as f64;
    let min_bin = ((f64::from(FLUX_MIN_HZ) / bin_hz).floor() as usize).max(1);
    let max_bin = ((f64::from(FLUX_MAX_HZ) / bin_hz).ceil() as usize).min(FFT_WINDOW / 2 - 1);
    if min_bin >= max_bin {
        return Vec::new();
    }

    // microfft 的实 FFT 输出不归一化，幅度随窗长线性增长；先除窗长让 FLUX_LOG_SCALE 与采样率/窗长解耦
    let scale = FLUX_LOG_SCALE / FFT_WINDOW as f32;

    // 预计算 Hann 窗：不加窗时帧边界的突然截断会在频谱里制造宽带噪声，淹没鼓点
    let window: Vec<f32> = (0..FFT_WINDOW)
        .map(|i| {
            let x = std::f32::consts::TAU * i as f32 / (FFT_WINDOW - 1) as f32;
            0.5 - 0.5 * x.cos()
        })
        .collect();

    let mut prev = vec![0.0_f32; max_bin + 1];
    let mut current = vec![0.0_f32; max_bin + 1];
    let mut flux = Vec::with_capacity(mono.len() / FFT_HOP + 1);
    let mut frame = [0.0_f32; FFT_WINDOW];
    let mut start = 0;

    while start + FFT_WINDOW <= mono.len() {
        for i in 0..FFT_WINDOW {
            frame[i] = mono[start + i] * window[i];
        }
        let spectrum = microfft::real::rfft_1024(&mut frame);

        let mut sum = 0.0_f32;
        for bin in min_bin..=max_bin {
            let re = spectrum[bin].re;
            let im = spectrum[bin].im;
            // 对数压缩 ln(1+λx)：大值近对数、小值近线性，安静段仍灵敏且不被爆音拉爆
            let mag = (re.hypot(im) * scale).ln_1p();
            current[bin] = mag;
            // 只累积增大方向（半波整流）：节拍是能量"出现"，不是"消失"
            let diff = mag - prev[bin];
            if diff > 0.0 {
                sum += diff;
            }
        }
        flux.push(sum);
        std::mem::swap(&mut prev, &mut current);
        start += FFT_HOP;
    }

    whiten(&flux, frame_rate(sample_rate))
}

/// 自适应白化：减去滑动均值后半波整流，再按最大值归一化。
/// 不能按均值归一化：转移代价是绝对量，峰值到 10–25 时单个强拍就能抵消"偏离拍长"的代价，拍点会脱离网格。
fn whiten(flux: &[f32], frame_rate: f64) -> Vec<f32> {
    if flux.is_empty() {
        return Vec::new();
    }
    let radius = ((WHITEN_SECS * frame_rate).round() as usize).max(1);
    let mut prefix = Vec::with_capacity(flux.len() + 1);
    prefix.push(0.0_f64);
    for value in flux {
        prefix.push(prefix[prefix.len() - 1] + f64::from(*value));
    }
    let mut whitened = Vec::with_capacity(flux.len());
    for (i, value) in flux.iter().enumerate() {
        let lo = i.saturating_sub(radius);
        let hi = (i + radius + 1).min(flux.len());
        let local_mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
        let residual = f64::from(*value) - local_mean;
        whitened.push(if residual > 0.0 { residual } else { 0.0 });
    }

    let peak = whitened.iter().copied().fold(0.0_f64, f64::max);
    if peak > f64::EPSILON {
        for value in &mut whitened {
            *value /= peak;
        }
    }
    whitened.into_iter().map(|v| v as f32).collect()
}

/// 一次节拍跟踪的结果
#[derive(Debug, Clone)]
struct BeatTrack {
    /// 拍点所在帧号（升序）
    beats: Vec<usize>,
    /// 回归得到的拍长（帧，可为小数）
    period: f64,
    /// 拍点的平均起音强度（单位为包络均值），见 `beat_strength`
    strength: f64,
    /// 拍点对包络能量的富集度，见 `beat_enrichment`
    enrichment: f64,
    /// 相邻拍点强度的交替显著性，见 `alternating_salience`
    salience: f64,
    /// 被跳过位置的起音强度比，见 `skipped_onset_ratio`（仅诊断，不参与打分）
    skipped_ratio: f64,
    /// 拍点链的刚性（相邻间隔相对拍长的平均绝对偏差），见 `beat_interval_mad`
    regularity: f64,
    /// 拍点覆盖的包络能量占比，见 `beat_coverage`
    coverage: f64,
    /// 拍点邻域覆盖的起音能量占比，见 `beat_recall`
    recall: f64,
}

/// Ellis 动态规划节拍跟踪：`C[t] = O[t] + max_τ(C[t-τ] - tightness·ln(τ/P)²)`，在给定目标拍长下求全局最优拍点序列。
/// 选它而不是直接取自相关峰，是为了整段共用一条自洽的拍点链，局部弱拍/切分不会把拍点带偏。
/// 返回 `None` 表示包络太短或拍数不足；`frame_rate` 只用于把 `BEAT_MATCH_TOLERANCE_SECS` 折算成帧数。
fn dp_beat_track(onset: &[f64], period: f64, tightness: f64, frame_rate: f64) -> Option<BeatTrack> {
    let frames = onset.len();
    if period < 2.0 || frames < 8 {
        return None;
    }
    let min_lag = (period * 0.5).round().max(1.0) as usize;
    let max_lag = (period * 2.0).round() as usize;
    if max_lag >= frames {
        return None;
    }

    // 预计算转移代价：只填 [min_lag, max_lag]，前面留 0 占位（滞后不会小于 min_lag）
    let mut cost = vec![0.0_f64; max_lag + 1];
    for (lag, slot) in cost.iter_mut().enumerate().take(max_lag + 1).skip(min_lag) {
        let ratio = lag as f64 / period;
        *slot = -tightness * ratio.ln().powi(2);
    }

    let mut cumulative = vec![0.0_f64; frames];
    let mut back = vec![usize::MAX; frames];
    for t in 0..frames {
        let mut best = f64::NEG_INFINITY;
        let mut best_prev = usize::MAX;
        let hi = max_lag.min(t);
        for lag in min_lag..=hi {
            let candidate = cumulative[t - lag] + cost[lag];
            if candidate > best {
                best = candidate;
                best_prev = t - lag;
            }
        }
        if best_prev == usize::MAX {
            // 序列开头还没有足够历史，把当前帧当作链条起点
            cumulative[t] = onset[t];
        } else {
            cumulative[t] = onset[t] + best;
            back[t] = best_prev;
        }
    }

    // 终点取最后两拍窗口内累积得分最高的帧：既保证覆盖到窗口末尾，又不需要知道总长
    let tail_floor = frames.saturating_sub(max_lag);
    let mut cursor = tail_floor;
    let mut best_score = f64::NEG_INFINITY;
    for (offset, value) in cumulative[tail_floor..].iter().enumerate() {
        if *value > best_score {
            best_score = *value;
            cursor = tail_floor + offset;
        }
    }

    let mut beats = vec![cursor];
    while back[cursor] != usize::MAX {
        cursor = back[cursor];
        beats.push(cursor);
    }
    beats.reverse();
    if beats.len() < 4 {
        return None;
    }

    let period_estimate = regression_period(&beats)?;

    Some(BeatTrack {
        strength: beat_strength(onset, &beats),
        enrichment: beat_enrichment(onset, &beats),
        salience: alternating_salience(onset, &beats),
        skipped_ratio: skipped_onset_ratio(onset, &beats, period_estimate),
        regularity: beat_interval_mad(&beats, period_estimate),
        coverage: beat_coverage(onset, &beats),
        recall: beat_recall(onset, &beats, frame_rate),
        period: period_estimate,
        beats,
    })
}

/// 对拍点序列做最小二乘回归，得到亚帧精度的拍长（整数帧的量化误差被平均掉）。
fn regression_period(beats: &[usize]) -> Option<f64> {
    let n = beats.len();
    if n < 4 {
        return None;
    }
    let n_f = n as f64;
    let mean_index = (n_f - 1.0) / 2.0;
    let mean_position = beats.iter().sum::<usize>() as f64 / n_f;
    let mut numerator = 0.0_f64;
    let mut denominator = 0.0_f64;
    for (i, position) in beats.iter().enumerate() {
        let x = i as f64 - mean_index;
        numerator += x * (*position as f64 - mean_position);
        denominator += x * x;
    }
    if denominator <= f64::EPSILON {
        return None;
    }
    let period = numerator / denominator;
    (period > 0.0).then_some(period)
}

/// 拍点的平均起音强度（包络已归一化到峰值 1），`candidate_score` 的精度项：惩罚落在空处的多余拍点。
/// 单独用会偏袒慢速（可挑的拍点更少、均值更高），必须与 `beat_recall` 成对使用。
fn beat_strength(onset: &[f64], beats: &[usize]) -> f64 {
    if beats.is_empty() {
        return 0.0;
    }
    let sum: f64 = beats
        .iter()
        .map(|position| onset.get(*position).copied().unwrap_or(0.0))
        .sum();
    sum / beats.len() as f64
}

/// 拍内 / 拍间对比度：拍点平均起音 ÷ 半拍处平均起音；仅诊断，不参与打分。
/// 绝对强度不能跨速度比较（拍点越密均值越低），这个比值与密度无关；换掉精度项前先用它量分离度。
fn beat_contrast(onset: &[f64], beats: &[usize], period: f64) -> f64 {
    if beats.len() < 2 || period <= 0.0 {
        return 0.0;
    }
    let half = (period * 0.5).round().max(1.0) as usize;
    let mut on_sum = 0.0;
    let mut off_sum = 0.0;
    let mut counted = 0_usize;
    for &beat in beats {
        let (Some(&on), Some(&off)) = (onset.get(beat), onset.get(beat + half)) else {
            continue;
        };
        on_sum += on;
        off_sum += off;
        counted += 1;
    }
    // 拍间完全安静时给 0 会让"最干净"的假设反而垫底，所以保底返回一个大但有限的值
    if counted == 0 {
        return 0.0;
    }
    if off_sum <= f64::EPSILON {
        return 99.0;
    }
    on_sum / off_sum
}

/// 拍点对包络能量的富集度：拍点能量占比 ÷ 随机落点占比；仅诊断，不参与打分。
fn beat_enrichment(onset: &[f64], beats: &[usize]) -> f64 {
    if beats.is_empty() || onset.is_empty() {
        return 0.0;
    }
    let total: f64 = onset.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    let on_beat: f64 = beats
        .iter()
        .map(|position| onset.get(*position).copied().unwrap_or(0.0))
        .sum();
    let random_fraction = beats.len() as f64 / onset.len() as f64;
    (on_beat / total / random_fraction).max(0.0)
}

/// 相邻拍点强度的交替显著性：`|Σ(-1)^i·s_i| / Σ s_i`，0 = 完全均匀，1 = 严格强弱交替。
/// 仅诊断，不参与打分：真实音乐里切分、加花造成的强弱变化与"错八度"并不同构。
fn alternating_salience(onset: &[f64], beats: &[usize]) -> f64 {
    if beats.len() < 4 {
        return 0.0;
    }
    let mut alternating = 0.0_f64;
    let mut total = 0.0_f64;
    for (index, position) in beats.iter().enumerate() {
        let value = onset.get(*position).copied().unwrap_or(0.0);
        total += value;
        if index % 2 == 0 {
            alternating += value;
        } else {
            alternating -= value;
        }
    }
    if total <= f64::EPSILON {
        return 0.0;
    }
    (alternating / total).abs()
}

/// 被跳过位置的起音强度比：拍点之间的"中点"强度 ÷ 拍点本身强度；仅诊断，不要再用它打分。
/// 真实音乐的反拍上常有踩镲、军鼓、扫弦，它衡量的是"反拍编曲密度"而不是"是否漏拍"，已被 `beat_recall` 取代。
fn skipped_onset_ratio(onset: &[f64], beats: &[usize], period: f64) -> f64 {
    if beats.len() < 2 || period < 2.0 {
        return 1.0;
    }
    let half = (period * 0.5).round().max(1.0) as usize;
    let mut on_sum = 0.0_f64;
    let mut off_sum = 0.0_f64;
    let mut off_count = 0_usize;
    for beat in beats {
        on_sum += onset.get(*beat).copied().unwrap_or(0.0);
        let offset = beat + half;
        if offset < onset.len() {
            off_sum += onset.get(offset).copied().unwrap_or(0.0);
            off_count += 1;
        }
    }
    if off_count == 0 || on_sum <= f64::EPSILON {
        return 1.0;
    }
    // 两边都按各自的样本数取均值再相比，否则拍点少一半的假设会被系统性压低
    let on_mean = on_sum / beats.len() as f64;
    let off_mean = off_sum / off_count as f64;
    off_mean / on_mean.max(f64::EPSILON)
}

/// 拍点链的刚性：相邻拍间隔相对回归拍长的平均绝对偏差（已归一化）。
///
/// 仅诊断，不参与打分：动态规划本身就把拍点链约束成近似等间距，
/// 这个量测的是"规划有多听话"，不是"假设对不对"。
fn beat_interval_mad(beats: &[usize], period: f64) -> f64 {
    if beats.len() < 3 || period < 1.0 {
        return 1.0;
    }
    let mut sum = 0.0_f64;
    let mut count = 0_usize;
    for pair in beats.windows(2) {
        sum += ((pair[1] - pair[0]) as f64 - period).abs();
        count += 1;
    }
    if count == 0 {
        return 1.0;
    }
    sum / count as f64 / period
}

/// 拍点覆盖了包络总能量的多少（单帧统计，无邻域容差）。
///
/// 仅诊断，不参与打分：天然偏袒更快的假设。实际打分用 `beat_recall`。
fn beat_coverage(onset: &[f64], beats: &[usize]) -> f64 {
    let total: f64 = onset.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    let on_beat: f64 = beats
        .iter()
        .map(|position| onset.get(*position).copied().unwrap_or(0.0))
        .sum();
    (on_beat / total).clamp(0.0, 1.0)
}

/// 判定"某个起音落在拍点附近"的容差（秒，不是帧）：35 ms 约占最小拍长的 14%。
/// 不能写死帧数：帧率随源采样率变化（44.1k → 86，96k → 188），固定帧数会让八度判别失效。
const BEAT_MATCH_TOLERANCE_SECS: f64 = 0.035;

/// 拍点召回：落在拍点 ±容差内的起音能量占总能量的比例。
///
/// 按邻域而不是单帧统计（鼓点位置有几十毫秒浮动）。它天然偏袒更快的假设，
/// 必须与 `beat_strength` 配对成 F 度量，见 `candidate_score`。
fn beat_recall(onset: &[f64], beats: &[usize], frame_rate: f64) -> f64 {
    if beats.is_empty() || onset.is_empty() {
        return 0.0;
    }
    let total: f64 = onset.iter().sum();
    if total <= f64::EPSILON {
        return 0.0;
    }
    // 容差（帧）。至少 1 帧，否则低帧率下会退化成"逐帧比对"，失去容差的意义
    let tolerance = ((BEAT_MATCH_TOLERANCE_SECS * frame_rate).round() as usize).max(1);
    // 前缀和求邻域和。拍点间隔远大于 2×容差，窗口不会重叠，可以直接相加而不必去重
    let mut prefix = Vec::with_capacity(onset.len() + 1);
    prefix.push(0.0_f64);
    for value in onset {
        prefix.push(prefix[prefix.len() - 1] + value);
    }
    let mut covered = 0.0_f64;
    for beat in beats {
        let lo = beat.saturating_sub(tolerance);
        let hi = (beat + tolerance + 1).min(onset.len());
        covered += prefix[hi] - prefix[lo];
    }
    (covered / total).clamp(0.0, 1.0)
}

fn standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance =
        values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / values.len() as f64;
    variance.sqrt()
}

/// 无偏自相关（除以重叠长度），只算到 `max_lag`。
///
/// 下标 0 存包络的均方值（零延迟自相关），供 `normalized_autocorrelation` 归一化。
fn autocorrelation(envelope: &[f64], max_lag: usize) -> Vec<f64> {
    let mut result = vec![0.0_f64; max_lag + 1];
    if envelope.is_empty() {
        return result;
    }
    result[0] = envelope.iter().map(|v| v * v).sum::<f64>() / envelope.len() as f64;
    for lag in 1..=max_lag.min(envelope.len().saturating_sub(1)) {
        let len = envelope.len() - lag;
        let mut acc = 0.0_f64;
        for i in 0..len {
            acc += envelope[i] * envelope[i + lag];
        }
        result[lag] = acc / len as f64;
    }
    result
}

/// 归一化自相关：把零延迟处的均方值当分母，结果是相关系数（上限 1）。
///
/// 它仍轻微偏袒快速假设，所以指数压到 `TEMPO_PERIODICITY_WEIGHT`。
#[must_use]
fn normalized_autocorrelation(correlations: &[f64], lag: usize) -> f64 {
    let energy = correlations.first().copied().unwrap_or(0.0);
    if energy <= f64::EPSILON {
        return 0.0;
    }
    correlations.get(lag).copied().unwrap_or(0.0) / energy
}

/// 速度先验：以 `TEMPO_PRIOR_CENTER` 为中心的对数正态权重
fn tempo_prior(bpm: f64) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }
    let octaves = (bpm / TEMPO_PRIOR_CENTER).log2() / TEMPO_PRIOR_OCTAVES;
    (-0.5 * octaves * octaves).exp()
}

/// 候选速度：自相关里的局部极大，每个再展开成倍频族（1/2、1、2 倍）。
///
/// 自相关在 T、T/2、2T 处都有峰，哪个最高取决于编曲；真正的取舍交给 `dp_beat_track`。
fn tempo_candidates(correlations: &[f64], frame_rate: f64) -> Vec<f64> {
    let min_lag = ((TEMPO_MIN_PERIOD_SECS * frame_rate).floor() as usize).max(2);
    let max_lag = ((TEMPO_MAX_PERIOD_SECS * frame_rate).ceil() as usize)
        .min(correlations.len().saturating_sub(2));
    if min_lag >= max_lag {
        return Vec::new();
    }

    let mut peaks: Vec<(usize, f64)> = Vec::new();
    for lag in (min_lag + 1)..max_lag {
        let value = correlations[lag];
        if value <= 0.0 || value < correlations[lag - 1] || value < correlations[lag + 1] {
            continue;
        }
        peaks.push((lag, value * tempo_prior(60.0 * frame_rate / lag as f64)));
    }
    if peaks.is_empty() {
        return Vec::new();
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let floor = peaks[0].1 * CANDIDATE_RELATIVE_FLOOR;

    let mut candidates: Vec<f64> = Vec::new();
    for (lag, score) in peaks.iter().take(MAX_TEMPO_SEEDS * 3) {
        if *score < floor {
            break;
        }
        let bpm = 60.0 * frame_rate / *lag as f64;
        for factor in [0.5_f64, 1.0, 2.0] {
            let candidate = bpm * factor;
            if !(BPM_MIN..=BPM_MAX).contains(&candidate) {
                continue;
            }
            // 2% 以内视为同一个假设，避免同一速度被反复评估
            if candidates
                .iter()
                .any(|existing| (existing - candidate).abs() / candidate < TEMPO_DEDUPE_RATIO)
            {
                continue;
            }
            candidates.push(candidate);
        }
        if candidates.len() >= MAX_TEMPO_SEEDS * 3 {
            break;
        }
    }
    candidates
}

/// 一次完整分析的中间结果：不做阈值判定，交调用方决定要不要采信
#[doc(hidden)]
pub struct AnalysisDetail {
    /// 每个候选的评估明细，按得分降序
    pub evaluated: Vec<CandidateScore>,
    /// 胜出候选的 BPM（未取整）
    bpm: f64,
    /// 胜出候选的拍点平均起音强度（可信度用它）
    strength: f64,
    /// 胜出候选的拍点帧号
    beats: Vec<usize>,
    /// 胜出候选每个拍点的起音强度，与 `beats` 一一对应（`--meter` 诊断用）
    #[doc(hidden)]
    pub beat_strengths: Vec<f64>,
}

/// 分析包络：估候选速度，再对每个候选跑节拍跟踪，按得分选出最佳假设。
fn analyze(envelope: &[f32], frame_rate: f64) -> Option<AnalysisDetail> {
    analyze_with(envelope, frame_rate, DP_TIGHTNESS)
}

/// 同 `analyze`，但可指定动态规划刚度与打分权重（仅诊断/标定用，生产走 `analyze`）
fn analyze_with(envelope: &[f32], frame_rate: f64, tightness: f64) -> Option<AnalysisDetail> {
    if frame_rate <= 0.0 || envelope.len() < (frame_rate * MIN_ENVELOPE_SECS) as usize {
        return None;
    }
    let onset: Vec<f64> = envelope.iter().map(|v| f64::from(*v)).collect();
    let std_dev = standard_deviation(&onset);
    if std_dev <= f64::EPSILON {
        return None;
    }

    let max_lag =
        ((TEMPO_MAX_PERIOD_SECS * frame_rate).ceil() as usize).min(onset.len().saturating_sub(2));
    let correlations = autocorrelation(&onset, max_lag);
    let candidates = tempo_candidates(&correlations, frame_rate);
    if candidates.is_empty() {
        return None;
    }

    let mut evaluated: Vec<CandidateScore> = Vec::with_capacity(candidates.len());
    // 保留每个候选的拍点链：提升层级时要换用另一条链，不能只留胜出的那条
    let mut tracks: Vec<BeatTrack> = Vec::with_capacity(candidates.len());
    for bpm in candidates {
        let period = 60.0 * frame_rate / bpm;
        // 目标拍长取整后再跟踪：动态规划只能在整数帧上落拍，用小数目标反而会让代价函数失真
        let Some(track) = dp_beat_track(&onset, period.round().max(1.0), tightness, frame_rate)
        else {
            continue;
        };
        // 周期性取自相关在假设速度对应滞后处的值，而不是回归拍长处：假设拍长是整数帧，
        // 自相关表只有整数滞后，取回归值会取到相邻的两格之一，结果随四舍五入跳变
        let periodicity =
            normalized_autocorrelation(&correlations, period.round().max(1.0) as usize);
        let score = candidate_score(track.strength, track.recall, periodicity, bpm);
        evaluated.push(CandidateScore {
            bpm,
            score,
            strength: track.strength,
            enrichment: track.enrichment,
            salience: track.salience,
            skipped_ratio: track.skipped_ratio,
            periodicity,
            regularity: track.regularity,
            coverage: track.coverage,
            recall: track.recall,
            contrast: beat_contrast(&onset, &track.beats, period.round().max(1.0)),
            beats: track.beats.len(),
        });
        tracks.push(track);
    }
    if evaluated.is_empty() {
        return None;
    }

    let mut best_index = 0_usize;
    for (index, candidate) in evaluated.iter().enumerate() {
        if candidate.score > evaluated[best_index].score {
            best_index = index;
        }
    }
    let chosen = promote_to_quarter_level(&evaluated, best_index);
    let track = tracks.into_iter().nth(chosen)?;
    // 用回归拍长而不是候选的整数帧速度：前者有亚帧精度（144.9 而不是 144.2）
    let bpm = 60.0 * frame_rate / track.period;

    evaluated.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let beat_strengths: Vec<f64> = track
        .beats
        .iter()
        .map(|&frame| onset.get(frame).copied().unwrap_or(0.0))
        .collect();
    Some(AnalysisDetail {
        evaluated,
        bpm,
        strength: track.strength,
        beats: track.beats,
        beat_strengths,
    })
}

/// 把胜出候选提升到 3/2 或 2 倍层级。
/// 复合拍号听感差 1.5 倍，数据库对高能量曲目常记 double-time；只有强度没塌、召回显著更高才提升（见 `LEVEL_UP_RATIOS`）。
fn promote_to_quarter_level(scores: &[CandidateScore], best: usize) -> usize {
    let top = &scores[best];
    if top.bpm <= 0.0 || top.recall <= f64::EPSILON {
        return best;
    }

    // 先看 3/2（复合拍号的附点拍），再看 2 倍（数据库惯例里的 double-time）。
    // 两者互斥：真值是 2 倍时，1.5 倍的候选拿不出同样的证据。
    for (ratio, strength_floor, recall_gain) in LEVEL_UP_RATIOS {
        let target = ratio.log2();
        let mut promoted = None;
        let mut best_strength = f64::NEG_INFINITY;
        for (index, candidate) in scores.iter().enumerate() {
            if index == best || candidate.bpm <= 0.0 {
                continue;
            }
            if ((candidate.bpm / top.bpm).log2() - target).abs() > LEVEL_UP_TOLERANCE {
                continue;
            }
            if candidate.strength < strength_floor * top.strength {
                continue;
            }
            if candidate.recall < recall_gain * top.recall {
                continue;
            }
            // 多个候选都满足时取强度最高的那个（最贴近"每一拍都有起音"）
            if candidate.strength > best_strength {
                promoted = Some(index);
                best_strength = candidate.strength;
            }
        }
        if let Some(index) = promoted {
            return index;
        }
    }
    best
}

/// 从分析结果里取可用结论：阈值不过或超出范围时返回 `None`
fn accept(detail: &AnalysisDetail) -> Option<BeatEstimate> {
    if detail.strength < MIN_CONFIDENCE {
        return None;
    }
    if !(BPM_MIN..=BPM_MAX).contains(&detail.bpm) {
        return None;
    }
    Some(BeatEstimate {
        bpm: round_to_tenth(detail.bpm),
        confidence: detail.strength,
    })
}

fn estimate_from_envelope(envelope: &[f32], frame_rate: f64) -> Option<BeatEstimate> {
    analyze(envelope, frame_rate).as_ref().and_then(accept)
}

/// 单个候选速度的评估结果
#[derive(Debug, Clone, Copy)]
pub struct CandidateScore {
    /// 假设速度（BPM）
    pub bpm: f64,
    /// 综合得分（越大越好）
    pub score: f64,
    /// 拍点平均起音强度（单位为包络均值）
    pub strength: f64,
    /// 拍点对包络能量的富集度（可信度用它）
    pub enrichment: f64,
    /// 相邻拍点强度的交替显著性（八度判别用它）
    pub salience: f64,
    /// 被跳过位置的起音强度比（仅诊断，不参与打分）
    pub skipped_ratio: f64,
    /// 归一化自相关（周期性强弱，上限 1），见 `normalized_autocorrelation`
    pub periodicity: f64,
    /// 拍点链刚性（越小越刚性），见 `beat_interval_mad`
    pub regularity: f64,
    /// 拍点覆盖的包络能量占比
    pub coverage: f64,
    /// 拍点邻域覆盖的起音能量占比（召回），见 `beat_recall`
    pub recall: f64,
    /// 拍内 / 拍间对比度（仅诊断，不参与打分），见 `beat_contrast`
    pub contrast: f64,
    /// 拍点数量
    pub beats: usize,
}

/// 提升到更高层级时允许的比值误差（对数域），覆盖候选从整数滞后算出的量化误差。
const LEVEL_UP_TOLERANCE: f64 = 0.05;

/// 允许提升的层级与证据门槛：(倍数, 强度下限, 召回下限)，均为相对胜出候选的比值。
/// 阈值余量很薄（3/2 只留约 2%），改动前先补标注样本重标。
const LEVEL_UP_RATIOS: [(f64, f64, f64); 2] = [(1.5, 0.80, 1.25), (2.0, 0.85, 1.75)];

/// 召回项在 F 度量里的相对权重（β²）。0.35 略偏精度，避免整库被推向高速端。
const RECALL_WEIGHT: f64 = 0.35;
/// 周期性在综合得分里的指数。不能为 0（合成信号靠它压住错误八度），
/// 也不能开到 1.0（它对快速假设单调偏高）。
const TEMPO_PERIODICITY_WEIGHT: f64 = 0.4;

/// 候选速度的综合得分。
///
/// `F(精度, 召回) × 周期性^0.4 × 先验^权重`：精度与召回各自单向偏袒，必须成对出现；
/// 调和平均对任一项偏低都敏感，正好把两个方向同时顶住。先验只用于打破证据均势。
fn candidate_score(strength: f64, recall: f64, periodicity: f64, bpm: f64) -> f64 {
    let s = strength.max(0.0);
    let r = recall.max(0.0);
    // 加权调和平均：s == r 时结果恰好等于 s，任一项趋近 0 时结果也趋近 0
    let combined = if s + r <= f64::EPSILON {
        0.0
    } else {
        (1.0 + RECALL_WEIGHT) * s * r / (RECALL_WEIGHT * s + r)
    };
    combined
        * periodicity.max(0.0).powf(TEMPO_PERIODICITY_WEIGHT)
        * tempo_prior(bpm).powf(TEMPO_PRIOR_WEIGHT)
}

/// 速度先验在综合得分里的指数。
/// 必须很弱：调强会系统性压掉高速曲目，去掉则失去破平局能力；由 `slow_tempo_survives_the_prior` / `fast_tempo_survives_the_prior` 守住。
const TEMPO_PRIOR_WEIGHT: f64 = 2.0;

// TEMP_PROFILE: 临时剖析接口，剖析完删除

/// 临时：解码分析窗口（供性能剖析）
#[doc(hidden)]
#[must_use]
pub fn profile_decode(path: &str) -> Option<AnalysisSamples> {
    decode_analysis_window(path).ok()
}

/// 临时：计算起音包络（供性能剖析）
#[doc(hidden)]
#[must_use]
pub fn profile_envelope(mono: &[f32], sample_rate: u32) -> Vec<f32> {
    onset_envelope(mono, sample_rate)
}

/// 临时：包络帧率（供性能剖析）
#[doc(hidden)]
#[must_use]
pub fn profile_frame_rate(sample_rate: u32) -> f64 {
    frame_rate(sample_rate)
}

/// 临时：跑一遍打分（供性能剖析）
#[doc(hidden)]
#[must_use]
pub fn profile_analyze(envelope: &[f32], frame_rate: f64) -> Option<AnalysisDetail> {
    analyze(envelope, frame_rate)
}

/// 节拍检测的中间量，供诊断工具判断"没测出"卡在哪一步
#[derive(Debug, Clone)]
pub struct BpmDiagnostics {
    pub sample_rate: u32,
    pub samples: usize,
    pub envelope_len: usize,
    pub frame_rate: f64,
    /// 每个候选假设的评估明细
    pub evaluated: Vec<CandidateScore>,
    /// 胜出假设的拍点平均起音强度
    pub strength: Option<f64>,
    /// 当前生效的可信度阈值
    pub threshold: f64,
    /// 胜出假设的可信度；未测出时为 `None`
    pub confidence: Option<f64>,
    /// 通过阈值时的结论；未通过为 `None`
    pub bpm: Option<f64>,
    /// 胜出假设每个拍点的起音强度（与 `beats` 一一对应），供 `--meter` 做重音周期分析
    pub beat_strengths: Vec<f64>,
    /// 胜出假设的拍点帧号，供诊断核对
    pub beats: Vec<usize>,
}

/// 诊断工具展示用的阈值（与 `MIN_CONFIDENCE` 同值，只是暴露给外部打印）
pub const DIAG_THRESHOLD: f64 = MIN_CONFIDENCE;

/// 对一首曲目跑一遍分析并返回中间量；只用于诊断，不写缓存
#[must_use]
pub fn diagnose(path: &str) -> Option<BpmDiagnostics> {
    diagnose_with(path, 0.0, ANALYSIS_WINDOW_SECS)
}

/// 诊断用：可指定跳过时长与分析时长（秒），用于比较不同窗口下的表现
#[must_use]
pub fn diagnose_with(path: &str, skip_secs: f64, window_secs: f64) -> Option<BpmDiagnostics> {
    diagnose_tuned(path, skip_secs, window_secs, DP_TIGHTNESS)
}

/// 诊断用：在 `diagnose_with` 之上再放开动态规划刚度，用于标定
#[must_use]
pub fn diagnose_tuned(
    path: &str,
    skip_secs: f64,
    window_secs: f64,
    tightness: f64,
) -> Option<BpmDiagnostics> {
    let analysis = decode_analysis_window_with(path, skip_secs, window_secs).ok()?;
    let frame_rate = frame_rate(analysis.sample_rate);
    let envelope = onset_envelope(&analysis.mono, analysis.sample_rate);

    // 无论最终是否出值，都把每个候选的评估结果带出来，否则"为什么选了 190 而不是 95"没法查
    let detail = analyze_with(&envelope, frame_rate, tightness);
    let (accepted, confidence, evaluated, beats, beat_strengths) = match &detail {
        Some(detail) => (
            accept(detail).map(|estimate| estimate.bpm),
            Some(detail.strength),
            detail.evaluated.clone(),
            detail.beats.clone(),
            detail.beat_strengths.clone(),
        ),
        None => (None, None, Vec::new(), Vec::new(), Vec::new()),
    };

    Some(BpmDiagnostics {
        sample_rate: analysis.sample_rate,
        samples: analysis.mono.len(),
        envelope_len: envelope.len(),
        frame_rate,
        evaluated,
        strength: detail.as_ref().map(|d| d.strength),
        threshold: MIN_CONFIDENCE,
        confidence,
        bpm: accepted,
        beats,
        beat_strengths,
    })
}

/// 交错多声道 -> 单声道：逐帧取平均
fn downmix_interleaved(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let mut mono = Vec::with_capacity(samples.len() / channels);
    for frame in samples.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() / channels as f32);
    }
    mono
}

/// 解码出的分析素材（单声道 + 采样率）
#[doc(hidden)]
pub struct AnalysisSamples {
    pub mono: Vec<f32>,
    pub sample_rate: u32,
}

/// 解码开头一段音频用于节拍检测。
///
/// 不复用播放链路的 `SymphoniaDecoder`：那个会预填充几百毫秒并起解码线程，
/// 这里只要定长采样，且要能在后台任务里随意并发调用。
fn decode_analysis_window(path: &str) -> Result<AnalysisSamples, AppError> {
    decode_analysis_window_with(path, ANALYSIS_SKIP_SECS, ANALYSIS_WINDOW_SECS)
}

/// 同 `decode_analysis_window`，但可指定跳过时长与分析时长（诊断/调参用）
fn decode_analysis_window_with(
    path: &str,
    skip_secs: f64,
    window_secs: f64,
) -> Result<AnalysisSamples, AppError> {
    let file = saf::open_media_file(path)?;
    let mss = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| AppError::Audio(format!("BPM 分析探测格式失败: {e}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| AppError::Audio("BPM 分析: 没有音频轨".to_string()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(CodecParameters::audio)
        .cloned()
        .ok_or_else(|| AppError::Audio("BPM 分析: 缺少解码参数".to_string()))?;
    let sample_rate = params.sample_rate.unwrap_or(44_100).max(1);
    let channels = params.channels.as_ref().map_or(2, Channels::count).max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| AppError::Audio(format!("BPM 分析创建解码器失败: {e}")))?;

    let sample_rate_f = f64::from(sample_rate);
    let skip_frames = (skip_secs * sample_rate_f) as u64;
    let want_frames = ((skip_secs + window_secs) * sample_rate_f) as u64;
    let mut mono: Vec<f32> = Vec::with_capacity((window_secs * sample_rate_f) as usize);
    let mut scratch: Vec<f32> = Vec::new();
    let mut seen_frames: u64 = 0;

    while seen_frames < want_frames {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                // 单个坏包不该让整首曲目的 BPM 归零：能解多少算多少
                log::debug!("BPM 分析读包中断 {path}: {e}");
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio_buf = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => {
                log::debug!("BPM 分析解码中断 {path}: {e}");
                break;
            }
        };
        let frames = audio_buf.frames() as u64;
        if frames == 0 {
            continue;
        }

        // 只在跨过跳过点之后才开始收集，避免把开头的静音/淡入算进包络
        let previous = seen_frames;
        seen_frames = seen_frames.saturating_add(frames);
        if seen_frames <= skip_frames {
            continue;
        }

        scratch.clear();
        append_mono(audio_buf, &mut scratch, channels);
        if seen_frames > want_frames {
            // 最后一个包可能越界：按帧数截断（scratch 是单声道，一帧一个采样）
            let keep = (want_frames - previous) as usize;
            scratch.truncate(keep.min(scratch.len()));
        }
        mono.extend_from_slice(&scratch);
    }

    if mono.is_empty() {
        return Err(AppError::Audio("BPM 分析: 未能解码出采样".to_string()));
    }
    Ok(AnalysisSamples { mono, sample_rate })
}

/// 把一个解码缓冲下混为单声道并追加到 `out`
fn append_mono(decoded: GenericAudioBufferRef<'_>, out: &mut Vec<f32>, channels: usize) {
    decoded.copy_to_vec_interleaved(out);
    if channels > 1 {
        let downmixed = downmix_interleaved(out, channels);
        out.clear();
        out.extend_from_slice(&downmixed);
    }
}

/// 单条分析结果：文件修改时间用于失效，`bpm` 为 `None` 表示"分析过但没测出来"
#[derive(Debug, Serialize, Deserialize, Clone)]
struct BpmEntry {
    modified_time: u64,
    bpm: Option<f64>,
    analyzed_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BpmCache {
    version: u32,
    entries: std::collections::HashMap<String, BpmEntry>,
}

impl BpmCache {
    fn empty() -> Self {
        Self {
            version: BPM_CACHE_VERSION,
            entries: std::collections::HashMap::new(),
        }
    }
}

/// 内存中的分析结果；与 metadata 缓存一样，内存是写盘的权威来源
static BPM_CACHE: RwLock<Option<BpmCache>> = RwLock::new(None);

fn bpm_cache_path() -> PathBuf {
    cache_dir().join(BPM_CACHE_FILENAME)
}

fn load_cache_from_disk() -> BpmCache {
    let path = bpm_cache_path();
    let Ok(content) = fs::read_to_string(&path) else {
        return BpmCache::empty();
    };
    match serde_json::from_str::<BpmCache>(&content) {
        Ok(cache) if cache.version == BPM_CACHE_VERSION => cache,
        Ok(_) => {
            log::info!("BPM 缓存版本不匹配，重新创建");
            BpmCache::empty()
        }
        Err(e) => {
            log::warn!("BPM 缓存解析失败（将重建）: {e}");
            BpmCache::empty()
        }
    }
}

/// 缓存查询结果。用专用枚举而不是 `Option<Option<f64>>`：要区分"查过没有"与"测没测出"。
enum CacheLookup {
    /// 没有分析记录（没分析过，或文件已改动导致旧结论作废）
    Unknown,
    /// 分析过：`Some(bpm)` 为测得值，`None` 表示明确没测出
    Analyzed(Option<f64>),
}

/// 查缓存；同时把"文件已改动"的陈旧条目就地删掉
fn lookup_cache(path: &str) -> CacheLookup {
    let Some(current) = get_file_modified_time(path) else {
        return CacheLookup::Unknown;
    };
    let mut guard = lock_or_log!(BPM_CACHE.write());
    if guard.is_none() {
        *guard = Some(load_cache_from_disk());
    }
    let Some(cache) = guard.as_mut() else {
        return CacheLookup::Unknown;
    };
    let Some(entry) = cache.entries.get(path) else {
        return CacheLookup::Unknown;
    };
    if entry.modified_time != current {
        // 文件已替换：旧结论作废，下次重新分析
        cache.entries.remove(path);
        return CacheLookup::Unknown;
    }
    let bpm = entry.bpm;
    drop(guard);
    CacheLookup::Analyzed(bpm)
}

/// 记一条分析结果，并同步写回元数据缓存（扫描/播放页读的是那份）
fn record_cache(path: &str, bpm: Option<f64>) {
    let Some(modified_time) = get_file_modified_time(path) else {
        return;
    };
    let analyzed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    {
        let mut guard = lock_or_log!(BPM_CACHE.write());
        if guard.is_none() {
            *guard = Some(load_cache_from_disk());
        }
        if let Some(cache) = guard.as_mut() {
            cache.entries.insert(
                path.to_string(),
                BpmEntry {
                    modified_time,
                    bpm,
                    analyzed_at,
                },
            );
        }
    }

    patch_cached_bpm(path, bpm);
}

/// 把内存里的分析结果落盘（批量分析结束后调一次即可）
pub fn flush_bpm_cache() -> Result<(), AppError> {
    let snapshot = {
        let lock = lock_or_log!(BPM_CACHE.read());
        lock.clone()
    };
    let Some(cache) = snapshot else {
        return Ok(());
    };

    let path = bpm_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::msg(format!("创建 BPM 缓存目录失败: {e}")))?;
    }
    let content = serde_json::to_string(&cache)
        .map_err(|e| AppError::msg(format!("序列化 BPM 缓存失败: {e}")))?;
    fs::write(&path, content).map_err(|e| AppError::msg(format!("写入 BPM 缓存失败: {e}")))?;
    Ok(())
}

/// 清空 BPM 缓存（内存 + 磁盘）。清空元数据缓存时一并调用，否则两份缓存会互相矛盾
pub fn clear_bpm_cache() -> Result<(), AppError> {
    {
        let mut guard = lock_or_log!(BPM_CACHE.write());
        *guard = Some(BpmCache::empty());
    }
    let path = bpm_cache_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|e| AppError::msg(format!("删除 BPM 缓存失败: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_decorated_bpm_text() {
        assert_eq!(parse_bpm_text("128"), Some(128.0));
        assert_eq!(parse_bpm_text(" 128.5 "), Some(128.5));
        assert_eq!(parse_bpm_text("128 BPM"), Some(128.0));
        assert_eq!(parse_bpm_text("BPM: 96"), Some(96.0));
        assert_eq!(parse_bpm_text("128.0"), Some(128.0));
    }

    #[test]
    fn parses_range_as_midpoint() {
        assert_eq!(parse_bpm_text("120-128"), Some(124.0));
        assert_eq!(parse_bpm_text("90 - 100"), Some(95.0));
    }

    #[test]
    fn rejects_empty_zero_and_out_of_range() {
        assert_eq!(parse_bpm_text(""), None);
        assert_eq!(parse_bpm_text("   "), None);
        assert_eq!(parse_bpm_text("0"), None);
        assert_eq!(parse_bpm_text("12"), None);
        assert_eq!(parse_bpm_text("400"), None);
        assert_eq!(parse_bpm_text("fast"), None);
        assert_eq!(parse_bpm_text("-128"), None);
    }

    #[test]
    fn beat_contrast_separates_on_beat_from_off_beat() {
        // 拍上 1.0、拍间 0.1：对比度应为 10，且与拍点密度无关
        let mut env = vec![0.0_f64; 200];
        for i in (0..200).step_by(4) {
            env[i] = 1.0;
            env[i + 2] = 0.1;
        }
        let beats: Vec<usize> = (0..200).step_by(4).collect();
        let contrast = beat_contrast(&env, &beats, 4.0);
        assert!((contrast - 10.0).abs() < 0.01, "应为 10，实际 {contrast}");

        // 拍与拍间一样强：说明这个假设的"拍间"其实也是拍，对比度掉到 1
        let flat = vec![1.0_f64; 200];
        let contrast = beat_contrast(&flat, &beats, 4.0);
        assert!((contrast - 1.0).abs() < 0.01, "应为 1，实际 {contrast}");

        // 拍点不足或周期非法时不能除零
        assert!(beat_contrast(&flat, &[], 4.0).abs() < f64::EPSILON);
        assert!(beat_contrast(&flat, &beats, 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn consensus_rejects_a_lone_octave_flip() {
        // 孤立的 ×1/2 读数必须被投掉，而不是把结果拉低
        let readings = |values: &[(f64, f64)]| -> Vec<BeatEstimate> {
            values
                .iter()
                .map(|&(bpm, confidence)| BeatEstimate { bpm, confidence })
                .collect()
        };
        let result = consensus(&readings(&[(125.0, 0.3), (124.0, 0.3), (94.0, 0.3)]));
        assert!(
            (result.bpm - 124.5).abs() < 1.0,
            "应保持在 124 附近，实际 {}",
            result.bpm
        );

        let result = consensus(&readings(&[(150.0, 0.3), (150.0, 0.3), (75.0, 0.3)]));
        assert!(
            (result.bpm - 150.0).abs() < 1.0,
            "应保持 150，实际 {}",
            result.bpm
        );
    }

    /// 快曲不能被"折到低八度"：三个窗口一致报 178 时必须还是 178
    #[test]
    fn consensus_keeps_unanimous_fast_tempo() {
        let readings: Vec<BeatEstimate> = [178.0, 179.0, 178.5]
            .iter()
            .map(|&bpm| BeatEstimate {
                bpm,
                confidence: 0.3,
            })
            .collect();
        let result = consensus(&readings);
        assert!(
            (result.bpm - 178.5).abs() < 1.0,
            "一致的高速读数不能被折半，实际 {}",
            result.bpm
        );
    }

    /// 两票对一票时按多数走，且结果落在多数那边
    #[test]
    fn consensus_follows_the_majority_octave() {
        let readings: Vec<BeatEstimate> = [88.0, 89.0, 176.0]
            .iter()
            .map(|&bpm| BeatEstimate {
                bpm,
                confidence: 0.25,
            })
            .collect();
        let result = consensus(&readings);
        assert!(
            (result.bpm - 88.5).abs() < 1.5,
            "两票在 88 侧，结果应靠近 88，实际 {}",
            result.bpm
        );
    }

    /// 只有一个读数时原样返回，不引入任何偏移
    #[test]
    fn consensus_single_reading_passes_through() {
        let result = consensus(&[BeatEstimate {
            bpm: 117.5,
            confidence: 0.2,
        }]);
        assert!((result.bpm - 117.5).abs() < f64::EPSILON);
    }

    #[test]
    fn level_up_rule_promotes_only_with_strong_evidence() {
        fn candidate(bpm: f64, strength: f64, recall: f64) -> CandidateScore {
            CandidateScore {
                bpm,
                score: strength,
                strength,
                enrichment: 0.0,
                salience: 0.0,
                skipped_ratio: 0.0,
                periodicity: 0.0,
                regularity: 0.0,
                coverage: 0.0,
                recall,
                contrast: 0.0,
                beats: 0,
            }
        }

        let scores = vec![
            candidate(122.28, 0.1933, 0.2887),
            candidate(187.50, 0.1868, 0.4396),
        ];
        assert_eq!(promote_to_quarter_level(&scores, 0), 1, "证据充分时应提升");

        let scores = vec![
            candidate(89.0, 0.3000, 0.4000),
            candidate(142.0, 0.1600, 0.3600),
        ];
        assert_eq!(promote_to_quarter_level(&scores, 0), 0, "强度腰斩不应提升");

        // 强度够但召回没涨：说明快网格没有覆盖到更多真实起音
        let scores = vec![
            candidate(120.0, 0.2000, 0.3000),
            candidate(180.0, 0.1900, 0.3100),
        ];
        assert_eq!(promote_to_quarter_level(&scores, 0), 0, "召回没涨不应提升");

        // 比值不是 1.5（这里是 1.4 倍）不能提升
        let scores = vec![
            candidate(120.0, 0.2000, 0.3000),
            candidate(168.0, 0.1900, 0.5000),
        ];
        assert_eq!(
            promote_to_quarter_level(&scores, 0),
            0,
            "非 3/2 比值不应提升"
        );
    }

    #[test]
    fn octave_rule_needs_more_evidence_than_three_halves() {
        fn candidate(bpm: f64, strength: f64, recall: f64) -> CandidateScore {
            CandidateScore {
                bpm,
                score: strength,
                strength,
                enrichment: 0.0,
                salience: 0.0,
                skipped_ratio: 0.0,
                periodicity: 0.0,
                regularity: 0.0,
                coverage: 0.0,
                recall,
                contrast: 0.0,
                beats: 0,
            }
        }

        let scores = vec![
            candidate(85.2, 0.2000, 0.2500),
            candidate(170.0, 0.1720, 0.4500),
        ];
        assert_eq!(promote_to_quarter_level(&scores, 0), 1, "证据充分时应翻倍");

        let scores = vec![
            candidate(89.3, 0.2000, 0.2600),
            candidate(179.0, 0.1620, 0.4500),
        ];
        assert_eq!(
            promote_to_quarter_level(&scores, 0),
            0,
            "强度 0.81 不足以翻倍"
        );
    }

    #[test]
    fn tempo_prior_favours_mid_range() {
        assert!(tempo_prior(120.0) > tempo_prior(60.0));
        assert!(tempo_prior(120.0) > tempo_prior(240.0));
        assert!(tempo_prior(0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn downmix_averages_channels() {
        let interleaved = vec![1.0, 0.0, 0.0, 1.0, -1.0, 1.0];
        assert_eq!(downmix_interleaved(&interleaved, 2), vec![0.5, 0.5, 0.0]);
        assert_eq!(downmix_interleaved(&interleaved, 1), interleaved);
    }

    #[test]
    fn regression_recovers_fractional_period() {
        // 43.4 帧一拍：动态规划只能在整数帧落拍，所以这里模拟"在 43/44 之间交替"
        let beats: Vec<usize> = (0..40)
            .map(|i| (i as f64 * 43.4).round() as usize)
            .collect();
        let period = regression_period(&beats).expect("应能回归出拍长");
        assert!((period - 43.4).abs() < 0.05, "回归得到 {period}");
    }

    /// 合成一段鼓点：每拍一个低频衰减脉冲，正拍比反拍响（模拟底鼓/军鼓的区别）
    ///
    /// 正反拍的强度差是八度判据的事实依据，所以合成信号必须把它做进去，
    /// 否则测出来的"检测正确"不能证明算法在真实音乐上也成立。
    fn synth_beat_track(sample_rate: u32, bpm: f64, seconds: usize) -> Vec<f32> {
        let beat_samples = (f64::from(sample_rate) * 60.0 / bpm) as usize;
        let total = sample_rate as usize * seconds;
        let mut mono = vec![0.0_f32; total];
        let beats = total / beat_samples;
        for beat in 0..beats {
            // 正拍 1.0、反拍 0.55：真实音乐里两者的响度差大致也是这个量级
            let amplitude = if beat % 2 == 0 { 1.0_f32 } else { 0.55 };
            let start = beat * beat_samples;
            let len = 2_000.min(total - start);
            for i in 0..len {
                let t = i as f32 / sample_rate as f32;
                // 60Hz 基频 + 快速衰减：能量集中在低频段，与底鼓一致
                mono[start + i] =
                    amplitude * (-t * 60.0).exp() * (t * 2.0 * std::f32::consts::PI * 60.0).sin();
            }
        }
        mono
    }

    fn detect(mono: &[f32], sample_rate: u32) -> Option<BeatEstimate> {
        let fps = frame_rate(sample_rate);
        let envelope = onset_envelope(mono, sample_rate);
        estimate_from_envelope(&envelope, fps)
    }

    /// 120 BPM 点击音应稳定落回 120
    #[test]
    fn detects_synthetic_click_track() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 120.0, 30);
        let estimate = detect(&mono, sample_rate).expect("应能测出 120 BPM");
        assert!(
            (estimate.bpm - 120.0).abs() < 3.0,
            "期望约 120 BPM，实际 {}",
            estimate.bpm
        );
    }

    /// 90 BPM 不能被拉成 180 或 45（倍频歧义）
    #[test]
    fn detects_slower_click_track() {
        let sample_rate = 48_000u32;
        let mono = synth_beat_track(sample_rate, 90.0, 30);
        let estimate = detect(&mono, sample_rate).expect("应能测出 90 BPM");
        assert!(
            (estimate.bpm - 90.0).abs() < 4.0,
            "期望约 90 BPM，实际 {}",
            estimate.bpm
        );
    }

    /// 150 BPM 也不能被压成 75
    #[test]
    fn detects_faster_click_track() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 150.0, 30);
        let estimate = detect(&mono, sample_rate).expect("应能测出 150 BPM");
        assert!(
            (estimate.bpm - 150.0).abs() < 5.0,
            "期望约 150 BPM，实际 {}",
            estimate.bpm
        );
    }

    /// 慢速曲目（60 BPM）不能被速度先验推到 120：这是 `TEMPO_PRIOR_WEIGHT` 的上界守卫
    #[test]
    fn slow_tempo_survives_the_prior() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 60.0, 60);
        let estimate = detect(&mono, sample_rate).expect("应能测出 60 BPM");
        assert!(
            (estimate.bpm - 60.0).abs() < 4.0,
            "期望约 60 BPM，实际 {}（先验权重可能过大）",
            estimate.bpm
        );
    }

    /// 快速曲目（180 BPM）也不能被先验拉回 120
    #[test]
    fn fast_tempo_survives_the_prior() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 180.0, 60);
        let estimate = detect(&mono, sample_rate).expect("应能测出 180 BPM");
        assert!(
            (estimate.bpm - 180.0).abs() < 5.0,
            "期望约 180 BPM，实际 {}（先验权重可能过大）",
            estimate.bpm
        );
    }

    /// 从不同起点分析同一段信号，结果必须一致——这是旧版最严重的问题
    #[test]
    fn detection_is_stable_across_start_offsets() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 128.0, 60);
        let fps = frame_rate(sample_rate);
        let readings: Vec<f64> = [0.0_f64, 7.0, 15.0]
            .iter()
            .map(|offset| {
                let start = (offset * f64::from(sample_rate)) as usize;
                let envelope = onset_envelope(&mono[start..], sample_rate);
                estimate_from_envelope(&envelope, fps)
                    .expect("每个起点都应能测出")
                    .bpm
            })
            .collect();
        for value in &readings {
            assert!(
                (value - 128.0).abs() < 3.0,
                "三个起点应给出一致结果，实际 {readings:?}"
            );
        }
    }

    /// 召回：拍点邻域覆盖的起音能量；漏掉真拍必须被扣分
    #[test]
    fn recall_drops_when_real_beats_are_skipped() {
        // 44.1k 的帧率，容差按秒折算后仍是 3 帧
        let fps = frame_rate(44_100);
        let mut onset = vec![0.0_f64; 200];
        // 每 20 帧一个真拍
        let real: Vec<usize> = (0..10).map(|i| i * 20).collect();
        for beat in &real {
            onset[*beat] = 1.0;
        }
        // 全部踩中：召回接近 1
        let full = beat_recall(&onset, &real, fps);
        assert!(full > 0.95, "全部踩中应接近 1，实际 {full}");
        // 隔一个漏一个（模拟"太慢"的假设）：一半能量被漏掉
        let half: Vec<usize> = real.iter().step_by(2).copied().collect();
        let skipped = beat_recall(&onset, &half, fps);
        assert!(
            (skipped - 0.5).abs() < 0.1,
            "漏掉一半真拍时召回应约 0.5，实际 {skipped}"
        );
    }

    /// 召回按邻域统计：真拍位置浮动几十毫秒不该被当成漏拍
    #[test]
    fn recall_tolerates_small_timing_offsets() {
        let fps = frame_rate(44_100);
        let mut onset = vec![0.0_f64; 200];
        let real: Vec<usize> = (0..10).map(|i| i * 20).collect();
        for beat in &real {
            onset[*beat] = 1.0;
        }
        // 拍点整体偏 2 帧（仍在按秒折算出的容差内）
        let shifted: Vec<usize> = real.iter().map(|b| b + 2).collect();
        let recall = beat_recall(&onset, &shifted, fps);
        assert!(recall > 0.95, "小幅偏移不应被当成漏拍，实际 {recall}");
    }

    /// 容差必须按秒折算，不能写死帧数——否则帧率变化时容差的真实时长会漂移
    #[test]
    fn recall_tolerance_scales_with_frame_rate() {
        // 同一段"每 20 帧一个真拍"的包络，在两种帧率下偏移同样的秒数
        let mut onset = vec![0.0_f64; 400];
        let real: Vec<usize> = (0..20).map(|i| i * 20).collect();
        for beat in &real {
            onset[*beat] = 1.0;
        }

        // 44.1k 与 48k 都低于抽取阈值，帧率是真实的 86 / 94 帧/秒
        let fps_low = frame_rate(44_100);
        let fps_high = frame_rate(48_000);
        assert!(fps_high > fps_low, "48k 的帧率应略高于 44.1k");

        // 偏移 30 ms：两种帧率下都应仍算"踩中"
        let shift_secs = 0.030;
        let shifted_low: Vec<usize> = real
            .iter()
            .map(|b| b + (shift_secs * fps_low).round() as usize)
            .collect();
        let shifted_high: Vec<usize> = real
            .iter()
            .map(|b| b + (shift_secs * fps_high).round() as usize)
            .collect();
        let recall_low = beat_recall(&onset, &shifted_low, fps_low);
        let recall_high = beat_recall(&onset, &shifted_high, fps_high);
        assert!(
            recall_low > 0.9 && recall_high > 0.9,
            "同样的 30 ms 偏移在两种帧率下都应被容忍，实际 {recall_low} / {recall_high}"
        );

        // 折算出的容差帧数应随帧率增长（写死常数就不会）
        let tol_low = (BEAT_MATCH_TOLERANCE_SECS * fps_low).round();
        let tol_high = (BEAT_MATCH_TOLERANCE_SECS * fps_high).round();
        assert!(
            tol_high >= tol_low,
            "容差帧数应随帧率增长，否则就是写死了帧数：{tol_low} vs {tol_high}"
        );
    }

    /// Hi-Res 必须折算回标定用的时间-频率格子：固定 1024 点时 bin 宽随采样率增长，
    /// 96 kHz 下底鼓基频会被滤掉——这是高采样率曲目判错八度的根因。
    #[test]
    fn high_sample_rates_are_decimated_into_the_calibrated_grid() {
        // 帧率被拉回标定附近，而不再是随源率翻倍
        assert!((frame_rate(44_100) - 86.1).abs() < 0.5);
        assert!((frame_rate(96_000) - 93.8).abs() < 1.0);
        assert!(
            frame_rate(96_000) < frame_rate(44_100) * 1.2,
            "96k 抽取后的帧率应与 CD 接近，实际 {}",
            frame_rate(96_000)
        );

        // 抽取倍数：44.1k/48k 不动，88.2k/96k 折半，176.4k/192k 折到四分之一
        assert_eq!(decimation_factor(44_100), 1);
        assert_eq!(decimation_factor(48_000), 1);
        assert_eq!(decimation_factor(88_200), 2);
        assert_eq!(decimation_factor(96_000), 2);
        assert_eq!(decimation_factor(176_400), 4);
        assert_eq!(decimation_factor(192_000), 4);

        // 抽取后 bin 宽回到 43–47 Hz，最低可分析频率不再被抬高
        for rate in [44_100u32, 48_000, 88_200, 96_000, 192_000] {
            let effective = analysis_rate(rate);
            let bin_hz = f64::from(effective) / FFT_WINDOW as f64;
            assert!(
                bin_hz < 50.0,
                "采样率 {rate} 抽取到 {effective} 后 bin 宽应低于 50Hz，实际 {bin_hz:.1}"
            );
        }

        // 输出长度按倍数缩短，且滤波后不再有超出目标奈奎斯特的能量
        let mono: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 0.05).sin() + (i as f32 * 1.7).sin())
            .collect();
        let (resampled, rate) = resample_for_analysis(&mono, 96_000);
        assert_eq!(rate, 48_000);
        assert_eq!(resampled.len(), mono.len() / 2);
        // 低于阈值时原样借用，不复制
        let (unchanged, rate) = resample_for_analysis(&mono, 44_100);
        assert_eq!(rate, 44_100);
        assert_eq!(unchanged.len(), mono.len());
    }

    /// 综合得分的取舍方向：漏掉真拍（太慢）与拍点落空（太快）都要被压过正确答案
    #[test]
    fn candidate_score_prefers_the_true_tempo() {
        // 正确拍长：拍点踩在能量上，且把所有真拍都覆盖到
        let truth = candidate_score(0.40, 0.90, 0.50, 120.0);
        // 太慢一倍：拍点同样踩在能量上（挑的都是重拍，强度更高），但漏掉一半真拍
        let too_slow = candidate_score(0.55, 0.45, 0.50, 60.0);
        // 太快一倍：拍点覆盖全部能量，但一半拍点落在空处，强度低
        let too_fast = candidate_score(0.18, 0.95, 0.50, 240.0);
        assert!(
            truth > too_slow,
            "漏拍应被扣分: 正确 {truth:.4} vs 太慢 {too_slow:.4}"
        );
        assert!(
            truth > too_fast,
            "落空应被扣分: 正确 {truth:.4} vs 太快 {too_fast:.4}"
        );
    }

    /// 周期性项必须能单独否掉"周期对不上"的假设
    #[test]
    fn normalized_autocorrelation_measures_periodicity() {
        // result[0] 是零延迟（均方值），故归一化后滞后 0 应为 1
        let mut envelope = vec![0.0_f64; 400];
        for i in (0..400).step_by(20) {
            envelope[i] = 1.0;
        }
        let correlations = autocorrelation(&envelope, 80);
        assert!((normalized_autocorrelation(&correlations, 0) - 1.0).abs() < 1e-9);
        // 周期处高、半周期处低
        assert!(normalized_autocorrelation(&correlations, 20) > 0.9);
        assert!(normalized_autocorrelation(&correlations, 10) < 0.5);
        // 全零包络不能除零
        assert!(normalized_autocorrelation(&[0.0, 0.0, 0.0], 1).abs() < f64::EPSILON);
    }

    /// 静音没有拍子
    #[test]
    fn silence_yields_no_estimate() {
        let sample_rate = 44_100u32;
        let mono = vec![0.0_f32; sample_rate as usize * 30];
        assert!(detect(&mono, sample_rate).is_none());
    }

    /// 白噪声没有拍子：不能报出一个"看起来像"的 BPM
    ///
    /// 用线性同余而不是 xorshift：后者的低次位结构经 FFT 后会留下周期纹路，
    /// 那测的就不是"能否拒绝噪声"，而是"伪随机数够不够随机"。
    #[test]
    fn white_noise_yields_no_estimate() {
        let sample_rate = 44_100u32;
        let mut state = 0x2545_F491_u32;
        let mono: Vec<f32> = (0..sample_rate as usize * 60)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 8) as f32 / 16_777_216.0 * 2.0 - 1.0
            })
            .collect();
        assert!(detect(&mono, sample_rate).is_none());
    }

    /// 纯持续音（无瞬态）：频谱通量接近零，测不出拍子
    #[test]
    fn steady_tone_yields_no_estimate() {
        let sample_rate = 44_100u32;
        let mono: Vec<f32> = (0..sample_rate as usize * 30)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (t * 2.0 * std::f32::consts::PI * 440.0).sin() * 0.5
            })
            .collect();
        assert!(detect(&mono, sample_rate).is_none());
    }

    /// 交替显著性：严格强弱交替应接近 1，完全均匀应为 0
    #[test]
    fn alternating_salience_distinguishes_uniform_from_alternating() {
        let alternating: Vec<usize> = (0..16).map(|i| i * 4).collect();
        let mut with_values = vec![0.0_f64; 64];
        for (i, position) in alternating.iter().enumerate() {
            with_values[*position] = if i % 2 == 0 { 1.0 } else { 0.1 };
        }
        let salience = alternating_salience(&with_values, &alternating);
        assert!(salience > 0.7, "严格交替应显著，实际 {salience}");

        // 每个拍点强度相同：没有交替结构
        let mut uniform = vec![0.0_f64; 64];
        for position in &alternating {
            uniform[*position] = 1.0;
        }
        assert!(alternating_salience(&uniform, &alternating) < 0.05);
    }

    /// 富集度：拍点落在能量上应显著高于随机摆点
    #[test]
    fn beat_enrichment_rewards_hitting_onsets() {
        let mut onset = vec![0.05_f64; 200];
        let beats: Vec<usize> = (0..20).map(|i| i * 10).collect();
        for beat in &beats {
            onset[*beat] = 5.0;
        }
        let on_beat = beat_enrichment(&onset, &beats);
        // 同样的拍点数，但都摆在低能量处
        let off: Vec<usize> = (0..20).map(|i| i * 10 + 5).collect();
        let off_beat = beat_enrichment(&onset, &off);
        assert!(
            on_beat > off_beat * 3.0,
            "踩点 {on_beat} 应远高于落空 {off_beat}"
        );
    }

    /// 峰值归一化：包络最大值应为 1，DP 的代价量级才稳定
    #[test]
    fn whitening_normalizes_to_unit_peak() {
        let sample_rate = 44_100u32;
        let mono = synth_beat_track(sample_rate, 120.0, 10);
        let envelope = onset_envelope(&mono, sample_rate);
        let peak = envelope.iter().copied().fold(0.0_f32, f32::max);
        assert!((peak - 1.0).abs() < 1e-5, "峰值应归一化到 1，实际 {peak}");
    }

    /// 自相关在整周期的倍数处出现峰：这是候选枚举的事实依据
    #[test]
    fn autocorrelation_peaks_at_period_multiples() {
        let mut envelope = vec![0.0_f64; 400];
        for i in (0..400).step_by(20) {
            envelope[i] = 1.0;
        }
        let correlations = autocorrelation(&envelope, 80);
        // 滞后 20 与 40 都应是局部高点
        assert!(correlations[20] > correlations[10]);
        assert!(correlations[40] > correlations[30]);
        assert!(correlations[20] > correlations[15]);
    }
}
