//! SIMD 加速的样本转换：AVX2/FMA + SSE2 intrinsics 做 f32 到 i16/i32 的字节转换。
//!
//! SIMD intrinsics 必须 unsafe，故在模块级统一 allow；
//! 该 allow 仅限本模块，不影响其他代码的 unsafe_code 审查。

#![allow(unsafe_code)]

/// 将 f32 采样转换为指定格式的字节,写入复用 buffer(零分配)
pub fn convert_samples_to_bytes_into(
    samples: &[f32],
    bits: u16,
    is_float: bool,
    out: &mut Vec<u8>,
) {
    // 预先计算所需容量,避免多次扩容
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
            // 32-bit float: f32 的内存表示即 LE 字节,可整块 memcpy
            // 安全性: f32 与 [u8; 4] 都是 POD,size 一致(f32 恒为 4 字节)
            let bytes = unsafe {
                core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 4)
            };
            out.extend_from_slice(bytes);
        }
        (32, false) => {
            // f32 → i32 (AVX2 加速,无 AVX2 时回落 SSE2)
            #[cfg(target_arch = "x86_64")]
            {
                if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                    unsafe { f32_to_i32_bytes_avx2(samples, out) };
                    return;
                }
                // SSE2 是 x86_64 baseline,所有 64 位 Intel/AMD CPU 必然支持
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
            // 24-bit: 字节打包(取 i32 低 3 字节)难以 SIMD 化,暂用标量
            for &s in samples {
                let int_val = (s.clamp(-1.0, 1.0) * 8_388_607.0) as i32;
                let bytes = int_val.to_le_bytes();
                out.extend_from_slice(&bytes[0..3]);
            }
        }
        (16, _) => {
            // f32 → i16 (AVX2 加速,无 AVX2 时回落 SSE2)
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
            let bytes = unsafe {
                core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), samples.len() * 4)
            };
            out.extend_from_slice(bytes);
        }
    }
}

// SIMD 优化: f32 → i16/i32 字节流
// 三层分发架构(运行时由 is_x86_feature_detected! 选择):
// 1. AVX2 + FMA path (Haswell 2013+ / Zen 2017+) - 一次 8/16 个 f32,最快
// 2. SSE2 path (所有 x86_64 CPU,含老至强) - 一次 4/8 个 f32,中等加速
// 3. 标量 fallback (非 x86_64 平台) - 逐样本循环
//
// 关键技术细节:
// - _mm_cvtps_epi32 / _mm256_cvtps_epi32 在输入超过 i32 范围时返回 0x80000000
//   (saturation indefinite),故 i32 路径必须先 clamp 到 2147483520.0
//   (小于 2^31 的最大 f32 = 2^31 - 128)
// - _mm256_packs_epi32 存在 256-bit lane 交错,需 _mm256_permute4x64_epi64(0xD8) 修复
//   SSE2 的 _mm_packs_epi32 只有 128-bit 单 lane,无交错问题
// - 舍入模式: Rust `as i32/i16` 是 truncation-toward-zero,
//   MXCSR 默认 RNE (round to nearest even),在 .5 边界处差 1 LSB

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
#[allow(unsafe_op_in_unsafe_fn)] // 整个函数由 target_feature 限定为 unsafe,内联 unsafe 块冗余
#[allow(clippy::wildcard_imports)] // SIMD intrinsics 数量多,逐个导入冗长
pub(super) unsafe fn f32_to_i16_bytes_avx2(samples: &[f32], out: &mut Vec<u8>) {
    use core::arch::x86_64::*;

    let one = _mm256_set1_ps(1.0);
    let neg_one = _mm256_set1_ps(-1.0);
    let scale = _mm256_set1_ps(i16::MAX as f32); // 32767.0

    let mut i = 0;
    let chunk = 16; // 16 个 f32 → 32 bytes (i16)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        let b = _mm256_loadu_ps(samples.as_ptr().add(i + 8));

        // clamp(-1, 1) * scale
        let a = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, a)), scale);
        let b = _mm256_mul_ps(_mm256_max_ps(neg_one, _mm256_min_ps(one, b)), scale);

        // f32 → i32 (round to nearest even)
        let a_i32 = _mm256_cvtps_epi32(a);
        let b_i32 = _mm256_cvtps_epi32(b);

        // pack i32 → i16 (saturating) + 修复 lane 交错
        // packs_epi32(a, b) 输出顺序: [a0..3, b0..3, a4..7, b4..7]
        // permute 0xD8 (= 0b11_01_10_00) 重排为: [a0..7, b0..7]
        let packed = _mm256_packs_epi32(a_i32, b_i32);
        let packed = _mm256_permute4x64_epi64(packed, 0xD8);

        // __m256i → [u8; 32]: 同 size(32B) POD 转换
        let bytes: [u8; 32] = core::mem::transmute(packed);
        out.extend_from_slice(&bytes);

        i += chunk;
    }

    // 处理剩余尾部
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
    // 2147483648.0 触发 cvtps_epi32 的 saturation indefinite(返回 0x80000000)
    // 故 clamp 到 2147483520.0(小于 2^31 的最大 f32 = 2^31 - 128)
    let max_safe = _mm256_set1_ps(2_147_483_520.0);
    let min_safe = _mm256_set1_ps(-2_147_483_648.0);

    let mut i = 0;
    let chunk = 8; // 8 个 f32 → 32 bytes (i32)

    while i + chunk <= samples.len() {
        let a = _mm256_loadu_ps(samples.as_ptr().add(i));
        // clamp(-1, 1) * scale,再 clamp 到 i32 安全范围
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

// SSE2 path: 所有 x86_64 CPU 的兜底加速(baseline feature),一次处理 4/8 个 f32

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
    let chunk = 8; // 8 个 f32 → 16 bytes (i16)

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
    let chunk = 4; // 4 个 f32 → 16 bytes (i32)

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

    /// 验证 AVX2 路径与标量路径产生相同字节流(允许 i32 路径 1 LSB 差异)
    /// 样本总数对齐到 16 的倍数,确保 SSE2(chunk=8) 和 AVX2(chunk=16)
    /// 的 SIMD path 都不进入尾部标量循环,从而可以精确对比两条 SIMD path
    fn make_test_samples(n: usize) -> Vec<f32> {
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / n as f32;
            // 覆盖 [-1, 1] 全范围,含边界
            samples.push((t * 2.0 - 1.0).clamp(-1.0, 1.0));
        }
        // 包含一些特殊值: 0、极值、超过 1.0 的值(测试 clamp)
        // 总数 16 个,确保 SIMD path 不进入尾部标量循环
        samples.extend([
            0.0_f32, 1.0, -1.0, 1.5, -1.5, // clamp 边界测试
            0.5, -0.5, 0.25, -0.25, 0.125, -0.125, // .5 边界舍入测试
            0.0, 0.0, 0.0, 0.0, 0.0, // 填充对齐
        ]);
        // 断言总长度是 16 的倍数(SSE2 chunk=8, AVX2 chunk=16 的 LCM)
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
            // 标量 `as i16` 与 SIMD _mm256_cvtps_epi32 在 .5 边界处采用不同舍入模式
            // (Rust as 是 truncation-toward-zero, MXCSR 默认 RNE),最大差异 1 LSB
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

        // i32 路径: SIMD 用 2147483520.0 作上限,标量用 i32::MAX as f32 饱和
        // 输入 = 1.0 时:标量得 i32::MAX(2147483647),SIMD 得 2147483520,差 127
        // 逐字节比较,允许差异时跳过
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
            // SIMD scale 比标量小最多 128,允许 1 LSB 误差
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

        // 32-bit float 应该是直接的字节拷贝
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

    /// 直接测试 SSE2 path(强制使用,绕过 is_x86_feature_detected 检测)
    /// 确保老 CPU 上的兜底路径输出正确
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

        // 直接调用 SSE2 path
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
            // SSE2 _mm_cvtps_epi32 与 AVX2 一样使用 MXCSR 默认 RNE
            // 标量 `as i16` 是 truncation-toward-zero,.5 边界处差 1 LSB
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
