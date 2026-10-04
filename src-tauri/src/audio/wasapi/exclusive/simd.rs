//! SIMD 加速的样本转换：AVX2/FMA + SSE2 intrinsics 做 f32 到 i16/i32 的字节转换。
//!
//! intrinsics 必须 unsafe，故在模块级统一 allow；该 allow 仅限本模块，不影响其它代码的 unsafe_code 审查。

#![allow(unsafe_code)]

/// 将 f32 采样转换为指定格式的字节,写入复用 buffer(零分配)
pub fn convert_samples_to_bytes_into(
    samples: &[f32],
    bits: u16,
    is_float: bool,
    out: &mut Vec<u8>,
) {
    let bytes_per_sample = match bits {
        16 => 2,
        24 => 3,
        32 => 4,
        _ => 4,
    };
    out.clear();
    out.reserve(samples.len() * bytes_per_sample);

    match (bits, is_float) {
        (32, true) => {
            // SAFETY: f32 的内存表示就是小端 4 字节,与 [u8; 4] 同 size 且都是 POD,可整块 memcpy
            let bytes = unsafe {
                core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 4)
            };
            out.extend_from_slice(bytes);
        }
        (32, false) => {
            #[cfg(target_arch = "x86_64")]
            {
                if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                    unsafe { f32_to_i32_bytes_avx2(samples, out) };
                    return;
                }
                // SSE2 是 x86_64 baseline,所有 64 位 Intel/AMD CPU 必然支持,无需运行时检测
                unsafe { f32_to_i32_bytes_sse2(samples, out) };
                return;
            }
            #[allow(unreachable_code)]
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
                out.extend_from_slice(&int_val.to_le_bytes());
            }
        }
        (24, _) => {
            // 24-bit: 字节打包(取 i32 低 3 字节)难以 SIMD 化,只能标量
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                let bytes = int_val.to_le_bytes();
                out.extend_from_slice(&bytes[0..3]);
            }
        }
        (16, _) => {
            #[cfg(target_arch = "x86_64")]
            {
                if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                    unsafe { f32_to_i16_bytes_avx2(samples, out) };
                    return;
                }
                unsafe { f32_to_i16_bytes_sse2(samples, out) };
                return;
            }
            #[allow(unreachable_code)]
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                out.extend_from_slice(&int_val.to_le_bytes());
            }
        }
        _ => {
            // 兜底:按 32-bit float 处理
            // SAFETY: 同 (32, true) 分支，f32 按字节直读
            let bytes = unsafe {
                core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 4)
            };
            out.extend_from_slice(bytes);
        }
    }
}

// 分发：AVX2+FMA -> SSE2 -> 标量(非 x86_64)，由 convert_samples_to_bytes_into 运行时选择。
// 两条 SIMD 路径都按 MXCSR 默认的 RNE 舍入，Rust 的 `as i32/i16` 是 toward-zero，.5 边界差 1 LSB。

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)] // 函数整体由 target_feature 限定为 unsafe,内联 unsafe 块冗余
#[allow(clippy::wildcard_imports)] // SIMD intrinsics 数量多,逐个导入冗长
pub(super) unsafe fn f32_to_i16_bytes_avx2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm256_set1_ps(1.0);
    let neg_one = _mm256_set1_ps(-1.0);
    let scale = _mm256_set1_ps(i16::MAX as f32); // 32767.0

    let mut i = 0;
    let chunk = 16; // 16 个 f32 -> 32 bytes (i16)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        let b = _mm256_loadu_ps(samples.as_ptr().add(i + 8));

        let a = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, a)), scale);
        let b = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, b)), scale);

        let a_i32 = _mm256_cvtps_epi32(a);
        let b_i32 = _mm256_cvtps_epi32(b);

        // packs_epi32 的输出有 256-bit lane 交错: [a0..3, b0..3, a4..7, b4..7]
        // permute 0xD8 (= 0b11_01_10_00) 重排回 [a0..7, b0..7];SSE2 单 lane 无此问题
        let packed = _mm256_packs_epi32(a_i32, b_i32);
        let packed = _mm256_permute4x64_epi64(packed, 0xD8);

        // __m256i -> [u8; 32]: 同 size(32B) POD 转换
        let bytes: [u8; 32] = core::mem::transmute(packed);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i32_bytes_avx2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm256_set1_ps(1.0);
    let neg_one = _mm256_set1_ps(-1.0);
    let scale = _mm256_set1_ps(i32::MAX as f32); // 2147483648.0 (f32 精度损失)
    // cvtps_epi32 超范围输入返回 saturation indefinite(0x80000000)，故上限取小于 2^31 的最大 f32
    let max_safe = _mm256_set1_ps(2_147_483_520.0);
    let min_safe = _mm256_set1_ps(-2_147_483_648.0);

    let mut i = 0;
    let chunk = 8; // 8 个 f32 -> 32 bytes (i32)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        let a = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, a)), scale);
        let a = _mm256_max_ps(min_safe, _mm256_min_ps(max_safe, a));
        let a_i32 = _mm256_cvtps_epi32(a);

        let bytes: [u8; 32] = core::mem::transmute(a_i32);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

// SSE2 兜底路径:一次处理 4/8 个 f32

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i16_bytes_sse2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm_set1_ps(1.0);
    let neg_one = _mm_set1_ps(-1.0);
    let scale = _mm_set1_ps(i16::MAX as f32);

    let mut i = 0;
    let chunk = 8; // 8 个 f32 -> 16 bytes (i16)

    while i + chunk <= samples.len() {
        let a = _mm_loadu_ps(samples.as_ptr().add(i));
        let b = _mm_loadu_ps(samples.as_ptr().add(i + 4));

        let a = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, a)), scale);
        let b = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, b)), scale);

        let a_i32 = _mm_cvtps_epi32(a);
        let b_i32 = _mm_cvtps_epi32(b);

        // SSE2 packs_epi32 输出顺序: [a0..3, b0..3] - 单 lane 无交错
        let packed = _mm_packs_epi32(a_i32, b_i32);
        let bytes: [u8; 16] = core::mem::transmute(packed);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[allow(unsafe_op_in_unsafe_fn)]
#[allow(clippy::wildcard_imports)]
pub(super) unsafe fn f32_to_i32_bytes_sse2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm_set1_ps(1.0);
    let neg_one = _mm_set1_ps(-1.0);
    let scale = _mm_set1_ps(i32::MAX as f32);
    let max_safe = _mm_set1_ps(2_147_483_520.0);
    let min_safe = _mm_set1_ps(-2_147_483_648.0);

    let mut i = 0;
    let chunk = 4; // 4 个 f32 -> 16 bytes (i32)

    while i + chunk <= samples.len() {
        let a = _mm_loadu_ps(samples.as_ptr().add(i));
        let a = _mm_mul_ps(_mm_max_ps(neg_one, _mm_min_ps(one, a)), scale);
        let a = _mm_max_ps(min_safe, _mm_min_ps(max_safe, a));
        let a_i32 = _mm_cvtps_epi32(a);

        let bytes: [u8; 16] = core::mem::transmute(a_i32);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    while i < samples.len() {
        let int_val = (samples[i].clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
        out.extend_from_slice(&int_val.to_le_bytes());
        i += 1;
    }
}

#[cfg(test)]
#[allow(unsafe_code)] // 测试 SIMD intrinsics 需要 unsafe
mod simd_tests {
    use super::*;

    /// 样本长度对齐到 16（SSE2 chunk=8 与 AVX2 chunk=16 的 LCM），让两条 SIMD 路径都不走尾部标量循环
    fn make_test_samples(n: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / n as f32;
            // 覆盖 [-1, 1] 全范围,含边界
            samples.push((t * 2.0 - 1.0).clamp(-1.0, 1.0));
        }
        // 特殊值: 0、极值、超过 1.0 的值(测试 clamp)
        samples.extend([
            0.0_f32, 1.0, -1.0, 1.5, -1.5, // clamp 边界测试
            0.5, -0.5, 0.25, -0.25, 0.125, -0.125, // .5 边界舍入测试
            0.0, 0.0, 0.0, 0.0, 0.0, // 填充对齐
        ]);
        debug_assert!(samples.len() % 16 == 0, "测试样本长度需对齐到 16");
        samples
    }

    #[test]
    fn test_i16_simd_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut simd_out = Vec::new();

        // 强制走标量路径:直接调用核心逻辑
        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        convert_samples_to_bytes_into(&samples, 16, false, &mut simd_out);

        assert_eq!(scalar_out.len(), simd_out.len(), "输出长度不一致");

        let scalar_i16: Vec<i16> = scalar_out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| i16::from_le_bytes(c))
            .collect();
        let simd_i16: Vec<i16> = simd_out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| i16::from_le_bytes(c))
            .collect();

        for (i, (s, v)) in scalar_i16.iter().zip(simd_i16.iter()).enumerate() {
            let diff = s.abs_diff(*v);
            // 容差 1 LSB：舍入模式差异见模块顶部分发说明
            assert!(
                diff <= 1,
                "i16 差异过大 at {i}: scalar={s}, simd={v}, diff={diff}"
            );
        }
    }

    #[test]
    fn test_i32_simd_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut simd_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        convert_samples_to_bytes_into(&samples, 32, false, &mut simd_out);

        // i32 路径: SIMD 夹到 2147483520.0，输入 1.0 时比标量的 i32::MAX 小 127
        assert_eq!(scalar_out.len(), simd_out.len(), "输出长度不一致");

        let scalar_i32: Vec<i32> = scalar_out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| i32::from_le_bytes(c))
            .collect();
        let simd_i32: Vec<i32> = simd_out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| i32::from_le_bytes(c))
            .collect();

        for (i, (s, v)) in scalar_i32.iter().zip(simd_i32.iter()).enumerate() {
            let diff = (s.abs_diff(*v)) as i64;
            // 容差即上面 clamp 上限与 i32::MAX 之差
            assert!(
                diff <= 128,
                "i32 差异过大 at {i}: scalar={s}, simd={v}, diff={diff}"
            );
        }
    }

    #[test]
    fn test_f32_bytes_passthrough() {
        let samples = make_test_samples(32);
        let mut out = Vec::new();
        convert_samples_to_bytes_into(&samples, 32, true, &mut out);

        assert_eq!(out.len(), samples.len() * 4);
        let as_f32: Vec<f32> = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_le_bytes(c))
            .collect();
        assert_eq!(as_f32, samples);
    }

    #[test]
    fn test_24bit_scalar_path() {
        let samples = make_test_samples(32);
        let mut out = Vec::new();
        convert_samples_to_bytes_into(&samples, 24, false, &mut out);
        assert_eq!(out.len(), samples.len() * 3);
    }

    /// 直接调用 SSE2 兜底路径（绕过运行时检测），确保老 CPU 上的输出正确
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_i16_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut sse2_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        unsafe { f32_to_i16_bytes_sse2(&samples, &mut sse2_out) };

        assert_eq!(scalar_out.len(), sse2_out.len(), "SSE2 i16 长度不一致");

        let scalar_i16: Vec<i16> = scalar_out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| i16::from_le_bytes(c))
            .collect();
        let sse2_i16: Vec<i16> = sse2_out
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| i16::from_le_bytes(c))
            .collect();

        for (i, (s, v)) in scalar_i16.iter().zip(sse2_i16.iter()).enumerate() {
            let diff = s.abs_diff(*v);
            // 容差 1 LSB：舍入模式差异见模块顶部分发说明
            assert!(
                diff <= 1,
                "SSE2 i16 差异过大 at {i}: scalar={s}, sse2={v}, diff={diff}"
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_i32_matches_scalar() {
        let samples = make_test_samples(64);
        let mut scalar_out = Vec::new();
        let mut sse2_out = Vec::new();

        for &s in &samples {
            let int_val = (s.clamp(-1.0, 1.0) * i32::MAX as f32) as i32;
            scalar_out.extend_from_slice(&int_val.to_le_bytes());
        }

        unsafe { f32_to_i32_bytes_sse2(&samples, &mut sse2_out) };

        assert_eq!(scalar_out.len(), sse2_out.len(), "SSE2 i32 长度不一致");

        let scalar_i32: Vec<i32> = scalar_out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| i32::from_le_bytes(c))
            .collect();
        let sse2_i32: Vec<i32> = sse2_out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| i32::from_le_bytes(c))
            .collect();

        for (i, (s, v)) in scalar_i32.iter().zip(sse2_i32.iter()).enumerate() {
            let diff = (s.abs_diff(*v)) as i64;
            // SSE2 i32 路径同样 clamp 到 2147483520.0,与 AVX2 path 行为一致
            assert!(
                diff <= 128,
                "SSE2 i32 差异过大 at {i}: scalar={s}, sse2={v}, diff={diff}"
            );
        }
    }

    /// 验证 SSE2 与 AVX2 path 输出完全一致(两者用相同舍入逻辑)
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_avx2_i16_identical() {
        if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
            return; // 无 AVX2 时跳过(不能调用 AVX2 path)
        }
        let samples = make_test_samples(128);
        let mut sse2_out = Vec::new();
        let mut avx2_out = Vec::new();

        unsafe {
            f32_to_i16_bytes_sse2(&samples, &mut sse2_out);
            f32_to_i16_bytes_avx2(&samples, &mut avx2_out);
        }

        assert_eq!(sse2_out, avx2_out, "SSE2 与 AVX2 i16 输出应完全一致");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn test_sse2_avx2_i32_identical() {
        if !(std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma")) {
            return;
        }
        let samples = make_test_samples(128);
        let mut sse2_out = Vec::new();
        let mut avx2_out = Vec::new();

        unsafe {
            f32_to_i32_bytes_sse2(&samples, &mut sse2_out);
            f32_to_i32_bytes_avx2(&samples, &mut avx2_out);
        }

        assert_eq!(sse2_out, avx2_out, "SSE2 与 AVX2 i32 输出应完全一致");
    }
}
