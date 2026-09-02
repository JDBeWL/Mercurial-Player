//! 均衡器处理吞吐 benchmark
//!
//! 测量生产 EQ 处理器 [`EqProcessor`] 的单样本/批量处理开销
//! (共享模式批量处理与独占模式逐采样处理共用此实现)。
//!
//! 运行: cargo bench --bench equalizer
//! 输出: target/criterion/equalizer/report/index.html

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use mercurial_player::audio::EqProcessor;
use mercurial_player::equalizer::{EQ_BAND_COUNT, EqSettings};

/// 生成测试用音频采样 (440Hz sine wave)
fn generate_samples(count: usize, sample_rate: u32) -> Vec<f32> {
    (0..count)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            // 交织的双声道: i%2 控制左右声道
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
        })
        .collect()
}

/// 创建已配置的 EqProcessor
fn make_eq(
    sample_rate: u32,
    channels: u16,
    enabled: bool,
    gains: [f32; EQ_BAND_COUNT],
) -> EqProcessor {
    let settings = EqSettings {
        enabled,
        gains,
        preamp: 0.0,
    };
    let mut eq = EqProcessor::new(sample_rate, channels);
    eq.update_settings(&settings);
    eq
}

/// 单样本 EQ 处理 (process_sample)
///
/// 这是独占模式逐采样处理的方式
fn bench_process_sample(c: &mut Criterion) {
    let sample_rate = 48000u32;
    let channels = 2u16;
    let samples = generate_samples(48000 * 2, sample_rate); // 1 秒数据

    let mut group = c.benchmark_group("eq_process_sample");
    group.throughput(Throughput::Elements(samples.len() as u64));

    // 禁用 EQ (baseline)
    group.bench_function("disabled", |b| {
        b.iter(|| {
            let mut eq = make_eq(sample_rate, channels, false, [0.0; EQ_BAND_COUNT]);
            for (i, s) in samples.iter().enumerate() {
                let channel = i % channels as usize;
                eq.process_sample(black_box(*s), channel);
            }
        });
    });

    // 启用 EQ,Flat 预设 (全 0 dB 增益)
    group.bench_function("enabled_flat", |b| {
        b.iter(|| {
            let mut eq = make_eq(sample_rate, channels, true, [0.0; EQ_BAND_COUNT]);
            for (i, s) in samples.iter().enumerate() {
                let channel = i % channels as usize;
                eq.process_sample(black_box(*s), channel);
            }
        });
    });

    // 启用 EQ,Bass Boost 预设 (有实际增益)
    group.bench_function("enabled_bass_boost", |b| {
        b.iter(|| {
            let mut eq = make_eq(
                sample_rate,
                channels,
                true,
                [6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            );
            for (i, s) in samples.iter().enumerate() {
                let channel = i % channels as usize;
                eq.process_sample(black_box(*s), channel);
            }
        });
    });

    group.finish();
}

/// 批量 EQ 处理 (process_batch)
///
/// 这是共享模式的处理方式 (通过 VisualizationSource)
fn bench_process_buffer(c: &mut Criterion) {
    let sample_rate = 48000u32;
    let channels = 2u16;
    let samples = generate_samples(48000 * 2, sample_rate); // 1 秒数据

    let mut group = c.benchmark_group("eq_process_buffer");
    group.throughput(Throughput::Elements(samples.len() as u64));

    // 不同 batch size 的影响 (batch size 保持声道帧对齐)
    for batch_size in [64, 256, 1024, 4096, 16384] {
        // 启用 EQ,Bass Boost
        group.bench_with_input(
            BenchmarkId::new("bass_boost", format!("batch_{batch_size}")),
            &batch_size,
            |b, &batch_size| {
                b.iter(|| {
                    let mut eq = make_eq(
                        sample_rate,
                        channels,
                        true,
                        [6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    );
                    let mut buf = samples.clone();
                    // 处理整个 1 秒数据,每次 batch_size 个样本
                    for chunk in buf.chunks_mut(batch_size) {
                        eq.process_batch(black_box(chunk));
                    }
                });
            },
        );
    }

    group.finish();
}

/// 设置应用开销
fn bench_apply_settings(c: &mut Criterion) {
    let mut group = c.benchmark_group("eq_apply_settings");
    group.throughput(Throughput::Elements(1));

    let presets = [
        ("flat", [0.0; EQ_BAND_COUNT]),
        (
            "bass_boost",
            [6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        ("vocal", [0.0, 0.0, 2.0, 4.0, 4.0, 4.0, 3.0, 2.0, 0.0, 0.0]),
    ];

    for (name, gains) in &presets {
        group.bench_function(*name, |b| {
            b.iter(|| {
                let _eq = make_eq(48000, 2, true, *black_box(gains));
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_process_sample,
    bench_process_buffer,
    bench_apply_settings
);
criterion_main!(benches);
