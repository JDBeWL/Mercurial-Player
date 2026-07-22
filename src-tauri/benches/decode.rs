//! 解码吞吐 benchmark
//!
//! 测量 SymphoniaDecoder 解码不同格式/参数时的吞吐量
//!
//! 运行: cargo bench --bench decode
//! 输出: target/criterion/decode/report/index.html

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use mercurial_player::audio::SymphoniaDecoder;
use std::io::Write;

/// 生成测试用 WAV 文件 (sine wave)
///
/// 不依赖外部 crate,直接按 RIFF/WAVE 格式手动写入
fn generate_test_wav(path: &str, duration_secs: f32, sample_rate: u32, channels: u16) -> std::io::Result<()> {
    let total_frames = (sample_rate as f32 * duration_secs) as usize;
    let bits_per_sample: u16 = 16;
    let data_size = total_frames * channels as usize * (bits_per_sample as usize / 8);
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;

    let mut file = std::fs::File::create(path)?;
    // RIFF header
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size as u32).to_le_bytes())?;
    file.write_all(b"WAVE")?;
    // fmt chunk
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    // data chunk
    file.write_all(b"data")?;
    file.write_all(&(data_size as u32).to_le_bytes())?;
    // 生成 440Hz sine wave 数据
    for i in 0..total_frames {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        let int_sample = (sample * i16::MAX as f32) as i16;
        for _ in 0..channels {
            file.write_all(&int_sample.to_le_bytes())?;
        }
    }
    Ok(())
}

/// 计算音频数据字节数 (用于 throughput 统计)
fn wav_data_size(duration_secs: f32, sample_rate: u32, channels: u16) -> u64 {
    let total_frames = (sample_rate as f32 * duration_secs) as u64;
    total_frames * channels as u64 * 2 // 16-bit = 2 bytes
}

/// 确保 WAV fixture 存在,返回文件路径
fn ensure_wav_fixture(name: &str, duration_secs: f32, sample_rate: u32, channels: u16) -> String {
    let dir = std::env::temp_dir().join("mercurial_bench");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.wav"));
    if !path.exists() {
        generate_test_wav(path.to_str().unwrap(), duration_secs, sample_rate, channels)
            .expect("Failed to generate test WAV");
    }
    path.to_str().unwrap().to_string()
}

/// 解码整个文件直到 EOF,返回总采样数
fn decode_all(path: &str) -> usize {
    let mut decoder = SymphoniaDecoder::new(path).expect("Failed to create decoder");
    let mut count = 0;
    while black_box(decoder.next()).is_some() {
        count += 1;
    }
    count
}

fn bench_decode(c: &mut Criterion) {
    // 生成不同参数的测试文件
    // 10 秒足够测量稳定吞吐,又不会太慢
    let fixtures: Vec<(&str, f32, u32, u16)> = vec![
        ("wav_44100_stereo_10s", 10.0, 44100, 2),
        ("wav_48000_stereo_10s", 10.0, 48000, 2),
        ("wav_96000_stereo_10s", 10.0, 96000, 2),
        ("wav_48000_mono_10s", 10.0, 48000, 1),
    ];

    let mut group = c.benchmark_group("decode");
    group.sample_size(20); // 20 次采样足够稳定

    for (name, duration, sr, ch) in &fixtures {
        let path = ensure_wav_fixture(name, *duration, *sr, *ch);
        let data_bytes = wav_data_size(*duration, *sr, *ch);
        group.throughput(Throughput::Bytes(data_bytes));

        group.bench_with_input(
            BenchmarkId::new(*name, format!("{sr}Hz_{ch}ch")),
            &path,
            |b, path| {
                b.iter(|| decode_all(path));
            },
        );
    }

    group.finish();
}

/// 测量 seek 后的解码性能 (模拟切歌场景)
fn bench_decode_after_seek(c: &mut Criterion) {
    let path = ensure_wav_fixture("wav_48000_stereo_10s", 10.0, 48000, 2);

    let mut group = c.benchmark_group("decode_seek");
    group.sample_size(20);
    group.throughput(Throughput::Elements(48000 * 2 * 5)); // 5 秒数据

    group.bench_function("seek_to_5s_decode_5s", |b| {
        b.iter(|| {
            let mut decoder = SymphoniaDecoder::new(&path).expect("Failed to create decoder");
            // Seek 到 5 秒位置
            decoder.seek(std::time::Duration::from_secs(5)).expect("Seek failed");
            // 解码剩余 5 秒
            let mut count = 0;
            while black_box(decoder.next()).is_some() {
                count += 1;
                if count >= 48000 * 2 * 5 {
                    break;
                }
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_decode, bench_decode_after_seek);
criterion_main!(benches);
