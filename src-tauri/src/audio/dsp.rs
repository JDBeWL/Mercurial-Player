//! 音频 DSP 辅助
//!
//! 软削波与 preamp 预计算查找表、Hann 窗、多通道转换。
//! 共享模式(VisualizationSource)与独占模式(解码推送线程)共用。

const SOFT_CLIP_TABLE_SIZE: usize = 2001;

/// 预计算的软削波查找表
static SOFT_CLIP_TABLE: std::sync::LazyLock<[f32; SOFT_CLIP_TABLE_SIZE]> =
    std::sync::LazyLock::new(|| {
        let mut table = [0.0f32; SOFT_CLIP_TABLE_SIZE];
        for (i, item) in table.iter_mut().enumerate() {
            let x = i as f32 / 1000.0; // 0.0到2.0
            *item = compute_soft_clip(x);
        }
        table
    });

/// 计算软削波值（用于生成查找表）
#[inline]
fn compute_soft_clip(x: f32) -> f32 {
    let threshold = 0.95;
    if x <= threshold {
        x
    } else {
        let over = x - threshold;
        threshold + (1.0 - threshold) * (over / (1.0 - threshold) * 0.5).tanh()
    }
}

/// 快速软削波 - 使用查找表
#[inline(always)]
pub(crate) fn soft_clip_fast(x: f32) -> f32 {
    let sign = x.signum();
    let abs_x = x.abs();

    // 快速路径：大多数采样在[-0.95, 0.95]范围内
    if abs_x <= 0.95 {
        return x;
    }

    // 查表路径
    let index = ((abs_x * 1000.0) as usize).min(SOFT_CLIP_TABLE_SIZE - 1);
    sign * SOFT_CLIP_TABLE[index]
}

/// Preamp增益查找表（-8dB到+8dB，精度0.1dB）
const PREAMP_TABLE_SIZE: usize = 161;

static PREAMP_TABLE: std::sync::LazyLock<[f32; PREAMP_TABLE_SIZE]> =
    std::sync::LazyLock::new(|| {
        let mut table = [0.0f32; PREAMP_TABLE_SIZE];
        for (i, item) in table.iter_mut().enumerate() {
            let db = (i as f32 - 80.0) / 10.0; // -8.0 到 +8.0 dB
            *item = 10.0_f32.powf(db / 20.0);
        }
        table
    });

/// 快速dB到线性增益转换
#[inline(always)]
pub(crate) fn db_to_linear_fast(db: f32) -> f32 {
    let clamped = db.clamp(-8.0, 8.0);
    let index = ((clamped + 8.0) * 10.0) as usize;
    PREAMP_TABLE[index.min(PREAMP_TABLE_SIZE - 1)]
}

/// 预计算 Hann 窗口(避免每次 FFT 都堆分配)
/// 公式: hann[i] = 0.5 - 0.5 * cos(2π * i / N)
pub(crate) fn precompute_hann_window(size: usize) -> Vec<f32> {
    let n = size as f32;
    (0..size)
        .map(|i| {
            let angle = 2.0 * std::f32::consts::PI * (i as f32) / n;
            0.5 - 0.5 * angle.cos()
        })
        .collect()
}

/// 5.1/7.1环绕声到立体声的专业混音
/// 使用ITU-R BS.775-1标准的下混系数
#[cfg(windows)]
pub(crate) fn downmix_surround_to_stereo(
    samples: &[f32],
    src_ch: usize,
    frame: usize,
) -> (f32, f32) {
    let start = frame * src_ch;

    let fl = samples[start];
    let fr = samples[start + 1];
    let fc = if src_ch > 2 { samples[start + 2] } else { 0.0 };
    let _lfe = if src_ch > 3 { samples[start + 3] } else { 0.0 };

    const CENTER_MIX: f32 = 0.707;
    const SURROUND_MIX: f32 = 0.707;
    const BACK_MIX: f32 = 0.5;

    let (mut left, mut right) = (fl, fr);

    left += fc * CENTER_MIX;
    right += fc * CENTER_MIX;

    match src_ch {
        6 => {
            let sl = samples[start + 4];
            let sr = samples[start + 5];
            left += sl * SURROUND_MIX;
            right += sr * SURROUND_MIX;
        }
        8 => {
            let bl = samples[start + 4];
            let br = samples[start + 5];
            let sl = samples[start + 6];
            let sr = samples[start + 7];
            left += sl * SURROUND_MIX + bl * BACK_MIX;
            right += sr * SURROUND_MIX + br * BACK_MIX;
        }
        _ => {}
    }

    let normalize = match src_ch {
        6 => 0.707,
        8 => 0.667,
        _ => 0.8,
    };

    (
        (left * normalize).clamp(-1.0, 1.0),
        (right * normalize).clamp(-1.0, 1.0),
    )
}

/// 通道转换(in-place 版本):写入预分配缓冲区,避免每帧堆分配。
/// out 会被 clear 并填充结果。
#[cfg(windows)]
pub(crate) fn convert_channels_into(
    samples: &[f32],
    src_ch: u16,
    target_ch: u16,
    out: &mut Vec<f32>,
) {
    out.clear();
    if src_ch == target_ch {
        out.extend_from_slice(samples);
        return;
    }
    let (src, tgt) = (src_ch as usize, target_ch as usize);
    let frames = samples.len() / src;
    out.reserve(frames * tgt);

    for f in 0..frames {
        let start = f * src;
        match (src, tgt) {
            (1, 2) => {
                let s = samples[start];
                out.push(s);
                out.push(s);
            }
            (2, 1) => {
                out.push(f32::midpoint(samples[start], samples[start + 1]));
            }
            (6 | 8, 2) => {
                // 5.1/7.1到立体声的专业混音
                let (left, right) = downmix_surround_to_stereo(samples, src, f);
                out.push(left);
                out.push(right);
            }
            _ => {
                // 其他情况:简单截取或填充
                for ch in 0..tgt {
                    out.push(if ch < src {
                        samples[start + ch]
                    } else {
                        samples[start]
                    });
                }
            }
        }
    }
}
