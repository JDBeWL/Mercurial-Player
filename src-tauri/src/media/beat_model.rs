//! 神经网络节拍跟踪（Beat This!）：拍点、小节线、BPM
//!
//! 仅用于后台批量与节拍同步，切歌路径用 `bpm` 的单窗 DSP。两个后端输出逐位相同，换后端不必重算缓存。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use beat_this::{BeatAnalysis, BeatThis, RtenRuntime, Runtime, beat_counts, calculate_bpm};
use serde::Serialize;
use symphonia::core::audio::{Channels, GenericAudioBufferRef};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::error::AppError;

/// 模型训练时固定的采样率，喂别的采样率结果会变形
const MODEL_SAMPLE_RATE: u32 = 22_050;

/// 一次完整分析的结果
#[derive(Debug, Clone, Serialize)]
pub struct BeatTrack {
    /// 由拍点间隔中位数换算；变速曲只代表最常出现的间隔，分段看 `beats`
    pub bpm: Option<f64>,
    /// 拍点时间（秒），升序
    pub beats: Vec<f64>,
    /// 小节线时间（秒），是 `beats` 的子集
    pub downbeats: Vec<f64>,
    /// 每小节拍数 -> 出现次数；主导键即拍号（`4` 表示 4/4）
    pub meter: Vec<MeterCount>,
    /// 拍点覆盖的时间范围（秒），判断分析是否覆盖了整曲
    pub span_secs: f64,
}

/// 一种小节长度及其出现次数
#[derive(Debug, Clone, Serialize)]
pub struct MeterCount {
    pub beats_per_bar: u32,
    pub count: u32,
}

/// 可选组件（节拍分析）的安装目录：`<exe>/plugins/beat-analysis/`
///
/// 模型和 GPU 运行时（28 MB）不随安装包分发，由用户手动放入；目录没有 `manifest.json`，插件系统不会加载它。
#[must_use]
pub fn component_dir() -> Option<PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        Some(dir.join("plugins").join("beat-analysis"))
    }
    #[cfg(target_os = "android")]
    {
        let base = crate::android::saf::get_app_data_dir().ok()??;
        Some(PathBuf::from(base).join("plugins").join("beat-analysis"))
    }
}

/// 按 `MERCURIAL_MODEL_DIR` → 组件目录 → exe 旁 `models/`、`resources/models` → 源码树 `resources/models` 顺序找模型
fn model_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MERCURIAL_MODEL_DIR") {
        let path = PathBuf::from(dir);
        if path.join("beat_this_small.onnx").exists() {
            return Some(path);
        }
    }
    if let Some(dir) = component_dir() {
        let candidate = dir.join("models");
        if candidate.join("beat_this_small.onnx").exists() {
            return Some(candidate);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("models"), dir.join("resources").join("models")] {
                if candidate.join("beat_this_small.onnx").exists() {
                    return Some(candidate);
                }
            }
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models");
    if dev.join("beat_this_small.onnx").exists() {
        return Some(dev);
    }
    None
}

/// 本模块是否可用（模型文件存在）
#[must_use]
pub fn is_available() -> bool {
    model_dir().is_some()
}

/// 可选组件的安装状态，给插件页显示"装了什么、还缺什么"。
#[derive(Debug, Clone, Serialize)]
pub struct ComponentStatus {
    /// 组件目录绝对路径（可能尚未创建）
    pub dir: String,
    /// 功能是否可用；含开发态源码树回退，不等于用户装没装（后者看 `missing`）
    pub ready: bool,
    /// GPU 运行时就位，推理会走显卡
    pub gpu: bool,
    /// 还需补的文件；功能可用时只列运行时，否则与 `ready` 自相矛盾
    pub missing: Vec<String>,
}

/// 组件需要的文件，`true` 表示必需（缺了整个功能不可用）
const COMPONENT_FILES: &[(&str, bool)] = &[
    ("models/beat_this_small.onnx", true),
    ("models/mel_spectrogram.onnx", true),
    ("runtime/onnxruntime.dll", false),
    // ort 的 load-dynamic 会连带加载它；漏算会让界面显示 GPU 而实际静默退回 CPU
    ("runtime/onnxruntime_providers_shared.dll", false),
];

/// `ready` 走 `is_available`，含开发态源码树回退：dev 时组件目录为空但功能可用。
#[must_use]
pub fn component_status() -> ComponentStatus {
    let dir = component_dir();
    let base = dir.clone().unwrap_or_default();
    let ready = is_available();
    let missing = COMPONENT_FILES
        .iter()
        .filter(|(name, _)| !base.join(name).exists())
        .filter(|(name, _)| !ready || !name.starts_with("models/"))
        .map(|(name, _)| (*name).to_string())
        .collect::<Vec<_>>();
    // 运行时这一组要么齐、要么不用：只缺一半时 DirectML 会建不起来
    let gpu = !missing.iter().any(|m| m.starts_with("runtime/"));
    ComponentStatus {
        dir: base.to_string_lossy().to_string(),
        ready,
        gpu,
        missing,
    }
}

/// 当前实际后端名；尚未分析过时返回 `backend_hint` 的探测值，前端拿到未知值会先显示 CPU。
pub fn backend_label() -> &'static str {
    TRACKER
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|guard| guard.as_ref().map(Tracker::label))
        .unwrap_or_else(backend_hint)
}

/// 预估本机会用哪个后端，不加载模型：设置页据此估耗时，不能为此占显存。
pub fn backend_hint() -> &'static str {
    #[cfg(windows)]
    if super::ort_model::runtime_present() {
        return "directml";
    }
    "rten"
}

/// rten 的模型类型没有从 crate 导出，只能通过关联类型指代
type RtenTracker = BeatThis<<RtenRuntime as Runtime>::Model>;

/// 可复用推理上下文；必须复用——ORT 建 session 约 0.9 s（解析 ONNX + 初始化 DirectML），比短曲推理还贵。
enum Tracker {
    #[cfg(windows)]
    Ort {
        inner: BeatThis<super::ort_model::OrtModel>,
        device: super::ort_model::Device,
    },
    Rten(RtenTracker),
}

impl Tracker {
    fn label(&self) -> &'static str {
        match self {
            #[cfg(windows)]
            Self::Ort { device, .. } => device.label(),
            Self::Rten(_) => "rten-cpu",
        }
    }

    fn analyze(&mut self, pcm: &[f32]) -> Result<BeatAnalysis, AppError> {
        let result = match self {
            #[cfg(windows)]
            Self::Ort { inner, .. } => inner.analyze_audio(pcm, MODEL_SAMPLE_RATE),
            Self::Rten(inner) => inner.analyze_audio(pcm, MODEL_SAMPLE_RATE),
        };
        result.map_err(|e| AppError::Audio(format!("节拍推理失败: {e}")))
    }
}

/// 全局唯一推理上下文；按曲目加锁，避免两个模型实例同时抢显存。
static TRACKER: OnceLock<Mutex<Option<Tracker>>> = OnceLock::new();

fn tracker_slot() -> &'static Mutex<Option<Tracker>> {
    TRACKER.get_or_init(|| Mutex::new(None))
}

fn build_tracker(dir: &Path) -> Option<Tracker> {
    #[cfg(windows)]
    {
        if super::ort_model::prepare_runtime().is_some() {
            match super::ort_model::load_pair(
                &dir.join("mel_spectrogram.onnx"),
                &dir.join("beat_this_small.onnx"),
            ) {
                Ok((mel, beat, device)) => {
                    log::info!("节拍分析后端: {}", device.label());
                    return Some(Tracker::Ort {
                        inner: BeatThis::from_models(mel, beat),
                        device,
                    });
                }
                Err(e) => log::warn!("ORT 后端不可用，退回 rten: {e}"),
            }
        }
    }
    let runtime = RtenRuntime;
    let built = BeatThis::new(
        &runtime,
        &dir.join("mel_spectrogram.onnx"),
        &dir.join("beat_this_small.onnx"),
    );
    match built {
        Ok(inner) => {
            log::info!("节拍分析后端: rten-cpu");
            Some(Tracker::Rten(inner))
        }
        Err(e) => {
            log::error!("节拍模型加载失败: {e}");
            None
        }
    }
}

/// 整曲节拍跟踪；失败时调用方应降级到 `bpm` 的单窗口算法。
pub fn analyze(path: &str) -> Result<BeatTrack, AppError> {
    let Some(dir) = model_dir() else {
        return Err(AppError::Audio("节拍模型缺失".to_string()));
    };
    let (mono, sample_rate) = decode_mono(path)?;
    let pcm = resample_to(&mono, sample_rate, MODEL_SAMPLE_RATE);
    if pcm.len() < MODEL_SAMPLE_RATE as usize {
        return Err(AppError::Audio("音频过短，无法做节拍跟踪".to_string()));
    }

    let mut guard = tracker_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.is_none() {
        *guard = build_tracker(&dir);
    }
    let tracker = guard
        .as_mut()
        .ok_or_else(|| AppError::Audio("节拍模型加载失败".to_string()))?;
    let analysis = tracker.analyze(&pcm)?;

    let bpm = calculate_bpm(&analysis).map(f64::from);
    let beats: Vec<f64> = analysis.beats.iter().map(|&t| f64::from(t)).collect();
    let downbeats: Vec<f64> = analysis.downbeats.iter().map(|&t| f64::from(t)).collect();
    let meter = meter_histogram(&beat_counts(&analysis));
    let span_secs = match (beats.first(), beats.last()) {
        (Some(first), Some(last)) => last - first,
        _ => 0.0,
    };

    log::debug!(
        "节拍跟踪 {path}: {} 拍 / {} 小节线, BPM {:?}, 主拍号 {:?}",
        beats.len(),
        downbeats.len(),
        bpm,
        meter.first().map(|m| m.beats_per_bar)
    );

    Ok(BeatTrack {
        bpm,
        beats,
        downbeats,
        meter,
        span_secs,
    })
}

/// 由"每拍在小节中的位置"（1 = 强拍）统计小节长度分布。
/// 主导值即拍号分子（`4` 表示 4/4）；变拍曲会有多个值并存，不能压成一个数。
fn meter_histogram(counts: &[i32]) -> Vec<MeterCount> {
    let mut histogram: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    let mut last = 0_i32;
    for &count in counts {
        if count == 1 && last != 1 && last > 0 {
            *histogram.entry(last.unsigned_abs()).or_insert(0) += 1;
        }
        last = count;
    }
    let mut rows: Vec<MeterCount> = histogram
        .into_iter()
        .map(|(beats_per_bar, count)| MeterCount {
            beats_per_bar,
            count,
        })
        .collect();
    // 最多者排前，调用方读第一条即主导拍号
    rows.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.beats_per_bar.cmp(&b.beats_per_bar))
    });
    rows
}

/// 整曲解码为单声道 `f32`；不做窗口截取，否则变速曲后面的速度段会被漏掉。
fn decode_mono(path: &str) -> Result<(Vec<f32>, u32), AppError> {
    let file = crate::android::saf::open_media_file(path)?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = Path::new(path).extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| AppError::Audio(format!("节拍跟踪探测格式失败: {e}")))?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| AppError::Audio("节拍跟踪: 没有音频轨".to_string()))?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(CodecParameters::audio)
        .cloned()
        .ok_or_else(|| AppError::Audio("节拍跟踪: 缺少解码参数".to_string()))?;
    let sample_rate = params.sample_rate.unwrap_or(44_100).max(1);
    let channels = params.channels.as_ref().map_or(2, Channels::count).max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| AppError::Audio(format!("节拍跟踪创建解码器失败: {e}")))?;

    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => {
                log::debug!("节拍跟踪读包中断 {path}: {e}");
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => {
                log::debug!("节拍跟踪解码中断 {path}: {e}");
                break;
            }
        };
        let mut block: Vec<f32> = Vec::new();
        append_mono(audio, &mut block, channels);
        mono.extend_from_slice(&block);
    }
    if mono.is_empty() {
        return Err(AppError::Audio("节拍跟踪: 未能解码出采样".to_string()));
    }
    Ok((mono, sample_rate))
}

/// 重采样到 22 050 Hz；48 kHz → 22.05 kHz 是 2.177×，必须多相/FEC，不能用 `bpm` 的整数倍抽取。
/// 输出缓冲每步按 `output_frames_next()` 扩：`Fft` 单步输出帧数不是常数，按固定尺寸会报
/// `Insufficient output buffer size` 并提前结束，整首被判"音频过短"。
fn resample_to(mono: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to {
        return mono.to_vec();
    }
    use audioadapter_buffers::direct::SequentialSliceOfVecs;
    use rubato::{Fft, FixedSync, Indexing, Resampler};
    const CHUNK: usize = 1024;
    let Ok(mut resampler) = Fft::<f32>::new(from as usize, to as usize, CHUNK, 1, FixedSync::Input)
    else {
        log::error!("节拍跟踪重采样器创建失败: {from} -> {to}");
        return mono.to_vec();
    };
    let mut output: Vec<Vec<f32>> = vec![vec![0.0_f32; resampler.output_frames_next()]];
    let mut out: Vec<f32> = Vec::with_capacity(mono.len() * to as usize / from as usize + CHUNK);
    let mut position = 0;
    while position < mono.len() {
        let end = (position + CHUNK).min(mono.len());
        let mut input: Vec<Vec<f32>> = vec![mono[position..end].to_vec()];
        // 最后一块补齐到整 chunk：FixedSync::Input 要求每次输入帧数恒为 chunk_size
        input[0].resize(CHUNK, 0.0);
        let need = resampler.output_frames_next();
        if output[0].len() < need {
            output[0].resize(need, 0.0);
        }
        let (Ok(input_adapter), Ok(mut output_adapter)) = (
            SequentialSliceOfVecs::new(&input, 1, CHUNK),
            SequentialSliceOfVecs::new_mut(&mut output, 1, need),
        ) else {
            break;
        };
        match resampler.process_into_buffer(
            &input_adapter,
            &mut output_adapter,
            Some(&Indexing::new()),
        ) {
            Ok((_, written)) => out.extend_from_slice(&output[0][..written]),
            Err(e) => {
                log::error!("节拍跟踪重采样中断: {e:?}");
                break;
            }
        }
        position = end;
    }
    // 尾部补齐会多出输出，按目标时长截断
    let want = (mono.len() as f64 * f64::from(to) / f64::from(from)).round() as usize;
    out.truncate(want);
    out
}

/// 解码缓冲区转单声道 `f32`；多声道取平均，避免只出现在一侧的乐器被丢掉
fn append_mono(decoded: GenericAudioBufferRef<'_>, out: &mut Vec<f32>, channels: usize) {
    decoded.copy_to_vec_interleaved(out);
    if channels > 1 {
        let frame_count = out.len() / channels;
        let mut mono = Vec::with_capacity(frame_count);
        for frame in out.chunks(channels) {
            let sum: f32 = frame.iter().sum();
            mono.push(sum / channels as f32);
        }
        out.clear();
        out.extend_from_slice(&mono);
    }
}

#[cfg(test)]
mod tests {
    use super::{component_status, is_available, meter_histogram, resample_to};

    /// 开发态组件目录为空但 `is_available()` 回退源码树，插件页可用性必须与之一致。
    #[test]
    fn component_status_agrees_with_reality() {
        let status = component_status();
        assert!(
            status.dir.contains("beat-analysis"),
            "组件目录应指向 plugins/beat-analysis，实际是 {}",
            status.dir
        );
        assert_eq!(
            status.ready,
            is_available(),
            "插件页显示的可用性必须等于真实可用性"
        );
        for file in &status.missing {
            assert!(
                file.starts_with("models/") || file.starts_with("runtime/"),
                "缺失清单应是相对组件目录的路径，实际是 {file}"
            );
        }
        if status.ready {
            assert!(
                !status.missing.iter().any(|f| f.starts_with("models/")),
                "功能已可用时，缺失清单只应包含运行时，实际是 {:?}",
                status.missing
            );
        }
        let runtime_missing = status.missing.iter().any(|f| f.starts_with("runtime/"));
        assert_eq!(status.gpu, !runtime_missing, "gpu 标志与缺失清单不一致");
    }

    /// 重采样长度必须与输入时长成正比；任一步提前结束会让长度差两个数量级。
    #[test]
    fn resample_keeps_duration_for_every_source_rate() {
        let seconds = 2.0_f64;
        for from in [44_100_u32, 48_000, 96_000, 192_000] {
            let total = (f64::from(from) * seconds) as usize;
            // 用非零、非常数信号：全零可能掩盖问题
            let mono: Vec<f32> = (0..total).map(|i| ((i % 97) as f32) / 97.0).collect();
            let out = resample_to(&mono, from, 22_050);
            let expected = seconds * 22_050.0;
            let error = (out.len() as f64 - expected).abs() / expected;
            assert!(
                error < 0.02,
                "{from} Hz 重采样后 {} 帧，期望约 {expected:.0}（偏差 {:.1}%）",
                out.len(),
                error * 100.0
            );
        }
    }

    #[test]
    fn meter_histogram_reads_bar_length_from_downbeats() {
        // 两小节 4/4 + 一小节 3/4：强拍位置 4/4/3 的循环
        let counts = [1, 2, 3, 4, 1, 2, 3, 4, 1, 2, 3, 1];
        let rows = meter_histogram(&counts);
        assert_eq!(rows[0].beats_per_bar, 4, "主导拍号应为 4");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[1].beats_per_bar, 3);
    }

    #[test]
    fn meter_histogram_handles_pickup_and_empty() {
        assert!(meter_histogram(&[]).is_empty());
        // 从弱拍起的小节（pickup）不该被当成一个完整小节
        let counts = [3, 4, 1, 2, 3, 4, 1];
        let rows = meter_histogram(&counts);
        assert_eq!(rows[0].beats_per_bar, 4);
    }
}
