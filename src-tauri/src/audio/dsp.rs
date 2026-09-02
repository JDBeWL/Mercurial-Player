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
/// 使用ITU-R BS.775-1标准的下混系数。
///
/// 共享模式(decoder::convert_audio_buffer)与独占模式(decode_push::convert_channels_into)
/// 共用此实现,避免两套系数漂移。非 6/8 声道的其它多声道布局退化为
/// FL/FR × 0.8 输出。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_soft_clip_fast_passthrough_below_threshold() {
        // |x| <= 0.95 直通(快速路径,位精确)
        for &x in &[0.0_f32, 0.1, 0.5, 0.9, 0.95, -0.5, -0.9, -0.95] {
            let y = soft_clip_fast(x);
            assert!(
                approx_eq(y, x),
                "soft_clip_fast({x}) should passthrough, got {y}"
            );
            assert_eq!(y.to_bits(), x.to_bits(), "passthrough must be bit-exact");
        }
    }

    #[test]
    fn test_soft_clip_fast_compresses_above_threshold() {
        // |x| > 0.95 压缩:输出幅度减小、符号保持、永不越过 1.0(f32 下大输入饱和到 1.0)
        for &x in &[
            0.951_f32, 1.0, 1.5, 2.0, 10.0, -0.951, -1.0, -1.5, -2.0, -10.0,
        ] {
            let y = soft_clip_fast(x);
            assert!(
                y.abs() < x.abs(),
                "soft_clip_fast should reduce magnitude: |{x}| -> |{y}|"
            );
            assert!(
                y.abs() <= 1.0,
                "soft_clip_fast output |{y}| must stay <= 1.0"
            );
            assert!(
                x * y >= 0.0,
                "soft_clip_fast should preserve sign: x={x}, y={y}"
            );
        }
    }

    #[test]
    fn test_soft_clip_fast_ceiling_monotonic() {
        // 查表路径单调逼近 1.0(f32 下 tanh 在极大输入处饱和为 1.0)
        let mut prev = soft_clip_fast(0.95);
        for &x in &[0.96_f32, 0.98, 1.0, 1.2, 1.5, 2.0, 100.0] {
            let y = soft_clip_fast(x);
            assert!(
                y >= prev - 1e-6,
                "output should be non-decreasing: {prev} -> {y}"
            );
            assert!(y <= 1.0, "ceiling should stay <= 1.0, got {y}");
            prev = y;
        }
        assert!(
            (0.999..=1.0).contains(&prev),
            "extreme input should saturate near 1.0 ceiling, got {prev}"
        );
    }

    fn downmix_frame(src_ch: usize, frame: &[f32]) -> (f32, f32) {
        downmix_surround_to_stereo(frame, src_ch, 0)
    }

    #[test]
    fn test_downmix_5_1_itut_coefficients() {
        // FL FR FC LFE SL SR (LFE 不混入)
        let frame = [0.5_f32, -0.4, 0.2, 0.9, 0.1, 0.2];
        let (l, r) = downmix_frame(6, &frame);
        let expected_l = (0.5 + 0.2 * 0.707 + 0.1 * 0.707) * 0.707;
        let expected_r = (-0.4 + 0.2 * 0.707 + 0.2 * 0.707) * 0.707;
        assert!(
            approx_eq(l, expected_l),
            "5.1 L = {l}, expected {expected_l}"
        );
        assert!(
            approx_eq(r, expected_r),
            "5.1 R = {r}, expected {expected_r}"
        );
    }

    #[test]
    fn test_downmix_7_1_itut_coefficients() {
        // FL FR FC LFE BL BR SL SR (LFE 不混入)
        let frame = [0.5_f32, -0.4, 0.2, 0.9, 0.1, -0.1, 0.3, 0.2];
        let (l, r) = downmix_frame(8, &frame);
        let expected_l = (0.5 + 0.2 * 0.707 + 0.3 * 0.707 + 0.1 * 0.5) * 0.667;
        let expected_r = (-0.4 + 0.2 * 0.707 + 0.2 * 0.707 + (-0.1) * 0.5) * 0.667;
        assert!(
            approx_eq(l, expected_l),
            "7.1 L = {l}, expected {expected_l}"
        );
        assert!(
            approx_eq(r, expected_r),
            "7.1 R = {r}, expected {expected_r}"
        );
    }

    #[cfg(windows)] // convert_channels_into 仅 Windows 独占模式使用
    #[test]
    fn test_convert_channels_into_6_to_2_matches_downmix() {
        // (6|8,2) 分支必须与 downmix_surround_to_stereo 逐帧一致
        let src: u16 = 6;
        let mut input = Vec::with_capacity(6 * 4);
        for f in 0..4 {
            let base = f as f32 * 0.1;
            input.extend_from_slice(&[0.5 + base, -0.4 - base, 0.2, 0.9, 0.1 + base, 0.2 - base]);
        }
        let mut out = Vec::new();
        convert_channels_into(&input, src, 2, &mut out);
        assert_eq!(out.len(), 4 * 2);
        for f in 0..4 {
            let (l, r) = downmix_surround_to_stereo(&input, src as usize, f);
            assert!(approx_eq(out[f * 2], l), "frame {f} L mismatch");
            assert!(approx_eq(out[f * 2 + 1], r), "frame {f} R mismatch");
        }
    }
}
